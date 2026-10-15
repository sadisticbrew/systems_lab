//! DWARF facts from the embedded executable: subprograms with their frame
//! base, parameters and variables with a single `DW_OP_fbreg` location, and
//! the line table at runtime addresses.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use gimli::{AttributeValue, EndianSlice, LittleEndian};
use object::{Object, ObjectSection};

type R<'a> = EndianSlice<'a, LittleEndian>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameBase {
    CallFrameCfa,
    /// `DW_OP_reg6` / `DW_OP_breg6 off`: the activation's `%rbp` plus `off`.
    Rbp(i64),
    /// `DW_OP_reg7` / `DW_OP_breg7 off`: the activation's `%rsp` plus `off`.
    Rsp(i64),
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enc {
    Signed,
    Unsigned,
    Bool,
    Float,
    Pointer,
    Other,
}

#[derive(Debug, Clone)]
pub struct DwVar {
    pub name: String,
    pub param: bool,
    pub fbreg: i64,
    pub size: u64,
    pub enc: Enc,
    /// Scope as runtime pc ranges; empty means the whole function.
    pub scope: Vec<(u64, u64)>,
}

#[derive(Debug, Clone)]
pub struct DwFunc {
    pub ranges: Vec<(u64, u64)>,
    pub frame_base: FrameBase,
    pub vars: Vec<DwVar>,
}

#[derive(Debug, Clone, Copy)]
struct LineRow {
    addr: u64,
    /// 0 marks the end of a sequence.
    line: u32,
    file: u32,
}

#[derive(Default)]
pub struct DwarfInfo {
    pub funcs: Vec<DwFunc>,
    rows: Vec<LineRow>,
    /// Normalized absolute source paths, indexed by `LineRow::file`.
    pub files: Vec<String>,
}

fn sleb(bytes: &[u8]) -> Option<(i64, usize)> {
    let mut result = 0i64;
    let mut shift = 0;
    for (i, &b) in bytes.iter().enumerate() {
        result |= i64::from(b & 0x7f) << shift;
        shift += 7;
        if b & 0x80 == 0 {
            if shift < 64 && b & 0x40 != 0 {
                result |= -1i64 << shift;
            }
            return Some((result, i + 1));
        }
        if shift >= 64 {
            return None;
        }
    }
    None
}

fn frame_base_of(expr: &[u8]) -> FrameBase {
    match expr {
        [0x9c] => FrameBase::CallFrameCfa,
        [0x56] => FrameBase::Rbp(0),
        [0x57] => FrameBase::Rsp(0),
        [op @ (0x76 | 0x77), rest @ ..] => match sleb(rest) {
            Some((off, n)) if n == rest.len() => {
                if *op == 0x76 { FrameBase::Rbp(off) } else { FrameBase::Rsp(off) }
            }
            _ => FrameBase::Unsupported,
        },
        _ => FrameBase::Unsupported,
    }
}

/// Lexically normalizes a path (removes `.` and `..`).
pub fn normalize(p: &Path) -> String {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out.to_string_lossy().into_owned()
}

fn exprloc<'a>(v: AttributeValue<R<'a>>) -> Option<&'a [u8]> {
    match v {
        AttributeValue::Exprloc(e) => Some(e.0.slice()),
        _ => None,
    }
}

fn udata(e: &gimli::DebuggingInformationEntry<R<'_>>, at: gimli::DwAt) -> Option<u64> {
    e.attr_value(at).and_then(|v| v.udata_value())
}

