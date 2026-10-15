//! /proc/<pid>/maps parsing and address-range helpers.

use anyhow::{Context, Result};

use crate::trace::Region;

pub fn parse_maps(pid: libc::pid_t) -> Result<Vec<Region>> {
    Ok(parse_maps_ident(pid)?.0)
}

/// Regions plus, per region, its `dev:inode` string (the backing file's identity).
pub fn parse_maps_ident(pid: libc::pid_t) -> Result<(Vec<Region>, Vec<String>)> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/maps")).context("reading /proc/<pid>/maps")?;
    let mut out = Vec::new();
    let mut ids = Vec::new();
    for line in text.lines() {
        let mut it = line.splitn(6, ' ');
        let (Some(range), Some(perms), Some(offset)) = (it.next(), it.next(), it.next()) else { continue };
        let dev = it.next().unwrap_or("");
        let inode = it.next().unwrap_or("");
        let path = it.next().unwrap_or("").trim_start().to_string();
        let Some((s, e)) = range.split_once('-') else { continue };
        let (Ok(start), Ok(end), Ok(offset)) =
            (u64::from_str_radix(s, 16), u64::from_str_radix(e, 16), u64::from_str_radix(offset, 16))
        else {
            continue;
        };
        out.push(Region { start, end, perms: perms.to_string(), offset, path });
        ids.push(format!("{dev}:{inode}"));
    }
    Ok((out, ids))
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

/// Readable ranges of `new` whose contents may differ from what `old` already
/// held: parts not covered by an old region with the same identity (perms, path,
/// backing file, and for files the same file offset at the same address).
pub fn changed_readable(new: &[Region], new_ids: &[String], old: &[Region], old_ids: &[String]) -> Vec<(u64, u64)> {
    let key = |r: &Region, id: &str| {
        let delta = if r.path.starts_with('/') { r.offset.wrapping_sub(r.start) } else { 0 };
        (r.perms.clone(), r.path.clone(), id.to_string(), delta)
    };
    let mut out: Vec<(u64, u64)> = Vec::new();
    for (r, id) in new.iter().zip(new_ids).filter(|(r, _)| is_readable(r)) {
        let k = key(r, id);
        let mut same: Vec<(u64, u64)> = old
            .iter()
            .zip(old_ids)
            .filter(|(o, oid)| is_readable(o) && key(o, oid) == k)
            .map(|(o, _)| (o.start, o.end))
            .collect();
        same.sort_unstable();
        out.extend(subtract(&[(r.start, r.end)], &same));
    }
    out.sort_unstable();
    out
}

/// Whether `addr` lies in any region.
pub fn is_mapped(regions: &[Region], addr: u64) -> bool {
    let i = regions.partition_point(|r| r.start <= addr);
    i > 0 && addr < regions[i - 1].end
}
