//! `compare`: facts about two recordings side by side (steps, output, syscalls, calls and steps per
//! function) and the first point where their event sequences differ.

use std::collections::HashMap;

use serde::Serialize;

use super::{EndView, LoadedTrace};
use crate::trace::EventKind;

/// Output bytes are read from the recording in chunks of this size.
const OUTPUT_CHUNK: u64 = 64 * 1024;

/// One call, syscall or signal of a recording, in step order.
struct EventAt {
    /// `call`, `syscall` or `signal`.
    kind: &'static str,
    /// The callee's function name, the syscall name or the signal name.
    name: String,
    step: u32,
}

/// What `compare` reads from one recording.
struct Profile {
    steps: u32,
    end: String,
    /// Bytes the program wrote to stdout and stderr.
    output: Vec<u8>,
    syscalls: Vec<String>,
    /// Executable function name to (calls, steps).
    functions: HashMap<String, (u64, u64)>,
    /// Steps outside every executable function (libraries, unsymbolized code).
    other_steps: u64,
    events: Vec<EventAt>,
}

impl LoadedTrace {
    fn profile(&self) -> Profile {
        let funcs = self.syms.exe_functions();
        let mut by_addr: HashMap<u64, (u64, u64)> = funcs.iter().map(|f| (f.addr, (0, 0))).collect();
        for act in &self.acts.list {
            if let Some(f) = by_addr.get_mut(&act.entry) {
                f.0 += 1;
            }
        }
        let mut other_steps = 0;
        for (idx, insn) in self.trace.insns.iter().enumerate() {
            let hits = self.occ[idx].len() as u64;
            match self.syms.symbol_at(insn.addr) {
                Some((s, None)) if by_addr.contains_key(&s.addr) => by_addr.get_mut(&s.addr).unwrap().1 += hits,
                _ => other_steps += hits,
            }
        }
        let mut functions: HashMap<String, (u64, u64)> = HashMap::new();
        for f in &funcs {
            let (calls, steps) = by_addr[&f.addr];
            let e = functions.entry(f.name.clone()).or_default();
            e.0 += calls;
            e.1 += steps;
        }

        // Same filter as the timeline's call markers: calls from or into the executable.
        let mut events = Vec::new();
        for act in self.acts.list.iter().skip(1) {
            if self.syms.in_exe(self.rip_at(act.call_step)) || self.syms.in_exe(act.entry) {
                events.push(EventAt { kind: "call", name: self.func_name_at(act.entry), step: act.call_step });
            }
        }
        let mut output = Vec::new();
        let mut syscalls = Vec::new();
        for e in &self.trace.events {
            match e.kind {
                EventKind::Syscall { nr, args, ret } => {
                    syscalls.push(crate::syscalls::name(nr));
                    events.push(EventAt { kind: "syscall", name: crate::syscalls::name(nr), step: e.step });
                    if nr == libc::SYS_write as u64 && matches!(args[0] as u32, 1 | 2) {
                        self.append_written(&mut output, e.step, args[1], args[2].min(ret.unwrap_or(0).max(0) as u64));
                    }
                }
                EventKind::Signal { signo, .. } => {
                    events.push(EventAt { kind: "signal", name: crate::signals::name(signo), step: e.step })
                }
            }
        }
        events.sort_by_key(|e| e.step);
        Profile {
            steps: self.n,
            end: EndView::of(&self.trace.meta.end).to_string(),
            output,
            syscalls,
            functions,
            other_steps,
            events,
        }
    }

    /// Appends the `len` bytes at `addr` in `state(s)` to `out`, stopping at the first byte the recording
    /// cannot supply (a successful `write` only reads mapped memory, so none is expected).
    fn append_written(&self, out: &mut Vec<u8>, s: u32, addr: u64, len: u64) {
        let mut off = 0;
        while off < len {
            let n = (len - off).min(OUTPUT_CHUNK) as usize;
            let bytes: Vec<u8> = self.read_mem(s, addr.wrapping_add(off), n).into_iter().map_while(|b| b).collect();
            let short = bytes.len() < n;
            out.extend_from_slice(&bytes);
            if short {
                return;
            }
            off += n as u64;
        }
    }
}

