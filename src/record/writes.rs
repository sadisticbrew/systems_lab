//! Instruction decoding and planning of the memory each instruction writes.

use std::collections::HashMap;

use iced_x86::{Decoder, DecoderOptions, InstructionInfoFactory, Mnemonic, OpAccess, UsedMemory};

use crate::trace::{Insn, RegFile, reg};
use crate::x86::reg_value;

use super::spawn::Tracee;

/// What the recorder needs to know about one instruction, computed once per address.
#[derive(Default)]
pub struct Plan {
    pub syscall: bool,
    /// `int $0x80`: the 32-bit system call entry. The kernel may write memory, but the 64-bit
    /// syscall table does not describe it, so only its memory effects are recorded, not an event.
    pub legacy_syscall: bool,
    /// `pushfq`: the real RFLAGS it stores contain the tracer's trap flag.
    pub pushf: bool,
    /// Writes to ES:RDI (movs/stos/ins); its range comes from the rdi delta.
    pub string_elem: Option<u64>,
    /// push/pushf/call/enter: the stack bytes below the old rsp are written.
    pub rsp_union: bool,
    /// Memory operands with a write access and a known size.
    pub mems: Vec<(UsedMemory, u64, bool)>,
    /// A write access of unknown size (xsave and friends): needs a full diff.
    pub unsized_write: bool,
}

#[derive(Clone, Copy)]
pub struct Range {
    pub start: u64,
    pub end: u64,
    /// Written only if the bytes change (CondWrite).
    pub cond: bool,
}

pub struct Insns {
    pub list: Vec<Insn>,
    pub plans: Vec<Plan>,
    map: HashMap<u64, u32>,
    factory: InstructionInfoFactory,
}

impl Insns {
    pub fn new() -> Self {
        Insns { list: Vec::new(), plans: Vec::new(), map: HashMap::new(), factory: InstructionInfoFactory::new() }
    }

    /// Index of the instruction at `rip`, fetching and decoding it on first use.
    pub fn index(&mut self, t: &Tracee, rip: u64) -> u32 {
        if let Some(&i) = self.map.get(&rip) {
            return i;
        }
        let mut buf = [0u8; 15];
        let n = t.read(rip, &mut buf);
        let mut plan = Plan::default();
        let mut len = n;
        if n > 0 {
            let mut dec = Decoder::with_ip(64, &buf[..n], rip, DecoderOptions::NONE);
            let ins = dec.decode();
            if !ins.is_invalid() {
                len = ins.len();
                plan.syscall = ins.mnemonic() == Mnemonic::Syscall;
                plan.legacy_syscall = ins.mnemonic() == Mnemonic::Int && ins.immediate8() == 0x80;
                plan.pushf = ins.mnemonic() == Mnemonic::Pushfq;
                plan.rsp_union = matches!(
                    ins.mnemonic(),
                    Mnemonic::Push | Mnemonic::Pushfq | Mnemonic::Pushfd | Mnemonic::Pushf | Mnemonic::Call | Mnemonic::Enter
                );
                let string = ins.is_string_instruction();
                for m in self.factory.info(&ins).used_memory() {
                    let (write, cond) = match m.access() {
                        OpAccess::Write | OpAccess::ReadWrite => (true, false),
                        OpAccess::CondWrite | OpAccess::ReadCondWrite => (true, true),
                        _ => (false, false),
                    };
                    if !write {
                        continue;
                    }
                    // iced reports a rep-prefixed string write as a CondWrite of unknown size; the
                    // element size is the instruction's own memory size.
                    let size = if string { ins.memory_size().size() } else { m.memory_size().size() } as u64;
                    if size == 0 {
                        plan.unsized_write = true;
                    } else if string {
                        plan.string_elem = Some(size);
                    } else {
                        plan.mems.push((*m, size, cond));
                    }
                }
            }
        }
        let idx = self.list.len() as u32;
        self.list.push(Insn { addr: rip, bytes: buf[..len].to_vec() });
        self.plans.push(plan);
        self.map.insert(rip, idx);
        idx
    }
}

/// Fills `out` with the ranges the instruction will write, with addresses
/// computed from `cur` (the state before the step).
pub fn plan_ranges(plan: &Plan, cur: &RegFile, out: &mut Vec<Range>) {
    out.clear();
    for (m, size, cond) in &plan.mems {
        if let Some(a) = m.virtual_address(0, |r, _, _| reg_value(cur, r)) {
            out.push(Range { start: a, end: a.wrapping_add(*size), cond: *cond });
        }
    }
}

/// Adds the ranges that depend on the registers after the step, then sorts
/// and merges overlapping ranges.
pub fn finish_ranges(plan: &Plan, cur: &RegFile, new: &RegFile, out: &mut Vec<Range>) {
    if let Some(elem) = plan.string_elem {
        let before = cur.gpr[reg::RDI as usize];
        let after = new.gpr[reg::RDI as usize];
        if after > before {
            out.push(Range { start: before, end: after, cond: false });
        } else if after < before {
            out.push(Range { start: after + elem, end: before + elem, cond: false });
        }
    }
    if plan.rsp_union && new.rsp() < cur.rsp() {
        out.push(Range { start: new.rsp(), end: cur.rsp(), cond: false });
    }
    if out.len() > 1 {
        out.sort_unstable_by_key(|r| r.start);
        let mut w = 0;
        for k in 1..out.len() {
            if out[k].start <= out[w].end {
                out[w].end = out[w].end.max(out[k].end);
                out[w].cond &= out[k].cond;
            } else {
                w += 1;
                out[w] = out[k];
            }
        }
        out.truncate(w + 1);
    }
}
