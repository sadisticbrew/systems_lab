//! Call/ret activation model: one sequential pass over the steps.

use super::LoadedTrace;
use crate::trace::{StepKind, reg};

#[derive(Debug, Clone)]
pub struct Activation {
    pub parent: Option<u32>,
    /// The root has depth 0.
    pub depth: u32,
    /// Step of the `call` (0 for the root).
    pub call_step: u32,
    /// Where the return address sits; `None` for a root recorded from `entry`.
    pub ret_slot: Option<u64>,
    pub ret_addr: u64,
    /// The `ret` that closed it; `None` while open or discarded.
    pub ret_step: Option<u32>,
    /// `rip` right after the call (the root: `state(0).rip`).
    pub entry: u64,
}

#[derive(Default)]
pub struct Activations {
    /// Index 0 is the root.
    pub list: Vec<Activation>,
    /// Innermost activation id at every state `0..=N`.
    pub act_at: Vec<u32>,
    /// Activation depth at every state `0..=N`.
    pub depth: Vec<u32>,
}

pub fn build(lt: &LoadedTrace) -> Activations {
    let t = &lt.trace;
    let n = lt.n as usize;
    let start = lt.regs_at(0);
    let root_known = t.meta.from != "entry";
    let mut list = vec![Activation {
        parent: None,
        depth: 0,
        call_step: 0,
        ret_slot: root_known.then(|| start.rsp()),
        ret_addr: if root_known { lt.read_u64(0, start.rsp()).unwrap_or(0) } else { 0 },
        ret_step: None,
        entry: start.rip,
    }];
    let mut stack = vec![0u32];
    let mut act_at = Vec::with_capacity(n + 1);
    let mut depth = Vec::with_capacity(n + 1);
    act_at.push(0);
    depth.push(0);
    let deltas = &t.reg_deltas;
    let mut di = deltas.partition_point(|d| d.seq == 0);
    let (mut rsp, mut rip) = (start.rsp(), start.rip);
    for s in 0..n {
        let rsp_before = rsp;
        while di < deltas.len() && deltas[di].seq as usize <= s + 1 {
            match deltas[di].reg {
                reg::RSP => rsp = deltas[di].value as u64,
                reg::RIP => rip = deltas[di].value as u64,
                _ => {}
            }
            di += 1;
        }
        let step = t.steps[s];
        if step.kind == StepKind::Instr {
            let info = &lt.insn_info[step.insn as usize];
            if info.call {
                let parent = *stack.last().unwrap();
                list.push(Activation {
                    parent: Some(parent),
                    depth: list[parent as usize].depth + 1,
                    call_step: s as u32,
                    ret_slot: Some(rsp),
                    ret_addr: lt.read_u64(s as u32 + 1, rsp).unwrap_or(0),
                    ret_step: None,
                    entry: rip,
                });
                stack.push(list.len() as u32 - 1);
            } else if info.ret {
                while let Some(&top) = stack.last() {
                    if top != 0 && list[top as usize].ret_slot.is_some_and(|r| r < rsp_before) {
                        stack.pop();
                    } else {
                        break;
                    }
                }
                if let Some(&top) = stack.last()
                    && top != 0
                    && list[top as usize].ret_slot == Some(rsp_before)
                {
                    list[top as usize].ret_step = Some(s as u32);
                    stack.pop();
                }
            }
        }
        let top = *stack.last().unwrap();
        act_at.push(top);
        depth.push(list[top as usize].depth);
    }
    Activations { list, act_at, depth }
}
