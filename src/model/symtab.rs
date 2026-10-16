//! Symbols, sections and address-range membership of the main executable and
//! the shared objects mapped during the recording; implements `loc(addr)`.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use iced_x86::{Decoder, DecoderOptions, Mnemonic, OpKind};
use object::{Object, ObjectSection, ObjectSegment, ObjectSymbol, RelocationTarget, SectionFlags, SectionKind, SymbolKind, SymbolSection};

use crate::trace::Trace;

#[derive(Debug, Clone)]
pub struct SymEntry {
    pub name: String,
    /// Runtime address.
    pub addr: u64,
    pub size: u64,
    /// Code symbol (as opposed to data).
    pub func: bool,
    /// A local label of hand-written assembly (`again:`): it names addresses (`loc`) but neither starts
    /// a function nor ends the one it sits in.
    pub label: bool,
}

struct Lib {
    file_name: String,
    path: String,
    bias: u64,
    syms: Vec<SymEntry>,
    ranges: Vec<(u64, u64)>,
}

struct Section {
    start: u64,
    end: u64,
    name: String,
}

pub struct SymTab {
    exe_path: String,
    exe_name: String,
    bias: u64,
    exe_ranges: Vec<(u64, u64)>,
    /// Every symbol of the executable, labels included.
    exe_syms: Vec<SymEntry>,
    /// `exe_syms` without the labels: what disassembly, frames and `exe_functions` call functions.
    exe_funcs: Vec<SymEntry>,
    sections: Vec<Section>,
    libs: Vec<Lib>,
}

fn sort_syms(v: &mut [SymEntry]) {
    v.sort_by(|a, b| {
        let ua = a.name.bytes().take_while(|&c| c == b'_').count();
        let ub = b.name.bytes().take_while(|&c| c == b'_').count();
        (a.addr, a.label, ua, &a.name).cmp(&(b.addr, b.label, ub, &b.name))
    });
}

/// The symbol of `syms` (sorted by `sort_syms`) containing `addr`; among aliases the first.
fn find_sym(syms: &[SymEntry], addr: u64) -> Option<&SymEntry> {
    let i = syms.partition_point(|s| s.addr <= addr);
    let mut j = i;
    let mut found = None;
    let mut tries = 0;
    while j > 0 && tries < 16 {
        j -= 1;
        tries += 1;
        if addr - syms[j].addr < syms[j].size {
            found = Some(j);
            break;
        }
    }
    let mut j = found?;
    while j > 0 && syms[j - 1].addr == syms[j].addr && addr - syms[j - 1].addr < syms[j - 1].size {
        j -= 1;
    }
    Some(&syms[j])
}

fn within(ranges: &[(u64, u64)], addr: u64) -> bool {
    ranges.iter().any(|&(s, e)| addr >= s && addr < e)
}

fn offset_name(name: &str, off: u64) -> String {
    if off == 0 { name.to_string() } else { format!("{name}+0x{off:x}") }
}

/// `NAME@plt` entries for the PLT stubs of the executable (objdump invents these names; the ELF has no
/// symbol for them). A stub is a `jmp *SLOT(%rip)` whose SLOT is a GOT entry that a dynamic relocation
/// binds to NAME, starting at the `endbr64` right before it when there is one.
fn plt_symbols(file: &object::File<'_>, bias: u64) -> Vec<SymEntry> {
    let names: HashMap<_, String> = file.dynamic_symbols().filter_map(|s| Some((s.index(), s.name().ok()?.to_string()))).collect();
    let mut got: HashMap<u64, &str> = HashMap::new();
    for (offset, rel) in file.dynamic_relocations().into_iter().flatten() {
        if let RelocationTarget::Symbol(i) = rel.target()
            && let Some(name) = names.get(&i).filter(|n| !n.is_empty())
        {
            got.entry(offset.wrapping_add(bias)).or_insert(name);
        }
    }
    let mut out = Vec::new();
    if got.is_empty() {
        return out;
    }
    for section in [".plt", ".plt.sec", ".plt.got"] {
        let Some(sec) = file.section_by_name(section) else { continue };
        let Ok(data) = sec.data() else { continue };
        let start = sec.address().wrapping_add(bias);
        let end = start + data.len() as u64;
        let mut dec = Decoder::with_ip(64, data, start, DecoderOptions::NONE);
        let mut stubs: Vec<(u64, &str)> = Vec::new();
        let mut endbr = None;
        while dec.can_decode() {
            let ins = dec.decode();
            if ins.mnemonic() == Mnemonic::Jmp
                && ins.op0_kind() == OpKind::Memory
                && ins.is_ip_rel_memory_operand()
                && let Some(&name) = got.get(&ins.ip_rel_memory_address())
            {
                stubs.push((endbr.unwrap_or(ins.ip()), name));
            }
            endbr = (ins.mnemonic() == Mnemonic::Endbr64).then(|| ins.ip());
        }
        for (i, &(addr, name)) in stubs.iter().enumerate() {
            let next = stubs.get(i + 1).map_or(end, |&(a, _)| a);
            out.push(SymEntry { name: format!("{name}@plt"), addr, size: next - addr, func: true, label: false });
        }
    }
    out
}

