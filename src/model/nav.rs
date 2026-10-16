//! Step navigation (`/api/nav`).

use anyhow::{Result, bail};

use super::LoadedTrace;
use crate::exit;
use crate::trace::reg;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavOp {
    Over,
    BackOver,
    Out,
    BackOut,
    NextLine,
    PrevLine,
    RegPrev(u8),
    RegNext(u8),
    WritePrev(u64),
    /// The next step that writes the byte at this address.
    WriteNext(u64),
    AtNext(u64),
    /// The next step that enters this source line (`file` as in a `FILE:LINE` locator), at any call depth.
    LineNext { file: String, line: u32 },
}

/// The request parameters that select a navigation target, as the API receives them.
#[derive(Debug, Clone, Copy, Default)]
pub struct NavParams<'a> {
    /// A register name (`rax`) for `reg-prev` / `reg-next`.
    pub reg: Option<&'a str>,
    /// A hex address, optional `0x`, for `write-prev`, `write-next` and `at-next`.
    pub addr: Option<&'a str>,
    /// A source file as in a `FILE:LINE` locator, for `line-next`.
    pub file: Option<&'a str>,
    /// A line number, for `line-next`.
    pub line: Option<&'a str>,
}

impl NavOp {
    /// Builds an op from the API's `op` and its parameters.
    pub fn parse(op: &str, p: &NavParams) -> Result<NavOp> {
        let reg_id = || -> Result<u8> {
            let name = p.reg.ok_or_else(|| exit::usage(format!("op {op} needs reg=")))?;
            reg::by_name(name).ok_or_else(|| exit::usage(format!("unknown register '{name}'")))
        };
        let addr_val = || -> Result<u64> {
            let a = p.addr.ok_or_else(|| exit::usage(format!("op {op} needs addr=")))?;
            u64::from_str_radix(a.strip_prefix("0x").unwrap_or(a), 16).map_err(|_| exit::usage(format!("bad address '{a}'")))
        };
        Ok(match op {
            "over" => NavOp::Over,
            "back-over" => NavOp::BackOver,
            "out" => NavOp::Out,
            "back-out" => NavOp::BackOut,
            "next-line" => NavOp::NextLine,
            "prev-line" => NavOp::PrevLine,
            "reg-prev" => NavOp::RegPrev(reg_id()?),
            "reg-next" => NavOp::RegNext(reg_id()?),
            "write-prev" => NavOp::WritePrev(addr_val()?),
            "write-next" => NavOp::WriteNext(addr_val()?),
            "at-next" => NavOp::AtNext(addr_val()?),
            "line-next" => {
                let file = p.file.ok_or_else(|| exit::usage(format!("op {op} needs file=")))?;
                let line = p.line.ok_or_else(|| exit::usage(format!("op {op} needs line=")))?;
                let line = line.parse().map_err(|_| exit::usage(format!("bad line '{line}'")))?;
                NavOp::LineNext { file: file.to_string(), line }
            }
            _ => bail!(exit::usage(format!("unknown nav op '{op}'"))),
        })
    }
}

impl LoadedTrace {
    /// The step reached by `op` from step `from`, or `None`. `from` must be at most `N`.
    pub fn nav(&self, from: u32, op: &NavOp) -> Result<Option<u32>> {
        if from > self.n {
            bail!(exit::usage(format!("step {from} is outside 0..={}", self.n)));
        }
        let d = &self.acts.depth;
        let dfrom = d[from as usize];
        let n = self.n;
        Ok(match op {
            NavOp::Over => (from + 1..=n).find(|&t| d[t as usize] <= dfrom),
            NavOp::BackOver => (0..from).rev().find(|&t| d[t as usize] <= dfrom),
            NavOp::Out => (from + 1..=n).find(|&t| d[t as usize] < dfrom),
            NavOp::BackOut => (0..from).rev().find(|&t| d[t as usize] < dfrom),
            NavOp::NextLine => {
                let line = self.line_of_state(from);
                (from + 1..=n).find(|&t| d[t as usize] <= dfrom && self.line_differs(t, line))
            }
            NavOp::PrevLine => {
                let line = self.line_of_state(from);
                (0..from).rev().find(|&t| d[t as usize] <= dfrom && self.line_differs(t, line)).map(|mut t| {
                    // Walk back to where execution entered this line, skipping the calls it made.
                    let (l, dt) = (self.line_of_state(t), d[t as usize]);
                    while t > 0 {
                        let p = t as usize - 1;
                        if d[p] > dt {
                            let Some(q) = (0..p).rev().find(|&q| d[q] <= dt) else { break };
                            if d[q] == dt && self.line_of_state(q as u32) == l {
                                t = q as u32;
                                continue;
                            }
                            break;
                        }
                        if d[p] == dt && self.line_of_state(p as u32) == l {
                            t = p as u32;
                        } else {
                            break;
                        }
                    }
                    t
                })
            }
            NavOp::RegPrev(id) => {
                let ds = &self.trace.reg_deltas;
                let hi = ds.partition_point(|x| x.seq <= from);
                ds[..hi].iter().rev().find(|x| x.reg == *id).map(|x| x.seq - 1)
            }
            NavOp::RegNext(id) => {
                let ds = &self.trace.reg_deltas;
                let lo = ds.partition_point(|x| x.seq <= from + 1);
                ds[lo..].iter().find(|x| x.reg == *id).map(|x| x.seq - 1)
            }
            NavOp::WritePrev(addr) => self.write_prev(from, *addr),
            NavOp::WriteNext(addr) => self.write_next(from, *addr),
            NavOp::AtNext(addr) => {
                let steps = self.insn_by_addr.get(addr).map_or(&[][..], |&idx| &self.occ[idx as usize]);
                let hit = steps.get(steps.partition_point(|&s| s <= from)).copied();
                hit.or_else(|| (from < n && self.regs_at(n).rip == *addr).then_some(n))
            }
            NavOp::LineNext { file, line } => {
                let files = self.source_files(file)?;
                (from + 1..n).find(|&t| self.enters_line(t, &files, *line))
            }
        })
    }

    /// Whether state `t` has a known source line different from `other`.
    fn line_differs(&self, t: u32, other: Option<(u32, u32)>) -> bool {
        let l = self.line_of_state(t);
        l.is_some() && l != other
    }

    /// The largest step `i < from` whose writes cover `addr`.
    fn write_prev(&self, from: u32, addr: u64) -> Option<u32> {
        let list = self.page_index.get(&(addr & !(crate::trace::PAGE_SIZE - 1)))?;
        let hi = list.partition_point(|&wi| self.trace.writes[wi as usize].seq <= from);
        list[..hi]
            .iter()
            .rev()
            .map(|&wi| &self.trace.writes[wi as usize])
            .find(|w| w.kind != crate::trace::WriteKind::Snapshot && w.seq >= 1 && w.addr <= addr && addr < w.addr + w.data.len() as u64)
            .map(|w| w.seq - 1)
    }

    /// The smallest step `i > from` whose writes cover `addr`.
    fn write_next(&self, from: u32, addr: u64) -> Option<u32> {
        let list = self.page_index.get(&(addr & !(crate::trace::PAGE_SIZE - 1)))?;
        let lo = list.partition_point(|&wi| self.trace.writes[wi as usize].seq <= from + 1);
        list[lo..]
            .iter()
            .map(|&wi| &self.trace.writes[wi as usize])
            .find(|w| w.kind != crate::trace::WriteKind::Snapshot && w.addr <= addr && addr < w.addr + w.data.len() as u64)
            .map(|w| w.seq - 1)
    }
}
