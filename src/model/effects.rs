//! What the instruction at `state(s).rip` does (`State.effects`): the registers and memory it reads
//! and writes, the flags it reads and sets, its condition, and how it moves control. Reads and the
//! condition come from decoding the instruction against `state(s)`; every written value comes from
//! the recording (`state(s+1)` and the writes of step `s`).

use iced_x86::{ConditionCode, FlowControl, Instruction, InstructionInfoFactory, OpAccess, OpKind, Register, RflagsBits};

use super::views::*;
use super::{FLAGS, LoadedTrace, asm, write_kind_name};
use crate::trace::{RegFile, StepKind, reg};
use crate::x86;

/// iced's mask for each flag of `FLAGS`; `TF` has none because no instruction names it.
fn iced_mask(flag: &str) -> u32 {
    match flag {
        "CF" => RflagsBits::CF,
        "PF" => RflagsBits::PF,
        "AF" => RflagsBits::AF,
        "ZF" => RflagsBits::ZF,
        "SF" => RflagsBits::SF,
        "IF" => RflagsBits::IF,
        "DF" => RflagsBits::DF,
        "OF" => RflagsBits::OF,
        _ => 0,
    }
}

/// Condition codes with the text and meaning the viewer shows.
const CONDITIONS: [(ConditionCode, &str, &str, &str); 16] = [
    (ConditionCode::o, "o", "OF = 1", "overflow"),
    (ConditionCode::no, "no", "OF = 0", "no overflow"),
    (ConditionCode::b, "b", "CF = 1", "below (unsigned) / carry"),
    (ConditionCode::ae, "ae", "CF = 0", "above or equal (unsigned) / no carry"),
    (ConditionCode::e, "e", "ZF = 1", "equal / zero"),
    (ConditionCode::ne, "ne", "ZF = 0", "not equal / not zero"),
    (ConditionCode::be, "be", "CF = 1 or ZF = 1", "below or equal (unsigned)"),
    (ConditionCode::a, "a", "CF = 0 and ZF = 0", "above (unsigned)"),
    (ConditionCode::s, "s", "SF = 1", "negative"),
    (ConditionCode::ns, "ns", "SF = 0", "not negative"),
    (ConditionCode::p, "p", "PF = 1", "parity even"),
    (ConditionCode::np, "np", "PF = 0", "parity odd"),
    (ConditionCode::l, "l", "SF ≠ OF", "less (signed)"),
    (ConditionCode::ge, "ge", "SF = OF", "greater or equal (signed)"),
    (ConditionCode::le, "le", "ZF = 1 or SF ≠ OF", "less or equal (signed)"),
    (ConditionCode::g, "g", "ZF = 0 and SF = OF", "greater (signed)"),
];

/// Whether `cc` holds for the flags in `rflags`.
fn condition_holds(cc: ConditionCode, rflags: u64) -> bool {
    let bit = |n: u32| rflags >> n & 1 == 1;
    let (cf, pf, zf, sf, of) = (bit(0), bit(2), bit(6), bit(7), bit(11));
    match cc {
        ConditionCode::o => of,
        ConditionCode::no => !of,
        ConditionCode::b => cf,
        ConditionCode::ae => !cf,
        ConditionCode::e => zf,
        ConditionCode::ne => !zf,
        ConditionCode::be => cf || zf,
        ConditionCode::a => !cf && !zf,
        ConditionCode::s => sf,
        ConditionCode::ns => !sf,
        ConditionCode::p => pf,
        ConditionCode::np => !pf,
        ConditionCode::l => sf != of,
        ConditionCode::ge => sf == of,
        ConditionCode::le => zf || sf != of,
        ConditionCode::g => !zf && sf == of,
        ConditionCode::None => false,
    }
}

fn reads(a: OpAccess) -> bool {
    matches!(a, OpAccess::Read | OpAccess::CondRead | OpAccess::ReadWrite | OpAccess::ReadCondWrite)
}

fn writes(a: OpAccess) -> bool {
    matches!(a, OpAccess::Write | OpAccess::CondWrite | OpAccess::ReadWrite | OpAccess::ReadCondWrite)
}

/// Whether trace register `id` is one the effects list as written: a general-purpose register, an
/// xmm register, or a segment base.
fn listed_write(id: u8) -> bool {
    id < 16 || (reg::XMM0..reg::XMM0 + 16).contains(&id) || id == reg::FS_BASE || id == reg::GS_BASE
}

/// A register's value as `0x…` text: 128 bits for xmm registers, else the low 64.
fn reg_hex(r: &RegFile, id: u8) -> String {
    if (reg::XMM0..reg::XMM0 + 16).contains(&id) { format!("0x{:032x}", r.get(id)) } else { format!("0x{:x}", r.get(id) as u64) }
}

