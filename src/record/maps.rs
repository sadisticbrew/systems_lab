//! /proc/<pid>/maps parsing and address-range helpers.

use anyhow::{Context, Result};

use crate::trace::Region;

pub fn parse_maps(pid: libc::pid_t) -> Result<Vec<Region>> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/maps")).context("reading /proc/<pid>/maps")?;
    let mut out = Vec::new();
    for line in text.lines() {
        let mut it = line.splitn(6, ' ');
        let (Some(range), Some(perms), Some(offset)) = (it.next(), it.next(), it.next()) else { continue };
        let _dev = it.next();
        let _inode = it.next();
        let path = it.next().unwrap_or("").trim_start().to_string();
        let Some((s, e)) = range.split_once('-') else { continue };
        let (Ok(start), Ok(end), Ok(offset)) =
            (u64::from_str_radix(s, 16), u64::from_str_radix(e, 16), u64::from_str_radix(offset, 16))
        else {
            continue;
        };
        out.push(Region { start, end, perms: perms.to_string(), offset, path });
    }
    Ok(out)
}

/// Regions the kernel exposes but that must not be snapshotted.
pub fn is_skipped(r: &Region) -> bool {
    matches!(r.path.as_str(), "[vvar]" | "[vvar_vclock]" | "[vsyscall]")
}

pub fn is_readable(r: &Region) -> bool {
    r.perms.as_bytes().first() == Some(&b'r') && !is_skipped(r)
}

pub fn is_writable(r: &Region) -> bool {
    r.perms.as_bytes().get(1) == Some(&b'w') && is_readable(r)
}

/// Sorted, merged `[start, end)` ranges of the regions selected by `pred`.
pub fn ranges(regions: &[Region], pred: impl Fn(&Region) -> bool) -> Vec<(u64, u64)> {
    let mut out: Vec<(u64, u64)> = Vec::new();
    for r in regions.iter().filter(|r| pred(r)) {
        match out.last_mut() {
            Some(last) if last.1 >= r.start => last.1 = last.1.max(r.end),
            _ => out.push((r.start, r.end)),
        }
    }
    out
}

/// The parts of `new` not covered by `old`; both sorted and merged.
pub fn subtract(new: &[(u64, u64)], old: &[(u64, u64)]) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    for &(mut s, e) in new {
        for &(os, oe) in old {
            if oe <= s || os >= e {
                continue;
            }
            if os > s {
                out.push((s, os));
            }
            s = s.max(oe);
            if s >= e {
                break;
            }
        }
        if s < e {
            out.push((s, e));
        }
    }
    out
}

/// Whether `addr` lies in any region.
pub fn is_mapped(regions: &[Region], addr: u64) -> bool {
    let i = regions.partition_point(|r| r.start <= addr);
    i > 0 && addr < regions[i - 1].end
}