fn type_of(unit: &gimli::Unit<R<'_>>, off: gimli::UnitOffset<usize>, depth: u32) -> (u64, Enc) {
    if depth > 16 {
        return (0, Enc::Other);
    }
    let Ok(e) = unit.entry(off) else { return (0, Enc::Other) };
    let inner = |e: &gimli::DebuggingInformationEntry<R<'_>>| match e.attr_value(gimli::DW_AT_type) {
        Some(AttributeValue::UnitRef(o)) => type_of(unit, o, depth + 1),
        _ => (0, Enc::Other),
    };
    match e.tag() {
        gimli::DW_TAG_typedef | gimli::DW_TAG_const_type | gimli::DW_TAG_volatile_type | gimli::DW_TAG_restrict_type | gimli::DW_TAG_atomic_type => {
            inner(&e)
        }
        gimli::DW_TAG_base_type => {
            let size = udata(&e, gimli::DW_AT_byte_size).unwrap_or(0);
            let enc = match e.attr_value(gimli::DW_AT_encoding) {
                Some(AttributeValue::Encoding(x)) => match x {
                    gimli::DW_ATE_boolean => Enc::Bool,
                    gimli::DW_ATE_float => Enc::Float,
                    gimli::DW_ATE_signed | gimli::DW_ATE_signed_char => Enc::Signed,
                    gimli::DW_ATE_unsigned | gimli::DW_ATE_unsigned_char | gimli::DW_ATE_UTF => Enc::Unsigned,
                    _ => Enc::Other,
                },
                _ => Enc::Other,
            };
            (size, enc)
        }
        gimli::DW_TAG_pointer_type | gimli::DW_TAG_reference_type | gimli::DW_TAG_rvalue_reference_type => {
            (udata(&e, gimli::DW_AT_byte_size).unwrap_or(8), Enc::Pointer)
        }
        gimli::DW_TAG_array_type => {
            let (elem, _) = inner(&e);
            let mut count = 1u64;
            if let Ok(mut tree) = unit.entries_tree(Some(off))
                && let Ok(root) = tree.root()
            {
                let mut kids = root.children();
                while let Ok(Some(k)) = kids.next() {
                    let ke = k.entry();
                    if ke.tag() == gimli::DW_TAG_subrange_type {
                        let n = udata(ke, gimli::DW_AT_count)
                            .or_else(|| udata(ke, gimli::DW_AT_upper_bound).map(|u| u + 1))
                            .unwrap_or(0);
                        count = count.saturating_mul(n);
                    }
                }
            }
            (elem.saturating_mul(count), Enc::Other)
        }
        _ => (udata(&e, gimli::DW_AT_byte_size).unwrap_or(0), Enc::Other),
    }
}

