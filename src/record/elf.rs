//! ELF facts the recorder needs: load bias, symbols, embedded sources.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use gimli::{EndianSlice, LittleEndian};
use object::{BinaryFormat, Object, ObjectKind, ObjectSection, ObjectSegment, ObjectSymbol, SectionKind, SymbolKind};

use crate::trace::{LibSyms, Region, Source, Sym};

const MAX_SOURCE_BYTES: u64 = 1 << 20;

fn page_down(v: u64) -> u64 {
    v & !0xfff
}

pub struct ExeInfo {
    /// Lowest PT_LOAD vaddr rounded down to a page.
    pub load_lo: u64,
    /// ET_EXEC: loaded at its link address.
    pub fixed: bool,
    pub syms: Vec<Sym>,
    /// Named symbols defined in code sections, whatever their type and size. A hand-written `main:`
    /// label is NOTYPE with size 0, so it is missing from `syms` but is a valid start point.
    pub starts: Vec<Sym>,
}

fn parse(bytes: &[u8]) -> Result<object::File<'_>> {
    let file = object::File::parse(bytes).context("parsing ELF")?;
    anyhow::ensure!(file.format() == BinaryFormat::Elf, "not an ELF file");
    Ok(file)
}

/// FUNC/OBJECT symbols with a size, from `.symtab` if present, else `.dynsym`.
fn symbols(file: &object::File<'_>) -> Vec<Sym> {
    let collect = |iter: &mut dyn Iterator<Item = object::Symbol<'_, '_>>| -> Vec<Sym> {
        let mut v: Vec<Sym> = iter
            .filter(|s| matches!(s.kind(), SymbolKind::Text | SymbolKind::Data) && !s.is_undefined() && s.size() > 0)
            .filter_map(|s| {
                let name = s.name().ok()?;
                (!name.is_empty()).then(|| Sym { name: name.to_string(), addr: s.address(), size: s.size() })
            })
            .collect();
        v.sort_by(|a, b| (a.addr, &a.name).cmp(&(b.addr, &b.name)));
        v
    };
    if file.section_by_name(".symtab").is_some() {
        collect(&mut file.symbols())
    } else {
        collect(&mut file.dynamic_symbols())
    }
}

/// Defined symbols of code sections of any type and size, from `.symtab` if present, else `.dynsym`.
fn code_symbols(file: &object::File<'_>) -> Vec<Sym> {
    let collect = |iter: &mut dyn Iterator<Item = object::Symbol<'_, '_>>| -> Vec<Sym> {
        iter.filter(|s| {
            let in_code = s.section_index().and_then(|i| file.section_by_index(i).ok());
            matches!(s.kind(), SymbolKind::Text | SymbolKind::Unknown)
                && in_code.is_some_and(|sec| sec.kind() == SectionKind::Text)
        })
        .filter_map(|s| {
            let name = s.name().ok()?;
            (!name.is_empty()).then(|| Sym { name: name.to_string(), addr: s.address(), size: s.size() })
        })
        .collect()
    };
    if file.section_by_name(".symtab").is_some() {
        collect(&mut file.symbols())
    } else {
        collect(&mut file.dynamic_symbols())
    }
}

pub fn analyze(bytes: &[u8]) -> Result<ExeInfo> {
    let file = parse(bytes)?;
    let load_lo = file.segments().map(|s| s.address()).min().map_or(0, page_down);
    let fixed = file.kind() == ObjectKind::Executable;
    Ok(ExeInfo { load_lo, fixed, syms: symbols(&file), starts: code_symbols(&file) })
}

/// Load bias of the mapped file at `path`: first offset-0 mapping minus the lowest PT_LOAD page.
pub fn bias_of(regions: &[Region], path: &str, info: &ExeInfo) -> u64 {
    if info.fixed {
        return 0;
    }
    let start = regions.iter().filter(|r| r.path == path && r.offset == 0).map(|r| r.start).min();
    start.map_or(0, |s| s.wrapping_sub(info.load_lo))
}

/// Symbols of a mapped shared object, or `None` when `path` is not a readable ELF file.
pub fn lib_syms(path: &str, regions: &[Region]) -> Option<LibSyms> {
    if !path.starts_with('/') || path.ends_with(" (deleted)") {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.get(..4) != Some(b"\x7fELF") {
        return None;
    }
    let info = analyze(&bytes).ok()?;
    let bias = bias_of(regions, path, &info);
    let file_name = Path::new(path).file_name()?.to_string_lossy().into_owned();
    Some(LibSyms { path: path.to_string(), file_name, bias, syms: info.syms })
}

/// Source files named by the DWARF line programs of `bytes` that exist on disk.
pub fn sources(bytes: &[u8]) -> Vec<Source> {
    let Ok(file) = parse(bytes) else { return Vec::new() };
    let load = |id: gimli::SectionId| -> Result<EndianSlice<'_, LittleEndian>, gimli::Error> {
        let data = file.section_by_name(id.name()).and_then(|s| s.data().ok()).unwrap_or(&[]);
        Ok(EndianSlice::new(data, LittleEndian))
    };
    let Ok(dwarf) = gimli::Dwarf::load(load) else { return Vec::new() };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut units = dwarf.units();
    while let Ok(Some(header)) = units.next() {
        let Ok(unit) = dwarf.unit(header) else { continue };
        let comp_dir = unit.comp_dir.map(|d| PathBuf::from(d.to_string_lossy().into_owned()));
        let Some(program) = unit.line_program.clone() else { continue };
        let lh = program.header();
        for entry in lh.file_names() {
            let Ok(name) = dwarf.attr_string(&unit, entry.path_name()) else { continue };
            let mut path = comp_dir.clone().unwrap_or_default();
            if let Some(dir) = entry.directory(lh) {
                if let Ok(dir) = dwarf.attr_string(&unit, dir) {
                    path.push(dir.to_string_lossy().as_ref());
                }
            }
            path.push(name.to_string_lossy().as_ref());
            let Ok(canon) = std::fs::canonicalize(&path) else { continue };
            if canon.starts_with("/usr/") || !seen.insert(canon.clone()) {
                continue;
            }
            let Ok(meta) = std::fs::metadata(&canon) else { continue };
            if !meta.is_file() || meta.len() >= MAX_SOURCE_BYTES {
                continue;
            }
            let Ok(raw) = std::fs::read(&canon) else { continue };
            let display = comp_dir
                .as_ref()
                .and_then(|d| std::fs::canonicalize(d).ok())
                .and_then(|d| canon.strip_prefix(d).ok().map(|p| p.to_string_lossy().into_owned()))
                .unwrap_or_else(|| canon.to_string_lossy().into_owned());
            out.push(Source {
                path: canon.to_string_lossy().into_owned(),
                display,
                text: String::from_utf8_lossy(&raw).into_owned(),
            });
        }
    }
    out
}