impl LoadedTrace {
    /// `render_value` for a general-purpose register or segment base; the hex text itself for xmm.
    fn reg_rendered(&self, r: &RegFile, id: u8, s: u32) -> String {
        if (reg::XMM0..reg::XMM0 + 16).contains(&id) { reg_hex(r, id) } else { self.render_value(r.get(id) as u64, s) }
    }

    /// Text of a memory value of `size` bytes whose first `min(size, 16)` bytes are `bytes`: size 8 as a
    /// rendered pointer-sized value, 1, 2 and 4 as unsigned decimal, anything else as hex bytes.
    fn memory_text(&self, bytes: &[u8], size: usize, s: u32) -> String {
        let le = |n: usize| {
            let mut v = [0u8; 8];
            v[..n].copy_from_slice(&bytes[..n]);
            u64::from_le_bytes(v)
        };
        match size {
            8 if bytes.len() >= 8 => self.render_value(le(8), s),
            1 | 2 | 4 if bytes.len() >= size => le(size).to_string(),
            _ => {
                let mut text = bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
                if size > 16 {
                    text.push_str(" …");
                }
                text
            }
        }
    }

    /// The text of `size` bytes at `addr` in `state(s)`; `?` when one is not mapped.
    fn memory_at(&self, s: u32, addr: u64, size: usize) -> String {
        match self.read_mem(s, addr, size.min(16)).into_iter().collect::<Option<Vec<u8>>>() {
            Some(bytes) => self.memory_text(&bytes, size, s),
            None => "?".to_string(),
        }
    }