impl DwarfInfo {
    /// Reads the DWARF of `bytes`; an executable without DWARF gives an empty result.
    pub fn build(bytes: &[u8], bias: u64) -> DwarfInfo {
        let Ok(file) = object::File::parse(bytes) else { return DwarfInfo::default() };
        let load = |id: gimli::SectionId| -> Result<R<'_>, gimli::Error> {
            let data = file.section_by_name(id.name()).and_then(|s| s.data().ok()).unwrap_or(&[]);
            Ok(EndianSlice::new(data, LittleEndian))
        };
        let Ok(dwarf) = gimli::Dwarf::load(load) else { return DwarfInfo::default() };
        let mut info = DwarfInfo::default();
        let mut file_ids: HashMap<String, u32> = HashMap::new();
        let mut units = dwarf.units();
        while let Ok(Some(header)) = units.next() {
            let Ok(unit) = dwarf.unit(header) else { continue };
            let _ = info.read_funcs(&dwarf, &unit, bias);
            let _ = info.read_lines(&dwarf, &unit, bias, &mut file_ids);
        }
        info.rows.sort_by_key(|r| (r.addr, r.line != 0));
        info
    }

    fn read_funcs<'a>(&mut self, dwarf: &gimli::Dwarf<R<'a>>, unit: &gimli::Unit<R<'a>>, bias: u64) -> gimli::Result<()> {
        struct Scope {
            depth: isize,
            func: Option<usize>,
            ranges: Vec<(u64, u64)>,
        }
        let ranges_of = |e: &gimli::DebuggingInformationEntry<R<'a>>| -> Vec<(u64, u64)> {
            let mut out = Vec::new();
            if let Ok(mut it) = dwarf.die_ranges(unit, e) {
                while let Ok(Some(r)) = it.next() {
                    if r.begin < r.end {
                        out.push((r.begin.wrapping_add(bias), r.end.wrapping_add(bias)));
                    }
                }
            }
            out
        };
        let mut cur = unit.entries();
        let mut scopes: Vec<Scope> = Vec::new();
        while let Some(e) = cur.next_dfs()? {
            let depth = e.depth();
            while scopes.last().is_some_and(|s| s.depth >= depth) {
                scopes.pop();
            }
            match e.tag() {
                gimli::DW_TAG_subprogram => {
                    let ranges = ranges_of(e);
                    if ranges.is_empty() {
                        scopes.push(Scope { depth, func: None, ranges });
                        continue;
                    }
                    let frame_base = e
                        .attr_value(gimli::DW_AT_frame_base)
                        .and_then(exprloc)
                        .map_or(FrameBase::Unsupported, frame_base_of);
                    self.funcs.push(DwFunc { ranges, frame_base, vars: Vec::new() });
                    scopes.push(Scope { depth, func: Some(self.funcs.len() - 1), ranges: Vec::new() });
                }
                gimli::DW_TAG_lexical_block => {
                    let func = scopes.last().and_then(|s| s.func);
                    scopes.push(Scope { depth, func, ranges: ranges_of(e) });
                }
                gimli::DW_TAG_formal_parameter | gimli::DW_TAG_variable => {
                    let Some(top) = scopes.last() else { continue };
                    let Some(fi) = top.func else { continue };
                    let Some(loc) = e.attr_value(gimli::DW_AT_location).and_then(exprloc) else { continue };
                    let Some((&0x91, rest)) = loc.split_first() else { continue };
                    let Some((fbreg, n)) = sleb(rest) else { continue };
                    if n != rest.len() {
                        continue;
                    }
                    let Some(name_attr) = e.attr_value(gimli::DW_AT_name) else { continue };
                    let name = dwarf.attr_string(unit, name_attr).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    let (size, enc) = match e.attr_value(gimli::DW_AT_type) {
                        Some(AttributeValue::UnitRef(o)) => type_of(unit, o, 0),
                        _ => (0, Enc::Other),
                    };
                    let scope = scopes.iter().rev().find(|s| !s.ranges.is_empty()).map(|s| s.ranges.clone()).unwrap_or_default();
                    self.funcs[fi].vars.push(DwVar {
                        name,
                        param: e.tag() == gimli::DW_TAG_formal_parameter,
                        fbreg,
                        size,
                        enc,
                        scope,
                    });
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn read_lines<'a>(
        &mut self,
        dwarf: &gimli::Dwarf<R<'a>>,
        unit: &gimli::Unit<R<'a>>,
        bias: u64,
        file_ids: &mut HashMap<String, u32>,
    ) -> gimli::Result<()> {
        let Some(program) = unit.line_program.clone() else { return Ok(()) };
        let comp_dir = unit.comp_dir.map(|d| PathBuf::from(d.to_string_lossy().into_owned())).unwrap_or_default();
        let mut local: HashMap<u64, Option<u32>> = HashMap::new();
        let mut rows = program.rows();
        while let Some((header, row)) = rows.next_row()? {
            let addr = row.address().wrapping_add(bias);
            if row.end_sequence() {
                self.rows.push(LineRow { addr, line: 0, file: 0 });
                continue;
            }
            if !row.is_stmt() {
                continue;
            }
            let Some(line) = row.line().map(|l| l.get() as u32) else { continue };
            let fid = *local.entry(row.file_index()).or_insert_with(|| {
                let entry = row.file(header)?;
                let name = dwarf.attr_string(unit, entry.path_name()).ok()?;
                let mut path = comp_dir.clone();
                if let Some(dir) = entry.directory(header)
                    && let Ok(dir) = dwarf.attr_string(unit, dir)
                {
                    path.push(dir.to_string_lossy().as_ref());
                }
                path.push(name.to_string_lossy().as_ref());
                let path = normalize(&path);
                let next = file_ids.len() as u32;
                let id = *file_ids.entry(path.clone()).or_insert(next);
                if id == next {
                    self.files.push(path);
                }
                Some(id)
            });
            if let Some(file) = fid {
                self.rows.push(LineRow { addr, line, file });
            }
        }
        Ok(())
    }

    pub fn has_dwarf(&self) -> bool {
        !self.funcs.is_empty() || !self.rows.is_empty()
    }

    /// The `(file index, line)` containing `addr`.
    pub fn line_at(&self, addr: u64) -> Option<(u32, u32)> {
        let i = self.rows.partition_point(|r| r.addr <= addr);
        let r = self.rows.get(i.checked_sub(1)?)?;
        (r.line != 0).then_some((r.file, r.line))
    }

    pub fn func_at(&self, pc: u64) -> Option<&DwFunc> {
        self.funcs.iter().find(|f| f.ranges.iter().any(|&(s, e)| pc >= s && pc < e))
    }
}

impl DwFunc {
    /// Parameters and variables in scope at `pc`, in declaration order.
    pub fn vars_at(&self, pc: u64) -> impl Iterator<Item = &DwVar> {
        self.vars.iter().filter(move |v| v.scope.is_empty() || v.scope.iter().any(|&(s, e)| pc >= s && pc < e))
    }
}
