//! System V ABI checks over the activations of a trace.

use serde::Serialize;

use super::LoadedTrace;
use crate::trace::reg;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub rule: &'static str,
    pub step: u32,
    pub loc: String,
    pub message: String,
}

const DF: u64 = 1 << 10;
const CALLEE_SAVED: [u8; 7] = [reg::RBX, reg::RBP, reg::R12, reg::R13, reg::R14, reg::R15, reg::RSP];

impl LoadedTrace {
    /// ABI findings ordered by step, then rule name. Without `all`, only calls located in the
    /// executable and activations whose callee entry is in it are checked.
    pub fn abi_findings(&self, all: bool) -> Vec<Finding> {
        let mut out = Vec::new();
        let finding = |rule: &'static str, step: u32, message: String| Finding {
            rule,
            step,
            loc: self.loc(self.rip_at(step)),
            message,
        };
        for act in self.acts.list.iter().skip(1) {
            let c = act.call_step;
            let call_in_scope = all || self.syms.in_exe(self.rip_at(c));
            let callee_in_scope = all || self.syms.in_exe(act.entry);
            if !call_in_scope && !callee_in_scope {
                continue;
            }
            let callee = self.loc(act.entry);
            let at_call = self.regs_at(c);
            // The kernel aligns the stack and clears DF when it enters a signal handler.
            if call_in_scope && !act.signal {
                let m = at_call.rsp() % 16;
                if m != 0 {
                    out.push(finding("call-alignment", c, format!("%rsp mod 16 = {m} before call to {callee} (must be 0)")));
                }
                if at_call.rflags & DF != 0 {
                    out.push(finding("direction-flag", c, format!("direction flag set at the call to {callee}")));
                }
            }
            let (true, Some(r)) = (callee_in_scope, act.ret_step) else { continue };
            let before_ret = self.regs_at(r);
            let after_ret = self.regs_at(r + 1);
            // The signal frame moved %rsp, which is restored by `rt_sigreturn`, not by the handler's `ret`.
            for id in CALLEE_SAVED.into_iter().filter(|&id| !(act.signal && id == reg::RSP)) {
                let (v1, v2) = (at_call.get(id) as u64, after_ret.get(id) as u64);
                if v1 != v2 {
                    out.push(finding(
                        "callee-saved",
                        c,
                        format!(
                            "{callee} changed %{}: {} at the call, {} after its ret (step {r})",
                            reg::name(id),
                            self.render_value(v1, c),
                            self.render_value(v2, r + 1)
                        ),
                    ));
                }
            }
            if before_ret.rflags & DF != 0 {
                out.push(finding("direction-flag", r, format!("direction flag set at the ret of {callee}")));
            }
            if after_ret.rip != act.ret_addr {
                out.push(finding(
                    "return-address",
                    r,
                    format!("ret went to {}, the call at step {c} pushed {}", self.loc(after_ret.rip), self.loc(act.ret_addr)),
                ));
            }
        }
        out.sort_by(|a, b| (a.step, a.rule).cmp(&(b.step, b.rule)));
        out
    }
}