/// Symbols of the executable: FUNC/OBJECT with a size (from `.symtab`, else `.dynsym`), the PLT stubs, plus
/// size-0 symbols in code sections (hand-written assembly). A size-0 symbol that is local, untyped and not
/// the target of a recorded `call` is a label (`again:`): it extends to the next symbol of any kind, while
/// the other size-0 symbols extend to the next non-label.
fn exe_symbols(file: &object::File<'_>, bias: u64, call_targets: &HashSet<u64>) -> Vec<SymEntry> {
    struct Cand {
        name: String,
        addr: u64,
        size: u64,
        func: bool,
        label: bool,
        section: Option<object::SectionIndex>,
    }
    let all: Vec<object::Symbol<'_, '_>> = if file.section_by_name(".symtab").is_some() {
        file.symbols().collect()
    } else {
        file.dynamic_symbols().collect()
    };
    let mut cands: Vec<Cand> = Vec::new();
    for s in all {
        if s.is_undefined() {
            continue;
        }
        let Ok(name) = s.name() else { continue };
        if name.is_empty() || name.starts_with(".L") {
            continue;
        }
        let section = match s.section() {
            SymbolSection::Section(i) => Some(i),
            _ => None,
        };
        let in_text = section.and_then(|i| file.section_by_index(i).ok()).is_some_and(|sec| sec.kind() == SectionKind::Text);
        let keep = match s.kind() {
            SymbolKind::Text | SymbolKind::Data => s.size() > 0 || (s.kind() == SymbolKind::Text && in_text),
            SymbolKind::Unknown => s.size() == 0 && in_text,
            _ => false,
        };
        if keep {
            let addr = s.address().wrapping_add(bias);
            cands.push(Cand {
                name: name.to_string(),
                addr,
                size: s.size(),
                func: s.kind() != SymbolKind::Data,
                label: s.kind() == SymbolKind::Unknown && !s.is_global() && !call_targets.contains(&addr),
                section,
            });
        }
    }
    let starts = |keep: &dyn Fn(&Cand) -> bool| {
        let mut v: Vec<(Option<usize>, u64)> = cands.iter().filter(|c| keep(c)).map(|c| (c.section.map(|i| i.0), c.addr)).collect();
        v.sort();
        v.dedup();
        v
    };
    let (any_starts, fn_starts) = (starts(&|_| true), starts(&|c| !c.label));
    for c in cands.iter_mut().filter(|c| c.size == 0) {
        let sec_end = c
            .section
            .and_then(|i| file.section_by_index(i).ok())
            .map_or(c.addr, |s| s.address().wrapping_add(bias) + s.size());
        let csec = c.section.map(|i| i.0);
        let starts = if c.label { &any_starts } else { &fn_starts };
        let k = starts.partition_point(|&(sec, a)| (sec, a) <= (csec, c.addr));
        let next = starts.get(k).filter(|&&(sec, _)| sec == csec).map_or(sec_end, |&(_, a)| a.min(sec_end));
        c.size = next.saturating_sub(c.addr);
    }
    let mut out: Vec<SymEntry> = cands
        .into_iter()
        .filter(|c| c.size > 0)
        .map(|c| SymEntry { name: c.name, addr: c.addr, size: c.size, func: c.func, label: c.label })
        .collect();
    out.extend(plt_symbols(file, bias));
    sort_syms(&mut out);
    out
}