    /// `GET /api/state/{S}`'s `effects`: `None` at the final state.
    pub fn effects(&self, s: u32) -> Option<EffectsView> {
        if s >= self.n {
            return None;
        }
        let step = self.trace.steps[s as usize];
        let (before, after) = (self.regs_at(s), self.regs_at(s + 1));
        let raw = &self.trace.insns[step.insn as usize];
        let decoded: Option<Instruction> = asm::decode(&raw.bytes, raw.addr);
        let mut factory = InstructionInfoFactory::new();
        // Entering a signal handler replaces the interrupted instruction with the kernel's frame setup:
        // that instruction reads and writes nothing, so only the recorded changes describe the step.
        let entered_handler = matches!(step.kind, StepKind::Deliver { .. }) && after.rip != before.rip;
        let run: Option<&Instruction> = decoded.as_ref().filter(|_| !entered_handler);
        let info = run.map(|d| factory.info(d));

        let mut reg_reads: Vec<RegReadView> = Vec::new();
        let mut written: Vec<(u8, Option<String>)> = Vec::new();
        if let Some(info) = info {
            for used in info.used_registers() {
                let r = used.register();
                if reads(used.access()) {
                    let view = match r {
                        Register::CS | Register::DS | Register::ES | Register::SS | Register::RIP => None,
                        Register::FS | Register::GS => {
                            let (name, v) = if r == Register::FS { ("fs_base", before.fs_base) } else { ("gs_base", before.gs_base) };
                            Some(RegReadView { name: name.to_string(), value: format!("0x{v:x}"), rendered: self.render_value(v, s) })
                        }
                        r if r.is_xmm() && r.number() < 16 => {
                            let hex = format!("0x{:032x}", before.xmm[r.number()]);
                            Some(RegReadView { name: x86::reg_name(r), value: hex.clone(), rendered: hex })
                        }
                        r => x86::reg_value(&before, r).map(|v| RegReadView {
                            name: x86::reg_name(r),
                            value: format!("0x{v:x}"),
                            rendered: self.render_value(v, s),
                        }),
                    };
                    if let Some(v) = view.filter(|v| !reg_reads.iter().any(|x| x.name == v.name)) {
                        reg_reads.push(v);
                    }
                }
                if writes(used.access())
                    && let Some(id) = x86::full_reg_id(r).filter(|&id| listed_write(id))
                    && !written.iter().any(|(w, _)| *w == id)
                {
                    let via = (!r.is_gpr64() && x86::reg_name(r) != reg::name(id)).then(|| x86::reg_name(r));
                    written.push((id, via));
                }
            }
        }
        for d in self.deltas_of_step(s) {
            if listed_write(d.reg) && !written.iter().any(|(w, _)| *w == d.reg) {
                written.push((d.reg, None));
            }
        }
        // iced reports a 32-bit write as a write of the whole 64-bit register (it zero-extends), so the
        // register the instruction spells comes from its destination operand.
        let operand_via = |id: u8| -> Option<String> {
            let (d, info) = (run?, info?);
            (0..d.op_count())
                .filter(|&i| d.op_kind(i) == OpKind::Register && writes(info.op_access(i)))
                .map(|i| d.op_register(i))
                .find(|&r| x86::full_reg_id(r) == Some(id))
                .filter(|r| !r.is_gpr64())
                .map(x86::reg_name)
        };
        let reg_writes = written
            .into_iter()
            .map(|(id, via)| RegWriteView {
                name: reg::name(id),
                via: via.or_else(|| operand_via(id)).filter(|v| *v != reg::name(id)),
                before: reg_hex(&before, id),
                after: reg_hex(&after, id),
                before_rendered: self.reg_rendered(&before, id, s),
                after_rendered: self.reg_rendered(&after, id, s + 1),
                changed: before.get(id) != after.get(id),
            })
            .collect();

        let mem_reads = info
            .map(|info| {
                info.used_memory()
                    .iter()
                    .filter(|m| reads(m.access()) && m.memory_size().size() > 0)
                    .filter_map(|m| {
                        let addr = m.virtual_address(0, |r, _, _| x86::reg_value(&before, r))?;
                        let size = m.memory_size().size();
                        Some(MemReadView {
                            addr: self.render_value(addr, s),
                            raw_addr: format!("0x{addr:x}"),
                            size,
                            value: self.memory_at(s, addr, size),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mem_writes = self
            .writes_of_step(s)
            .iter()
            .map(|w| MemChangeView {
                addr: self.render_value(w.addr, s),
                raw_addr: format!("0x{:x}", w.addr),
                size: w.data.len(),
                kind: write_kind_name(w.kind),
                before: self.memory_at(s, w.addr, w.data.len()),
                after: self.memory_text(&w.data[..w.data.len().min(16)], w.data.len(), s + 1),
            })
            .collect();

        let (read_mask, modified_mask, undefined_mask) = run.map_or((0, 0, 0), |d| (d.rflags_read(), d.rflags_modified(), d.rflags_undefined()));
        let bit = |r: &RegFile, n: u32| (r.rflags >> n & 1) as u8;
        let flags_read = FLAGS
            .iter()
            .filter(|(name, _)| iced_mask(name) & read_mask != 0)
            .map(|&(name, n)| FlagReadView { name, value: bit(&before, n) })
            .collect();
        let flags_written = FLAGS
            .iter()
            .filter(|&&(name, n)| iced_mask(name) & modified_mask != 0 || bit(&before, n) != bit(&after, n))
            .map(|&(name, n)| FlagChangeView { name, before: bit(&before, n), after: bit(&after, n), undefined: iced_mask(name) & undefined_mask != 0 })
            .collect();

        let condition = run.and_then(|d| {
            let cc = d.condition_code();
            CONDITIONS
                .iter()
                .find(|c| c.0 == cc)
                .map(|&(_, code, text, meaning)| ConditionView { code, text, meaning, holds: condition_holds(cc, before.rflags) })
        });

        let control = match (&decoded, step.kind) {
            (Some(d), StepKind::Instr) => {
                let next_ip = d.next_ip();
                let known = &self.insn_info[step.insn as usize];
                if known.call {
                    Some(ControlView::Call {
                        target: self.loc(after.rip),
                        return_address: self.loc(next_ip),
                        slot: self.render_value(after.rsp(), s + 1),
                        rsp_before: self.render_value(before.rsp(), s),
                        rsp_after: self.render_value(after.rsp(), s + 1),
                        rsp_mod16: (before.rsp() % 16) as u8,
                    })
                } else if known.ret {
                    Some(ControlView::Ret {
                        to: self.loc(after.rip),
                        slot: self.render_value(before.rsp(), s),
                        rsp_before: self.render_value(before.rsp(), s),
                        rsp_after: self.render_value(after.rsp(), s + 1),
                        call_step: self.acts.closing_call(s),
                    })
                } else {
                    match d.flow_control() {
                        FlowControl::ConditionalBranch => {
                            let taken = after.rip != next_ip;
                            let target = if taken { after.rip } else { d.near_branch_target() };
                            Some(ControlView::Branch { conditional: true, taken, target: self.loc(target), fallthrough: self.loc(next_ip) })
                        }
                        FlowControl::UnconditionalBranch | FlowControl::IndirectBranch => {
                            Some(ControlView::Branch { conditional: false, taken: true, target: self.loc(after.rip), fallthrough: self.loc(next_ip) })
                        }
                        _ => None,
                    }
                }
            }
            _ => None,
        };

        let delivered = match step.kind {
            StepKind::Deliver { signo } => Some(DeliveredView {
                signal: crate::signals::name(signo),
                handler: (after.rip != before.rip).then(|| self.loc(after.rip)),
            }),
            _ => None,
        };

        Some(EffectsView {
            mnemonic: decoded.as_ref().map_or_else(|| "(bad)".to_string(), |d| format!("{:?}", d.mnemonic()).to_lowercase()),
            kind: self.step_kind(s),
            next_loc: self.loc(after.rip),
            reg_reads,
            reg_writes,
            mem_reads,
            mem_writes,
            flags_read,
            flags_written,
            condition,
            control,
            event: self.event_of(s).map(|e| self.event_detail(e)),
            delivered,
        })
    }
}
