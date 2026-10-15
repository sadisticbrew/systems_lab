//! Shadow copy of the tracee's writable memory, used to find writes that no
//! instruction accounts for and to decide whether conditional writes changed bytes.

use std::collections::HashMap;

use crate::trace::{PAGE_SIZE, Region};

use super::maps::{self, is_writable};
use super::spawn::Tracee;

const PAGE: usize = PAGE_SIZE as usize;
const CHUNK_PAGES: u64 = 256;

/// A coalesced difference found by a full diff, within one page.
pub struct DiffRun {
    pub addr: u64,
    pub data: Vec<u8>,
    /// Number of bytes in the run that actually differ.
    pub changed: u64,
}

#[derive(Default)]
pub struct Shadow {
    pages: HashMap<u64, Box<[u8; PAGE]>>,
    buf: Vec<u8>,
}

/// Reads `npages` pages starting at page-aligned `start`, calling `f(page_addr, bytes)`
/// for each readable page. Unreadable pages are skipped.
pub fn read_pages(t: &Tracee, start: u64, npages: u64, buf: &mut Vec<u8>, mut f: impl FnMut(u64, &[u8])) {
    let mut p = 0;
    while p < npages {
        let n = (npages - p).min(CHUNK_PAGES);
        let base = start + p * PAGE_SIZE;
        buf.resize(n as usize * PAGE, 0);
        let got = t.read(base, &mut buf[..]);
        let full = got / PAGE;
        for k in 0..full {
            f(base + (k * PAGE) as u64, &buf[k * PAGE..(k + 1) * PAGE]);
        }
        for k in full..n as usize {
            let addr = base + (k * PAGE) as u64;
            if t.read(addr, &mut buf[..PAGE]) == PAGE {
                f(addr, &buf[..PAGE]);
            }
        }
        p += n;
    }
}

impl Shadow {
    /// Makes the shadow cover exactly the writable pages of `regions`: drops
    /// pages that are no longer writable and loads new ones from the tracee.
    pub fn sync(&mut self, t: &Tracee, regions: &[Region]) {
        let writable = maps::ranges(regions, is_writable);
        self.pages.retain(|&page, _| {
            let i = writable.partition_point(|r| r.0 <= page);
            i > 0 && page < writable[i - 1].1
        });
        for &(s, e) in &writable {
            let mut page = s;
            while page < e {
                if self.pages.contains_key(&page) {
                    page += PAGE_SIZE;
                    continue;
                }
                let run_start = page;
                while page < e && !self.pages.contains_key(&page) {
                    page += PAGE_SIZE;
                }
                let pages = &mut self.pages;
                read_pages(t, run_start, (page - run_start) / PAGE_SIZE, &mut self.buf, |addr, data| {
                    let mut b = Box::new([0u8; PAGE]);
                    b.copy_from_slice(data);
                    pages.insert(addr, b);
                });
            }
        }
    }

    /// Copies `data` (just written at `addr`) into the shadow where pages are tracked.
    pub fn update(&mut self, addr: u64, data: &[u8]) {
        let mut addr = addr;
        let mut rest = data;
        while !rest.is_empty() {
            let page = addr & !(PAGE_SIZE - 1);
            let off = (addr - page) as usize;
            let n = rest.len().min(PAGE - off);
            if let Some(p) = self.pages.get_mut(&page) {
                p[off..off + n].copy_from_slice(&rest[..n]);
            }
            addr += n as u64;
            rest = &rest[n..];
        }
    }

    /// Whether the shadow already holds exactly `data` at `addr`.
    pub fn matches(&self, addr: u64, data: &[u8]) -> bool {
        let mut addr = addr;
        let mut rest = data;
        while !rest.is_empty() {
            let page = addr & !(PAGE_SIZE - 1);
            let off = (addr - page) as usize;
            let n = rest.len().min(PAGE - off);
            match self.pages.get(&page) {
                Some(p) if p[off..off + n] == rest[..n] => {}
                _ => return false,
            }
            addr += n as u64;
            rest = &rest[n..];
        }
        true
    }

    /// Compares every tracked page with the tracee, updating the shadow and
    /// appending the differences to `out`.
    pub fn diff(&mut self, t: &Tracee, regions: &[Region], out: &mut Vec<DiffRun>) {
        let writable = maps::ranges(regions, is_writable);
        let pages = &mut self.pages;
        for &(s, e) in &writable {
            read_pages(t, s, (e - s) / PAGE_SIZE, &mut self.buf, |addr, data| {
                match pages.get_mut(&addr) {
                    Some(old) => {
                        if old[..] != *data {
                            page_runs(addr, &old[..], data, out);
                            old.copy_from_slice(data);
                        }
                    }
                    None => {
                        let mut b = Box::new([0u8; PAGE]);
                        b.copy_from_slice(data);
                        pages.insert(addr, b);
                    }
                }
            });
        }
    }
}

/// Differing byte runs of one page; runs closer than 8 bytes are merged.
fn page_runs(page: u64, old: &[u8], new: &[u8], out: &mut Vec<DiffRun>) {
    let mut i = 0;
    while i < PAGE {
        if old[i] == new[i] {
            i += 1;
            continue;
        }
        let start = i;
        let mut last = i;
        let mut changed = 1u64;
        i += 1;
        while i < PAGE {
            if old[i] != new[i] {
                last = i;
                changed += 1;
            } else if i - last > 8 {
                break;
            }
            i += 1;
        }
        out.push(DiffRun { addr: page + start as u64, data: new[start..=last].to_vec(), changed });
    }
}