impl SymTab {
    /// `call_targets`: addresses some recorded `call` jumped to; they are never labels.
    pub fn build(trace: &Trace, call_targets: &HashSet<u64>) -> Result<SymTab> {
        let bias = trace.exe.bias;
        let file = object::File::parse(&trace.exe.bytes[..]).context("parsing the embedded executable")?;
        let exe_ranges = file
            .segments()
            .filter(|s| s.size() > 0)
            .map(|s| (s.address().wrapping_add(bias), s.address().wrapping_add(bias) + s.size()))
            .collect();
        let mut sections = Vec::new();
        for s in file.sections() {
            let alloc = matches!(s.flags(), SectionFlags::Elf { sh_flags, .. } if sh_flags.0 & 2 != 0);
            if !alloc || s.address() == 0 || s.size() == 0 || s.kind() == SectionKind::UninitializedTls || s.kind() == SectionKind::Tls {
                continue;
            }
            let Ok(name) = s.name() else { continue };
            let start = s.address().wrapping_add(bias);
            sections.push(Section { start, end: start + s.size(), name: name.to_string() });
        }
        sections.sort_by_key(|s| s.start);
        let exe_syms = exe_symbols(&file, bias, call_targets);
        let exe_funcs = exe_syms.iter().filter(|s| !s.label).cloned().collect();
        let exe_name = std::path::Path::new(&trace.exe.path)
            .file_name()
            .map_or_else(|| trace.exe.path.clone(), |n| n.to_string_lossy().into_owned());
        let libs = trace
            .libs
            .iter()
            .map(|l| {
                let mut ranges: Vec<(u64, u64)> = trace
                    .maps
                    .iter()
                    .flat_map(|m| m.regions.iter())
                    .filter(|r| r.path == l.path)
                    .map(|r| (r.start, r.end))
                    .collect();
                ranges.sort();
                ranges.dedup();
                let mut syms: Vec<SymEntry> = l
                    .syms
                    .iter()
                    .map(|s| SymEntry { name: s.name.clone(), addr: l.bias.wrapping_add(s.addr), size: s.size, func: true, label: false })
                    .collect();
                sort_syms(&mut syms);
                Lib { file_name: l.file_name.clone(), path: l.path.clone(), bias: l.bias, syms, ranges }
            })
            .collect();
        Ok(SymTab { exe_path: trace.exe.path.clone(), exe_name, bias, exe_ranges, exe_syms, exe_funcs, sections, libs })
    }

    pub fn in_exe(&self, addr: u64) -> bool {
        within(&self.exe_ranges, addr)
    }

    /// Inside the executable or a mapped shared object.
    pub fn in_elf(&self, addr: u64) -> bool {
        self.in_exe(addr) || self.libs.iter().any(|l| within(&l.ranges, addr))
    }

    /// Whether `path` (from a maps region) names the executable or a known shared object.
    pub fn is_elf_path(&self, path: &str) -> bool {
        path == self.exe_path || self.libs.iter().any(|l| l.path == path)
    }

    /// Code symbols of the executable (PLT stubs included, labels not), one per address, ascending.
    pub fn exe_functions(&self) -> Vec<&SymEntry> {
        let mut out: Vec<&SymEntry> = Vec::new();
        for s in self.exe_funcs.iter().filter(|s| s.func) {
            if out.last().is_none_or(|l| l.addr != s.addr) {
                out.push(s);
            }
        }
        out
    }

    /// The function containing `addr` (local assembly labels do not count) and the shared object it lives
    /// in (`None` for the executable).
    pub fn symbol_at(&self, addr: u64) -> Option<(&SymEntry, Option<&str>)> {
        if self.in_exe(addr) {
            return find_sym(&self.exe_funcs, addr).map(|s| (s, None));
        }
        let lib = self.libs.iter().find(|l| within(&l.ranges, addr))?;
        find_sym(&lib.syms, addr).map(|s| (s, Some(lib.file_name.as_str())))
    }

    /// Deterministic text for an address, see the plan's Contract (`loc`); names labels as well as functions.
    pub fn loc(&self, addr: u64) -> String {
        if self.in_exe(addr) {
            if let Some(s) = find_sym(&self.exe_syms, addr) {
                return offset_name(&s.name, addr - s.addr);
            }
            if let Some(sec) = self.sections.iter().find(|s| addr >= s.start && addr < s.end) {
                return format!("{}+0x{:x}", sec.name, addr - sec.start);
            }
            return format!("{}+0x{:x}", self.exe_name, addr.wrapping_sub(self.bias));
        }
        if let Some(lib) = self.libs.iter().find(|l| within(&l.ranges, addr)) {
            if let Some(s) = find_sym(&lib.syms, addr) {
                return format!("{}:{}", lib.file_name, offset_name(&s.name, addr - s.addr));
            }
            return format!("{}+0x{:x}", lib.file_name, addr.wrapping_sub(lib.bias));
        }
        format!("0x{addr:x}")
    }

    /// The function name containing `addr` (`libfile:name` in shared objects); `loc(addr)` without a symbol.
    pub fn func_name(&self, addr: u64) -> String {
        match self.symbol_at(addr) {
            Some((s, None)) => s.name.clone(),
            Some((s, Some(lib))) => format!("{lib}:{}", s.name),
            None => self.loc(addr),
        }
    }

    /// Address and size of the symbol `name` (`name` of the executable, label or PLT stub included, or `libfile:name`).
    pub fn lookup(&self, name: &str) -> Option<(u64, u64)> {
        if let Some((lib, sym)) = name.split_once(':') {
            let l = self.libs.iter().find(|l| l.file_name == lib)?;
            return l.syms.iter().find(|s| s.name == sym).map(|s| (s.addr, s.size));
        }
        self.exe_syms.iter().find(|s| s.name == name).map(|s| (s.addr, s.size))
    }
}
