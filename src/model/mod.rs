//! Analysis model over a recorded trace: state reconstruction, addresses and
//! values rendering, locators, activations, stack view, ABI checks, navigation.

mod abi;
mod activations;
mod api;
mod asm;
pub mod compare;
mod dwarf;
mod effects;
mod events;
mod expr;
pub mod locator;
mod nav;
mod stack;
mod symtab;
mod views;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, OnceLock};

use anyhow::Result;

use crate::exit;
use crate::trace::{self, CHECKPOINT_INTERVAL, Event, EventKind, MemWrite, MapsSnapshot, PAGE_SIZE, RegDelta, RegFile, Region, StepKind, Trace, WriteKind, reg};

pub use abi::Finding;
pub use activations::Activation;
pub use expr::WatchType;
pub use nav::{NavOp, NavParams};
pub use stack::{FrameView, RootFrame, SlotView, StackView};
pub use views::*;

use activations::Activations;
use asm::{Fmt, InsnInfo};
use dwarf::DwarfInfo;
use locator::{AddrSpec, Locator};
use symtab::SymTab;

/// EFLAGS bits in display order.
pub const FLAGS: [(&str, u32); 9] =
    [("CF", 0), ("PF", 2), ("AF", 4), ("ZF", 6), ("SF", 7), ("TF", 8), ("IF", 9), ("DF", 10), ("OF", 11)];

pub fn flag_names(rflags: u64) -> Vec<String> {
    FLAGS.iter().filter(|&&(_, bit)| rflags >> bit & 1 == 1).map(|&(n, _)| n.to_string()).collect()
}

/// The name a write's kind has in the JSON views.
fn write_kind_name(kind: WriteKind) -> &'static str {
    match kind {
        WriteKind::Snapshot => "snapshot",
        WriteKind::Instr => "instr",
        WriteKind::Kernel => "kernel",
        WriteKind::Unattributed => "unattributed",
    }
}

struct InsnText {
    asm: String,
    mnemonic: String,
}

pub struct LoadedTrace {
    pub trace: Trace,
    n: u32,
    /// Page address to indices into `trace.writes` (ascending, so ascending `seq`).
    page_index: HashMap<u64, Vec<u32>>,
    syms: Arc<SymTab>,
    dwarf: DwarfInfo,
    insn_by_addr: HashMap<u64, u32>,
    insn_info: Vec<InsnInfo>,
    insn_text: Vec<OnceLock<InsnText>>,
    insn_line: Vec<Option<(u32, u32)>>,
    /// Instruction indices ordered by address.
    insns_sorted: Vec<u32>,
    /// Steps (ascending) executing each instruction, indexed like `trace.insns`.
    occ: Vec<Vec<u32>>,
    /// Timeline markers, computed on first use.
    markers: OnceLock<Vec<Marker>>,
    /// Display name per DWARF file index.
    file_display: Vec<String>,
    stack0: u64,
    acts: Activations,
}

impl LoadedTrace {
    pub fn load(path: &Path) -> Result<LoadedTrace> {
        LoadedTrace::from_trace(trace::load(path)?)
    }