/// One recording's column of the comparison.
#[derive(Debug, Clone, Serialize)]
pub struct SideView {
    /// The trace path as the caller named it.
    pub trace: String,
    pub steps: u32,
    /// `EndView` text: `exit 0`, `signal SIGSEGV`, `max-steps`.
    pub end: String,
    pub output_bytes: usize,
    /// Number of syscalls.
    pub syscalls: usize,
}

/// One executable function's calls and steps in both recordings.
#[derive(Debug, Clone, Serialize)]
pub struct FunctionRow {
    pub name: String,
    pub a_calls: u64,
    pub a_steps: u64,
    pub b_calls: u64,
    pub b_steps: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepsPair {
    pub a: u64,
    pub b: u64,
}

/// An event of one recording: `event` is `call NAME`, `syscall NAME` or `signal NAME`.
#[derive(Debug, Clone, Serialize)]
pub struct EventRef {
    pub event: String,
    pub step: u32,
    pub locator: String,
}

/// The first index where the event sequences differ; a side is `None` when its sequence ended.
#[derive(Debug, Clone, Serialize)]
pub struct Divergence {
    pub index: usize,
    pub a: Option<EventRef>,
    pub b: Option<EventRef>,
}

/// Where the syscall name sequences first differ (text output only).
#[derive(Debug, Clone)]
pub struct SyscallDiff {
    pub index: usize,
    pub a: Option<String>,
    pub b: Option<String>,
}

/// The `compare` command's view.
#[derive(Debug, Clone, Serialize)]
pub struct Comparison {
    pub a: SideView,
    pub b: SideView,
    pub output_identical: bool,
    pub syscalls_identical: bool,
    /// Functions with any call or step, most steps first.
    pub functions: Vec<FunctionRow>,
    pub other_steps: StepsPair,
    pub divergence: Option<Divergence>,
    #[serde(skip)]
    pub syscall_diff: Option<SyscallDiff>,
}

/// First index where `a` and `b` differ, or where the shorter one ends; `None` when equal.
fn first_difference<T>(a: &[T], b: &[T], same: impl Fn(&T, &T) -> bool) -> Option<usize> {
    (0..a.len().max(b.len())).find(|&i| match (a.get(i), b.get(i)) {
        (Some(x), Some(y)) => !same(x, y),
        _ => true,
    })
}

/// Compares two recordings; the names label the sides.
pub fn compare(a_name: &str, a: &LoadedTrace, b_name: &str, b: &LoadedTrace) -> Comparison {
    let (pa, pb) = (a.profile(), b.profile());

    let mut names: Vec<&String> = pa.functions.keys().chain(pb.functions.keys()).collect();
    names.sort();
    names.dedup();
    let mut functions: Vec<FunctionRow> = names
        .into_iter()
        .map(|name| {
            let (a_calls, a_steps) = pa.functions.get(name).copied().unwrap_or_default();
            let (b_calls, b_steps) = pb.functions.get(name).copied().unwrap_or_default();
            FunctionRow { name: name.clone(), a_calls, a_steps, b_calls, b_steps }
        })
        .filter(|r| r.a_calls + r.a_steps + r.b_calls + r.b_steps > 0)
        .collect();
    functions.sort_by_key(|r| std::cmp::Reverse(r.a_steps.max(r.b_steps)));

    let divergence = first_difference(&pa.events, &pb.events, |x, y| x.kind == y.kind && x.name == y.name).map(|index| {
        let side = |p: &Profile, t: &LoadedTrace| {
            p.events.get(index).map(|e| EventRef { event: format!("{} {}", e.kind, e.name), step: e.step, locator: t.locator_of(e.step) })
        };
        Divergence { index, a: side(&pa, a), b: side(&pb, b) }
    });
    let syscall_diff = first_difference(&pa.syscalls, &pb.syscalls, |x, y| x == y)
        .map(|index| SyscallDiff { index, a: pa.syscalls.get(index).cloned(), b: pb.syscalls.get(index).cloned() });

    let side = |name: &str, p: &Profile| SideView {
        trace: name.to_string(),
        steps: p.steps,
        end: p.end.clone(),
        output_bytes: p.output.len(),
        syscalls: p.syscalls.len(),
    };
    Comparison {
        a: side(a_name, &pa),
        b: side(b_name, &pb),
        output_identical: pa.output == pb.output,
        syscalls_identical: syscall_diff.is_none(),
        functions,
        other_steps: StepsPair { a: pa.other_steps, b: pb.other_steps },
        divergence,
        syscall_diff,
    }
}
