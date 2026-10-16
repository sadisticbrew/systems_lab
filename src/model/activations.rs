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
    /// Entered by the kernel delivering a signal to a handler rather than by a `call`.
    pub signal: bool,
}

#[derive(Default)]
pub struct Activations {
    /// Index 0 is the root.
    pub list: Vec<Activation>,
    /// Innermost activation id at every state `0..=N`.
    pub act_at: Vec<u32>,
    /// Activation depth at every state `0..=N`.
    pub depth: Vec<u32>,
    /// `(ret step, activation index)` of every closed activation, sorted.
    pub by_ret: Vec<(u32, u32)>,
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
        signal: false,
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
        let (rsp_before, rip_before) = (rsp, rip);
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
                    signal: false,
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
        if matches!(step.kind, StepKind::Deliver { .. }) && rip != rip_before {
            // The kernel enters the handler with the return address (the signal trampoline) on top of
            // the stack, exactly as a `call` would.
            let parent = *stack.last().unwrap();
            list.push(Activation {
                parent: Some(parent),
                depth: list[parent as usize].depth + 1,
                call_step: s as u32,
                ret_slot: Some(rsp),
                ret_addr: lt.read_u64(s as u32 + 1, rsp).unwrap_or(0),
                ret_step: None,
                entry: rip,
                signal: true,
            });
            stack.push(list.len() as u32 - 1);
        }
        // A live activation keeps %rsp at or below its return slot. Above it, the frame is gone without
        // a `ret` (longjmp, siglongjmp, a handler that never returns): discard it.
        while let Some(&top) = stack.last() {
            if top != 0 && list[top as usize].ret_slot.is_some_and(|r| r < rsp) {
                stack.pop();
            } else {
                break;
            }
        }
        let top = *stack.last().unwrap();
        act_at.push(top);
        depth.push(list[top as usize].depth);
    }
    let mut by_ret: Vec<(u32, u32)> = list.iter().enumerate().filter_map(|(i, a)| a.ret_step.map(|r| (r, i as u32))).collect();
    by_ret.sort_unstable();
    Activations { list, act_at, depth, by_ret }
}

impl Activations {
    /// The step of the call (or signal delivery) whose activation the `ret` at `ret_step` closed.
    pub fn closing_call(&self, ret_step: u32) -> Option<u32> {
        let i = self.by_ret.binary_search_by_key(&ret_step, |&(r, _)| r).ok()?;
        Some(self.list[self.by_ret[i].1 as usize].call_step)
    }
}

#[cfg(test)]
mod tests {
    use crate::model::LoadedTrace;
    use crate::record::{Options, record};

    /// `tests/fixtures/labels.s` calls `helper` three times; every `ret` names the `call` it closes.
    #[test]
    fn closing_call_names_the_call_a_ret_closes() {
        let dir = tempfile::tempdir().unwrap();
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/labels.s");
        let exe = dir.path().join("labels");
        let gcc = std::process::Command::new("gcc").args(["-nostdlib", "-static", "-o"]).arg(&exe).arg(&src).output().expect("gcc on PATH");
        assert!(gcc.status.success(), "{}", String::from_utf8_lossy(&gcc.stderr));
        let opts = Options {
            from: "auto".into(),
            max_steps: 10_000,
            strict: true,
            quiet: true,
            output: dir.path().join("labels.slt"),
            program: exe.to_string_lossy().into_owned(),
            args: Vec::new(),
        };
        let lt = LoadedTrace::from_trace(record(&opts).unwrap().trace).unwrap();
        for k in 1..=3 {
            // `helper` is a single `ret`, so its k-th execution is the ret step.
            let (call, ret) = (lt.locate(&format!("again@{k}")).unwrap(), lt.locate(&format!("helper@{k}")).unwrap());
            assert_eq!(lt.acts.closing_call(ret), Some(call), "call {k}");
        }
        // Only a `ret` closes an activation: a call, or a step past the end, closes nothing.
        assert_eq!(lt.acts.closing_call(lt.locate("again@1").unwrap()), None);
        assert_eq!(lt.acts.closing_call(lt.steps() + 5), None);
    }
}