    pub fn from_trace(trace: Trace) -> Result<LoadedTrace> {
        let n = trace.steps.len() as u32;
        let mut page_index: HashMap<u64, Vec<u32>> = HashMap::new();
        for (i, w) in trace.writes.iter().enumerate() {
            page_index.entry(w.addr & !(PAGE_SIZE - 1)).or_default().push(i as u32);
        }
        let insn_info: Vec<InsnInfo> = trace.insns.iter().map(|x| asm::analyze(&x.bytes, x.addr)).collect();
        // Where recorded near calls landed: those addresses start functions, even when the symbol table
        // has them as untyped local labels.
        let call_targets: HashSet<u64> = trace
            .steps
            .windows(2)
            .filter(|w| w[0].kind == StepKind::Instr && insn_info[w[0].insn as usize].call)
            .map(|w| trace.insns[w[1].insn as usize].addr)
            .collect();
        let syms = Arc::new(SymTab::build(&trace, &call_targets)?);
        let dwarf = DwarfInfo::build(&trace.exe.bytes, trace.exe.bias);
        let insn_by_addr = trace.insns.iter().enumerate().map(|(i, x)| (x.addr, i as u32)).collect();
        let insn_text = trace.insns.iter().map(|_| OnceLock::new()).collect();
        let insn_line = trace.insns.iter().map(|x| dwarf.line_at(x.addr)).collect();
        let mut insns_sorted: Vec<u32> = (0..trace.insns.len() as u32).collect();
        insns_sorted.sort_by_key(|&i| trace.insns[i as usize].addr);
        let mut occ = vec![Vec::new(); trace.insns.len()];
        for (i, s) in trace.steps.iter().enumerate() {
            occ[s.insn as usize].push(i as u32);
        }
        let file_display = dwarf
            .files
            .iter()
            .map(|path| {
                let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned());
                trace
                    .sources
                    .iter()
                    .find(|s| s.path == *path)
                    .or_else(|| {
                        let mut same = trace.sources.iter().filter(|s| Path::new(&s.path).file_name().map(|n| n.to_string_lossy().into_owned()) == name);
                        let first = same.next();
                        if same.next().is_none() { first } else { None }
                    })
                    .map_or_else(|| path.clone(), |s| s.display.clone())
            })
            .collect();
        let mut lt = LoadedTrace {
            trace,
            n,
            page_index,
            syms,
            dwarf,
            insn_by_addr,
            insn_info,
            insn_text,
            insn_line,
            insns_sorted,
            occ,
            markers: OnceLock::new(),
            file_display,
            stack0: 0,
            acts: Activations::default(),
        };
        lt.stack0 = lt.regs_at(0).rsp();
        lt.acts = activations::build(&lt);
        Ok(lt)
    }

    /// Number of steps `N`; states are `0..=N`.
    pub fn steps(&self) -> u32 {
        self.n
    }

    pub fn stack0(&self) -> u64 {
        self.stack0
    }

    pub fn has_dwarf(&self) -> bool {
        self.dwarf.has_dwarf()
    }

    // ---- registers ----

    /// Registers in `state(s)`.
    pub fn regs_at(&self, s: u32) -> RegFile {
        let s = s.min(self.n);
        let cps = &self.trace.reg_checkpoints;
        let j = ((s / CHECKPOINT_INTERVAL) as usize).min(cps.len() - 1);
        let base = j as u32 * CHECKPOINT_INTERVAL;
        let mut r = cps[j];
        let d = &self.trace.reg_deltas;
        let lo = d.partition_point(|x| x.seq <= base);
        let hi = d.partition_point(|x| x.seq <= s);
        for x in &d[lo..hi] {
            r.set(x.reg, x.value);
        }
        r
    }

    /// Register deltas produced by step `i` (`seq == i + 1`).
    pub fn deltas_of_step(&self, i: u32) -> &[RegDelta] {
        let d = &self.trace.reg_deltas;
        let lo = d.partition_point(|x| x.seq <= i);
        let hi = d.partition_point(|x| x.seq <= i + 1);
        &d[lo..hi]
    }

    // ---- memory ----

    pub fn maps_at(&self, s: u32) -> &MapsSnapshot {
        let m = &self.trace.maps;
        let i = m.partition_point(|x| x.seq <= s);
        &m[i.saturating_sub(1)]
    }

    fn region_at(snapshot: &MapsSnapshot, addr: u64) -> Option<&Region> {
        let i = snapshot.regions.partition_point(|r| r.start <= addr);
        snapshot.regions.get(i.checked_sub(1)?).filter(|r| addr < r.end)
    }

    /// Bytes and last writer steps of `[addr, addr+len)` in `state(s)`. A `None` byte is unmapped
    /// (or was never captured). A `None` writer means the byte comes from a snapshot.
    pub fn read_with_writers(&self, s: u32, addr: u64, len: usize) -> (Vec<Option<u8>>, Vec<Option<u32>>) {
        let mut bytes = vec![None; len];
        let mut writers = vec![None; len];
        let mut remaining = len;
        let end = addr.saturating_add(len as u64);
        let mut page = addr & !(PAGE_SIZE - 1);
        while page < end && remaining > 0 {
            if let Some(list) = self.page_index.get(&page) {
                let hi = list.partition_point(|&wi| self.trace.writes[wi as usize].seq <= s);
                for &wi in list[..hi].iter().rev() {
                    let w: &MemWrite = &self.trace.writes[wi as usize];
                    let lo = w.addr.max(addr);
                    let up = (w.addr + w.data.len() as u64).min(end);
                    for a in lo..up.max(lo) {
                        let k = (a - addr) as usize;
                        if bytes[k].is_none() {
                            bytes[k] = Some(w.data[(a - w.addr) as usize]);
                            writers[k] = (w.kind != WriteKind::Snapshot).then(|| w.seq.saturating_sub(1));
                            remaining -= 1;
                        }
                    }
                    if remaining == 0 {
                        break;
                    }
                }
            }
            let Some(next) = page.checked_add(PAGE_SIZE) else { break };
            page = next;
        }
        let snapshot = self.maps_at(s);
        for k in 0..len {
            if bytes[k].is_some() && Self::region_at(snapshot, addr + k as u64).is_none() {
                bytes[k] = None;
                writers[k] = None;
            }
        }
        (bytes, writers)
    }

    pub fn read_mem(&self, s: u32, addr: u64, len: usize) -> Vec<Option<u8>> {
        self.read_with_writers(s, addr, len).0
    }

    /// The 8 bytes at `addr` in `state(s)` as a little-endian value, if all are mapped.
    pub fn read_u64(&self, s: u32, addr: u64) -> Option<u64> {
        let b = self.read_mem(s, addr, 8);
        let mut v = [0u8; 8];
        for (i, x) in b.iter().enumerate() {
            v[i] = (*x)?;
        }
        Some(u64::from_le_bytes(v))
    }

    /// The step that last wrote the byte at `addr` in `state(s)`; `None` if only snapshots did.
    pub fn last_writer(&self, s: u32, addr: u64) -> Option<u32> {
        self.read_with_writers(s, addr, 1).1[0]
    }

    /// Writes performed by step `i` (`seq == i + 1`).
    pub fn writes_of_step(&self, i: u32) -> &[MemWrite] {
        let w = &self.trace.writes;
        let lo = w.partition_point(|x| x.seq <= i);
        let hi = w.partition_point(|x| x.seq <= i + 1);
        &w[lo..hi]
    }

    pub fn event_of(&self, i: u32) -> Option<&Event> {
        let e = &self.trace.events;
        let k = e.partition_point(|x| x.step < i);
        e.get(k).filter(|x| x.step == i)
    }

    // ---- rendering ----

    pub fn loc(&self, addr: u64) -> String {
        self.syms.loc(addr)
    }

    /// Deterministic text for a 64-bit value in `state(s)`.
    pub fn render_value(&self, v: u64, s: u32) -> String {
        if let Some(r) = Self::region_at(self.maps_at(s), v) {
            match r.path.as_str() {
                "[stack]" => {
                    let d = v.wrapping_sub(self.stack0) as i64;
                    return match d {
                        0 => "stack0".to_string(),
                        d if d > 0 => format!("stack0+0x{d:x}"),
                        d => format!("stack0-0x{:x}", d.unsigned_abs()),
                    };
                }
                "[heap]" => return format!("heap+0x{:x}", v - r.start),
                p if self.syms.is_elf_path(p) => return self.syms.loc(v),
                _ => {}
            }
        }
        let i = v as i64;
        if (-(1i64 << 31)..(1i64 << 32)).contains(&i) { i.to_string() } else { format!("0x{v:x}") }
    }

    fn insn_text(&self, idx: u32) -> &InsnText {
        self.insn_text[idx as usize].get_or_init(|| {
            let insn = &self.trace.insns[idx as usize];
            match asm::decode(&insn.bytes, insn.addr) {
                Some(d) => {
                    let mut f = Fmt::new(&self.syms);
                    InsnText { asm: f.asm(&d), mnemonic: f.mnemonic(&d) }
                }
                None => InsnText { asm: "(bad)".into(), mnemonic: "(bad)".into() },
            }
        })
    }

    /// Instruction text for bytes read from memory at `state(s)` (for addresses the trace never executed).
    fn text_from_memory(&self, addr: u64, s: u32) -> InsnText {
        let bytes: Vec<u8> = self.read_mem(s, addr, 15).into_iter().map_while(|b| b).collect();
        match asm::decode(&bytes, addr) {
            Some(d) => {
                let mut f = Fmt::new(&self.syms);
                InsnText { asm: f.asm(&d), mnemonic: f.mnemonic(&d) }
            }
            None => InsnText { asm: "(bad)".into(), mnemonic: "(bad)".into() },
        }
    }

    /// AT&T text of the instruction at `state(s).rip`.
    pub fn asm_at(&self, s: u32) -> String {
        if s < self.n {
            return self.insn_text(self.trace.steps[s as usize].insn).asm.clone();
        }
        let rip = self.regs_at(s).rip;
        match self.insn_by_addr.get(&rip) {
            Some(&i) => self.insn_text(i).asm.clone(),
            None => self.text_from_memory(rip, s).asm,
        }
    }

    /// The size-suffixed mnemonic of the instruction at `addr` (executed, else read from `state(s)`).
    pub fn mnemonic_at(&self, addr: u64, s: u32) -> String {
        match self.insn_by_addr.get(&addr) {
            Some(&i) => self.insn_text(i).mnemonic.clone(),
            None => self.text_from_memory(addr, s).mnemonic,
        }
    }

    /// `rip` of `state(s)`.
    pub fn rip_at(&self, s: u32) -> u64 {
        if s < self.n { self.trace.insns[self.trace.steps[s as usize].insn as usize].addr } else { self.regs_at(s).rip }
    }

    fn line_of_state(&self, s: u32) -> Option<(u32, u32)> {
        if s < self.n { self.insn_line[self.trace.steps[s as usize].insn as usize] } else { self.dwarf.line_at(self.regs_at(s).rip) }
    }

    fn source_ref(&self, line: Option<(u32, u32)>) -> Option<SourceRef> {
        line.map(|(f, l)| SourceRef { file: self.file_display[f as usize].clone(), line: l })
    }

    // ---- locators ----

    /// Resolves a locator to a step in `0..=N`.
    pub fn locate(&self, text: &str) -> Result<u32> {
        let nth = |steps: &[u32], text: &str, k: u64| {
            usize::try_from(k - 1)
                .ok()
                .and_then(|i| steps.get(i).copied())
                .ok_or_else(|| exit::usage(format!("{text} occurs {} times; @{k} asked", steps.len())))
        };
        match locator::parse(text)? {
            Locator::Step(n) => {
                if n > u64::from(self.n) {
                    return Err(exit::usage(format!("step:{n} is outside 0..={}", self.n)));
                }
                Ok(n as u32)
            }
            Locator::End => Ok(self.n),
            Locator::Line { file, line, text, k } => {
                let files = self.source_files(&file)?;
                let entries: Vec<u32> = (0..self.n).filter(|&s| self.enters_line(s, &files, line)).collect();
                nth(&entries, &text, k)
            }
            Locator::Addr { spec, text, k } => {
                let addr = match spec {
                    AddrSpec::Hex(a) => a,
                    AddrSpec::Symbol { name, offset } => {
                        let (a, _) = self.syms.lookup(&name).ok_or_else(|| exit::usage(format!("unknown symbol '{name}'")))?;
                        a.wrapping_add(offset)
                    }
                };
                nth(self.insn_by_addr.get(&addr).map_or(&[][..], |&idx| &self.occ[idx as usize]), &text, k)
            }
        }
    }

    /// A locator that `locate` resolves back to state `s`: `step:end` for the final state, else the
    /// instruction's `loc` and occurrence number (`fact+0xc@3`), or its raw address (`0x…@3`) when the
    /// symbolic form would not resolve to `s` (a stub or an ambiguous name).
    pub fn locator_of(&self, s: u32) -> String {
        if s >= self.n {
            return "step:end".to_string();
        }
        let idx = self.trace.steps[s as usize].insn as usize;
        let rip = self.trace.insns[idx].addr;
        let k = self.occ[idx].partition_point(|&x| x < s) + 1;
        let symbolic = format!("{}@{k}", self.loc(rip));
        if self.locate(&symbolic).is_ok_and(|t| t == s) { symbolic } else { format!("0x{rip:x}@{k}") }
    }

    /// The DWARF file indices a locator's FILE names: the entries of `file_display` equal to it, else
    /// those whose file name equals it, provided exactly one display path has that file name.
    fn source_files(&self, file: &str) -> Result<Vec<u32>> {
        let indices = |display: &str| -> Vec<u32> { (0..self.file_display.len() as u32).filter(|&i| self.file_display[i as usize] == display).collect() };
        let exact = indices(file);
        if !exact.is_empty() {
            return Ok(exact);
        }
        let mut named: Vec<&str> = self
            .file_display
            .iter()
            .map(String::as_str)
            .filter(|d| Path::new(d).file_name().is_some_and(|n| n == file))
            .collect();
        named.sort_unstable();
        named.dedup();
        match named[..] {
            [] => Err(exit::usage(format!("unknown source file '{file}'"))),
            [only] => Ok(indices(only)),
            _ => Err(exit::usage(format!("source file '{file}' is ambiguous: {}", named.join(", ")))),
        }
    }

    /// Whether state `s` is on `line` of one of the DWARF `files`.
    fn on_line(&self, s: u32, files: &[u32], line: u32) -> bool {
        self.line_of_state(s).is_some_and(|(f, l)| l == line && files.contains(&f))
    }

    /// Whether state `s` enters `line` of `files`: it is on the line and the previous state was not.
    fn enters_line(&self, s: u32, files: &[u32], line: u32) -> bool {
        self.on_line(s, files, line) && (s == 0 || !self.on_line(s - 1, files, line))
    }

    /// Runtime address range `[start, end)` of the executable symbol `name` (or `libfile:name`).
    pub fn symbol_range(&self, name: &str) -> Option<(u64, u64)> {
        self.syms.lookup(name).map(|(a, s)| (a, a + s))
    }

    // ---- step views ----

    fn write_view(&self, w: &MemWrite, s: u32) -> WriteView {
        let mut v = [0u8; 8];
        let n = w.data.len().min(8);
        v[..n].copy_from_slice(&w.data[..n]);
        WriteView {
            addr: self.render_value(w.addr, s),
            raw_addr: format!("0x{:x}", w.addr),
            size: w.data.len(),
            value: self.render_value(u64::from_le_bytes(v), s),
            kind: write_kind_name(w.kind),
        }
    }

    pub fn event_view(&self, e: &Event) -> EventView {
        match e.kind {
            EventKind::Syscall { nr, args, ret } => EventView::Syscall {
                nr,
                name: crate::syscalls::name(nr),
                args: args.iter().map(|&a| self.render_value(a, e.step)).collect(),
                ret: ret.map(|r| self.render_value(r as u64, e.step + 1)),
            },
            EventKind::Signal { signo, code, addr } => {
                EventView::Signal { signal: crate::signals::name(signo), code, addr: self.loc(addr) }
            }
        }
    }

    /// The `steps` command's view of step `i` (`i < N`).
    pub fn step_view(&self, i: u32) -> StepView {
        let s = i + 1;
        let mut regs = Vec::new();
        let mut rflags = None;
        for d in self.deltas_of_step(i) {
            if d.reg < 16 {
                regs.push((reg::name(d.reg), self.render_value(d.value as u64, s)));
            } else if d.reg == reg::RFLAGS {
                rflags = Some(flag_names(d.value as u64));
            }
        }
        StepView {
            step: i,
            loc: self.loc(self.rip_at(i)),
            asm: self.asm_at(i),
            regs: OrderedMap(regs),
            rflags,
            writes: self.writes_of_step(i).iter().map(|w| self.write_view(w, s)).collect(),
            event: self.event_of(i).map(|e| self.event_view(e)),
        }
    }

    pub fn step_kind(&self, i: u32) -> &'static str {
        if i >= self.n {
            return "end";
        }
        match self.trace.steps[i as usize].kind {
            StepKind::Instr => "instr",
            StepKind::Fault { .. } => "fault",
            StepKind::Deliver { .. } => "deliver",
            StepKind::Exit => "exit",
        }
    }

    // ---- info ----

    /// The `info` command's view.
    pub fn info_view(&self) -> InfoView {
        let m = &self.trace.meta;
        let mut syscalls: Vec<NameCount> = Vec::new();
        let mut signals: Vec<NameCount> = Vec::new();
        let bump = |list: &mut Vec<NameCount>, name: String| match list.iter_mut().find(|x| x.name == name) {
            Some(x) => x.count += 1,
            None => list.push(NameCount { name, count: 1 }),
        };
        for e in &self.trace.events {
            match e.kind {
                EventKind::Syscall { nr, .. } => bump(&mut syscalls, crate::syscalls::name(nr)),
                EventKind::Signal { signo, .. } => bump(&mut signals, crate::signals::name(signo)),
            }
        }
        InfoView {
            program: m.program.clone(),
            argv: m.argv.clone(),
            from: m.from.clone(),
            steps: self.n,
            end: EndView::of(&m.end),
            aslr: if m.aslr_disabled { "disabled" } else { "enabled" },
            unattributed_bytes: m.unattributed_bytes,
            syscalls,
            signals,
        }
    }
}
