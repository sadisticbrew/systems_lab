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
    AtNext(u64),
}

impl NavOp {
    /// Builds an op from the API's `op`, `reg` and `addr` (hex, optional `0x`) parameters.
    pub fn parse(op: &str, reg_name: Option<&str>, addr: Option<&str>) -> Result<NavOp> {
        let reg_id = || -> Result<u8> {
            let name = reg_name.ok_or_else(|| exit::usage(format!("op {op} needs reg=")))?;
            reg::by_name(name).ok_or_else(|| exit::usage(format!("unknown register '{name}'")))
        };
        let addr_val = || -> Result<u64> {
            let a = addr.ok_or_else(|| exit::usage(format!("op {op} needs addr=")))?;
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
            "at-next" => NavOp::AtNext(addr_val()?),
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
                let lo = ds.partition_point(|x| x.seq <= from);
                ds[lo..].iter().find(|x| x.reg == *id).map(|x| x.seq - 1)
            }
            NavOp::WritePrev(addr) => self.write_prev(from, *addr),
            NavOp::AtNext(addr) => {
                let hit = self.insn_by_addr.get(addr).and_then(|&idx| {
                    (from as usize + 1..self.trace.steps.len()).find(|&i| self.trace.steps[i].insn == idx).map(|i| i as u32)
                });
                hit.or_else(|| (from < n && self.regs_at(n).rip == *addr).then_some(n))
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
}
