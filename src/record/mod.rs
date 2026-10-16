//! Ptrace single-step recorder: runs a program and captures every step, its
//! register changes and its memory writes into a [`Trace`].

mod elf;
mod maps;
mod shadow;
mod spawn;
mod writes;

use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::exit;
use crate::signals;
use crate::trace::{
    self, CHECKPOINT_INTERVAL, End, Event, EventKind, Exe, LibSyms, MapsSnapshot, MemWrite, Meta, PAGE_SIZE,
    RegDelta, RegFile, Region, Step, StepKind, Trace, WriteKind, reg,
};

use shadow::{DiffRun, Shadow};
use spawn::{Stop, Tracee};
use writes::{Insns, Range};

/// Steps between periodic full diffs that look for unattributed writes.
const PERIODIC_DIFF_STEPS: u32 = 65_536;
/// Unattributed writes remembered for the `--strict` report.
const UNATTRIBUTED_REPORT: usize = 5;
/// `si_code` of a single-step trap, and of a stepped syscall.
const TRAP_TRACE: i32 = 2;
const TRAP_BRKPT: i32 = 1;
/// `si_code` of an int3 trap.
const SI_KERNEL: i32 = 0x80;
/// The trap flag in RFLAGS: the tracer sets it to single-step.
const TRAP_FLAG: u64 = 1 << 8;

pub struct Options {
    pub from: String,
    pub max_steps: u64,
    pub strict: bool,
    pub quiet: bool,
    pub output: PathBuf,
    pub program: String,
    pub args: Vec<String>,
}

/// The result of a recording plus the first unattributed writes (`addr`, `step`).
pub struct Recording {
    pub trace: Trace,
    pub unattributed: Vec<(u64, u32)>,
}

/// `record` command: records, writes the trace file, prints the summary, applies `--strict`.
pub fn run(opts: &Options) -> Result<u8> {
    let rec = record(opts)?;
    trace::save(&opts.output, &rec.trace).map_err(|e| exit::usage(format!("{e:#}")))?;
    let meta = &rec.trace.meta;
    if !opts.quiet {
        let ended = match meta.end {
            End::Exit { code } => format!("exit {code}"),
            End::Signal { signo } => format!("signal {}", signals::name(signo)),
            End::MaxSteps => "max-steps".to_string(),
        };
        eprintln!(
            "recorded {} steps from {}; ended: {}; unattributed bytes: {}; wrote {}",
            rec.trace.steps.len(),
            meta.from,
            ended,
            meta.unattributed_bytes,
            opts.output.display()
        );
    }
    if opts.strict && meta.unattributed_bytes > 0 {
        for (addr, step) in &rec.unattributed {
            eprintln!("unattributed write at {addr:#x}, found at step {step}");
        }
        return Err(exit::strict(format!(
            "--strict: {} bytes of memory changed without an instruction or syscall that accounts for them",
            meta.unattributed_bytes
        )));
    }
    Ok(0)
}

fn unsupported_event(t: &Tracee, event: i32, step: u32) -> anyhow::Error {
    t.kill_event_child(event);
    if event == libc::PTRACE_EVENT_EXEC {
        exit::unsupported(format!("unsupported: the program called execve at step {step}"))
    } else {
        exit::unsupported(format!(
            "unsupported: the program created a thread or process (clone/fork/vfork) at step {step}"
        ))
    }
}

/// A job-control stop (SIGSTOP, SIGTSTP, …): only another process could resume the program.
fn group_stop(sig: i32, when: &str) -> anyhow::Error {
    exit::unsupported(format!(
        "unsupported: the program was stopped by {} {when}; only another process can resume a stopped program",
        signals::name(sig)
    ))
}

/// Runs the tracee until it reaches the requested start point; returns the
/// resolved `from` name.
fn run_to_start(t: &Tracee, from: &str, info: &elf::ExeInfo, bias: u64) -> Result<String> {
    let find = |name: &str| info.syms.iter().chain(&info.starts).find(|s| s.name == name);
    let name = match from {
        "auto" if find("main").is_some() => "main",
        "auto" | "entry" => return Ok("entry".to_string()),
        other => other,
    };
    let sym = find(name).ok_or_else(|| exit::usage(format!("unknown symbol '{name}'")))?;
    let addr = bias + sym.addr;
    let orig = t.peek(addr)?;
    t.poke(addr, (orig & !0xff) | 0xcc)?;
    let mut sig = 0;
    loop {
        t.cont(sig)?;
        match t.wait()? {
            Stop::Exited(_) | Stop::Signaled(_) => {
                return Err(exit::usage(format!("program exited before reaching {name}")));
            }
            Stop::Stopped { event, .. } if event != 0 => return Err(unsupported_event(t, event, 0)),
            Stop::Stopped { sig: libc::SIGTRAP, .. } => {
                // Only our own int3 (SI_KERNEL, rip just past it) is consumed;
                // any other SIGTRAP belongs to the program.
                let (_, code, _) = t.siginfo()?;
                if code == SI_KERNEL && t.rip()? == addr + 1 {
                    t.poke(addr, orig)?;
                    t.set_rip(addr)?;
                    return Ok(name.to_string());
                }
                sig = libc::SIGTRAP;
            }
            Stop::Stopped { sig: s, .. } => {
                if t.in_group_stop() {
                    return Err(group_stop(s, &format!("before reaching {name}")));
                }
                sig = s;
            }
        }
    }
}

fn exe_path_of(pid: libc::pid_t) -> Result<String> {
    Ok(std::fs::read_link(format!("/proc/{pid}/exe")).context("reading /proc/<pid>/exe")?.to_string_lossy().into_owned())
}

fn push_deltas(out: &mut Vec<RegDelta>, seq: u32, old: &RegFile, new: &RegFile) {
    for id in 0..reg::COUNT {
        let (a, b) = (old.get(id), new.get(id));
        if a != b {
            out.push(RegDelta { seq, reg: id, value: b });
        }
    }
}

struct Recorder {
    t: Tracee,
    regions: Vec<Region>,
    ids: Vec<String>,
    shadow: Shadow,
    maps: Vec<MapsSnapshot>,
    writes: Vec<MemWrite>,
    libs: Vec<LibSyms>,
    lib_seen: HashSet<String>,
    exe_path: String,
    runs: Vec<DiffRun>,
    buf: Vec<u8>,
    unattributed_bytes: u64,
    unattributed: Vec<(u64, u32)>,
}

impl Recorder {
    /// Page-splits `data` into write records.
    fn push_write(&mut self, seq: u32, addr: u64, data: &[u8], kind: WriteKind) {
        let mut addr = addr;
        let mut rest = data;
        while !rest.is_empty() {
            let off = (addr & (PAGE_SIZE - 1)) as usize;
            let n = rest.len().min(PAGE_SIZE as usize - off);
            self.writes.push(MemWrite { seq, addr, data: rest[..n].to_vec(), kind });
            addr += n as u64;
            rest = &rest[n..];
        }
    }

    fn snapshot_ranges(&mut self, seq: u32, ranges: &[(u64, u64)]) {
        let mut found: Vec<(u64, Vec<u8>)> = Vec::new();
        for &(s, e) in ranges {
            shadow::read_pages(&self.t, s, (e - s) / PAGE_SIZE, &mut self.buf, |addr, data| {
                found.push((addr, data.to_vec()));
            });
        }
        for (addr, data) in found {
            self.writes.push(MemWrite { seq, addr, data, kind: WriteKind::Snapshot });
        }
    }

    fn add_libs(&mut self, regions: &[Region]) {
        let mut new_paths = Vec::new();
        for r in regions {
            if r.path.starts_with('/') && r.path != self.exe_path && !self.lib_seen.contains(&r.path) {
                self.lib_seen.insert(r.path.clone());
                new_paths.push(r.path.clone());
            }
        }
        for p in new_paths {
            if let Some(l) = elf::lib_syms(&p, regions) {
                self.libs.push(l);
            }
        }
    }

    /// Re-reads the maps; on a change, records a snapshot and the newly readable memory.
    /// The new snapshots go before the records already pushed for `seq`: a later record
    /// wins when a state is read, and an instruction that wrote into a freshly grown page
    /// must keep its attribution.
    fn refresh_maps(&mut self, seq: u32) -> Result<()> {
        let (new, new_ids) = maps::parse_maps_ident(self.t.pid)?;
        if new == self.regions && new_ids == self.ids {
            return Ok(());
        }
        let fresh = maps::changed_readable(&new, &new_ids, &self.regions, &self.ids);
        self.maps.push(MapsSnapshot { seq, regions: new.clone() });
        let before = self.writes.len();
        self.snapshot_ranges(seq, &fresh);
        let (first, added) = (self.writes[..before].partition_point(|w| w.seq < seq), self.writes.len() - before);
        self.writes[first..].rotate_right(added);
        self.shadow.sync(&self.t, &new);
        self.add_libs(&new);
        self.regions = new;
        self.ids = new_ids;
        Ok(())
    }

    /// Full diff of writable memory against the shadow; every difference becomes a write of `kind`.
    fn full_diff(&mut self, seq: u32, kind: WriteKind) {
        let mut runs = std::mem::take(&mut self.runs);
        runs.clear();
        self.shadow.diff(&self.t, &self.regions, &mut runs);
        for run in &runs {
            if kind == WriteKind::Unattributed {
                self.unattributed_bytes += run.changed;
                if self.unattributed.len() < UNATTRIBUTED_REPORT {
                    self.unattributed.push((run.addr, seq.saturating_sub(1)));
                }
            }
            self.push_write(seq, run.addr, &run.data, kind);
        }
        self.runs = runs;
    }

    /// Records the instruction writes of the step that produced `seq`.
    /// Returns whether a written address was outside the known maps.
    fn record_instr_writes(&mut self, seq: u32, ranges: &[Range]) -> bool {
        let mut outside = false;
        let mut buf = std::mem::take(&mut self.buf);
        for r in ranges {
            let len = r.end.wrapping_sub(r.start) as usize;
            if len == 0 || len > (1 << 20) {
                continue;
            }
            buf.resize(len, 0);
            let n = self.t.read(r.start, &mut buf[..len]);
            if n == 0 {
                continue;
            }
            if !maps::is_mapped(&self.regions, r.start) {
                outside = true;
            }
            let data = &buf[..n];
            if r.cond && self.shadow.matches(r.start, data) {
                continue;
            }
            self.push_write(seq, r.start, data, WriteKind::Instr);
            self.shadow.update(r.start, data);
        }
        self.buf = buf;
        outside
    }
}

pub fn record(opts: &Options) -> Result<Recording> {
    let (t, aslr_disabled) = Tracee::spawn(&opts.program, &opts.args)?;
    let exe_path = exe_path_of(t.pid)?;
    let exe_bytes = std::fs::read(format!("/proc/{}/exe", t.pid)).context("reading the executable")?;
    let info = elf::analyze(&exe_bytes).map_err(|e| exit::usage(format!("{}: {e:#}", opts.program)))?;
    let exec_regions = maps::parse_maps(t.pid)?;
    let bias = elf::bias_of(&exec_regions, &exe_path, &info);
    let from = run_to_start(&t, &opts.from, &info, bias)?;

    let (regions, ids) = maps::parse_maps_ident(t.pid)?;
    let mut rec = Recorder {
        t,
        regions: regions.clone(),
        ids,
        shadow: Shadow::default(),
        maps: vec![MapsSnapshot { seq: 0, regions: regions.clone() }],
        writes: Vec::new(),
        libs: Vec::new(),
        lib_seen: HashSet::new(),
        exe_path: exe_path.clone(),
        runs: Vec::new(),
        buf: Vec::new(),
        unattributed_bytes: 0,
        unattributed: Vec::new(),
    };
    rec.snapshot_ranges(0, &maps::ranges(&regions, maps::is_readable));
    rec.shadow.sync(&rec.t, &regions);
    rec.add_libs(&regions);
    let sources = elf::sources(&exe_bytes);

    let mut insns = Insns::new();
    let mut steps: Vec<Step> = Vec::new();
    let mut reg_deltas: Vec<RegDelta> = Vec::new();
    let mut events: Vec<Event> = Vec::new();
    let mut checkpoints: Vec<RegFile> = Vec::new();
    let mut cur = RegFile::default();
    rec.t.regs(&mut cur)?;
    let mut new = cur;
    checkpoints.push(cur);
    let mut pending = 0i32;
    let mut ranges: Vec<Range> = Vec::new();
    let mut end = End::MaxSteps;
    let limit = opts.max_steps.min(u32::MAX as u64 - 1) as u32;

    for i in 0..limit {
        let seq = i + 1;
        let idx = insns.index(&rec.t, cur.rip);
        let plan = &insns.plans[idx as usize];
        let rax = cur.gpr[reg::RAX as usize];
        if plan.syscall && matches!(rax, 60 | 231) || plan.legacy_syscall && matches!(rax as u32, 1 | 252) {
            rec.full_diff(i, WriteKind::Unattributed);
        }
        writes::plan_ranges(plan, &cur, &mut ranges);
        rec.t.singlestep(pending)?;
        let delivered = std::mem::take(&mut pending);
        let delivered_kind = if delivered != 0 { StepKind::Deliver { signo: delivered } } else { StepKind::Instr };
        let syscall_event = |ret: Option<i64>| Event {
            step: i,
            kind: EventKind::Syscall {
                nr: cur.gpr[reg::RAX as usize],
                args: [
                    cur.gpr[reg::RDI as usize],
                    cur.gpr[reg::RSI as usize],
                    cur.gpr[reg::RDX as usize],
                    cur.gpr[10],
                    cur.gpr[8],
                    cur.gpr[9],
                ],
                ret,
            },
        };

        // A signal this resume delivers to a handler enters the handler instead of
        // running the interrupted instruction.
        let handler_entry = delivered != 0 && spawn::caught_signals(rec.t.pid) & (1u64 << (delivered - 1)) != 0;
        // A SIGTRAP that is not the single-step report (int3, raise, kill).
        let mut genuine_trap: Option<(i32, u64)> = None;

        let (kind, completed) = match rec.t.wait()? {
            Stop::Exited(code) => {
                rec.t.mark_dead();
                steps.push(Step { insn: idx, kind: StepKind::Exit });
                if plan.syscall {
                    events.push(syscall_event(None));
                }
                if seq % CHECKPOINT_INTERVAL == 0 {
                    checkpoints.push(cur);
                }
                end = End::Exit { code };
                break;
            }
            Stop::Signaled(signo) => {
                rec.t.mark_dead();
                steps.push(Step { insn: idx, kind: delivered_kind });
                if plan.syscall && !handler_entry {
                    events.push(syscall_event(None));
                }
                if seq % CHECKPOINT_INTERVAL == 0 {
                    checkpoints.push(cur);
                }
                end = End::Signal { signo };
                break;
            }
            Stop::Stopped { event, .. } if event != 0 => {
                let err = unsupported_event(&rec.t, event, i);
                rec.t.kill();
                return Err(err);
            }
            Stop::Stopped { sig: libc::SIGTRAP, .. } => {
                let (_, code, addr) = rec.t.siginfo()?;
                // Entering a handler reports its own trap (si_code TRAP_UNK).
                if code == TRAP_TRACE || code == TRAP_BRKPT || handler_entry {
                    (delivered_kind, !handler_entry)
                } else {
                    // The instruction completed if it was a user-sent signal that
                    // arrived after it ran (rip moved); int3 never completes.
                    rec.t.regs(&mut new)?;
                    let ran = code <= 0 && new.rip != cur.rip;
                    genuine_trap = Some((code, if code <= 0 { 0 } else { addr }));
                    if ran && !handler_entry {
                        (delivered_kind, true)
                    } else if ran {
                        (delivered_kind, false)
                    } else {
                        (StepKind::Fault { signo: libc::SIGTRAP }, false)
                    }
                }
            }
            Stop::Stopped { sig, .. } => {
                if rec.t.in_group_stop() {
                    rec.t.kill();
                    // The signal that led here was recorded as the previous step's fault.
                    return Err(group_stop(sig, &format!("at step {}", if delivered != 0 { i - 1 } else { i })));
                }
                (StepKind::Fault { signo: sig }, false)
            }
        };

        rec.t.regs(&mut new)?;
        // Single-stepping sets TF in the real RFLAGS. The program must not see it in %r11, where
        // `syscall` saves RFLAGS, or in the flags `pushfq` stores: it would behave differently.
        if completed && cur.rflags & TRAP_FLAG == 0 {
            if plan.syscall && new.gpr[11] & TRAP_FLAG != 0 {
                new.gpr[11] &= !TRAP_FLAG;
                rec.t.set_r11(new.gpr[11])?;
            }
            if plan.pushf {
                rec.t.clear_pushed_tf(new.rsp())?;
            }
        }
        push_deltas(&mut reg_deltas, seq, &cur, &new);
        steps.push(Step { insn: idx, kind });
        if seq % CHECKPOINT_INTERVAL == 0 {
            checkpoints.push(new);
        }
        let run_syscall_event = plan.syscall && !handler_entry;
        if run_syscall_event && completed {
            events.push(syscall_event(Some(new.gpr[reg::RAX as usize] as i64)));
        }
        if let Some((code, addr)) = genuine_trap {
            events.push(Event { step: i, kind: EventKind::Signal { signo: libc::SIGTRAP, code, addr } });
            pending = libc::SIGTRAP;
        } else if let StepKind::Fault { signo } = kind {
            let (_, code, addr) = rec.t.siginfo()?;
            events.push(Event { step: i, kind: EventKind::Signal { signo, code, addr } });
            pending = signo;
        }

        let mut maps_dirty = run_syscall_event;
        if completed {
            writes::finish_ranges(plan, &cur, &new, &mut ranges);
            maps_dirty |= rec.record_instr_writes(seq, &ranges);
        }
        let kernel = run_syscall_event || plan.legacy_syscall || !matches!(kind, StepKind::Instr) || handler_entry;
        if kernel && !run_syscall_event {
            // Signal frames and `int $0x80` calls may have grown or mapped memory: capture new pages first.
            rec.refresh_maps(seq)?;
            rec.full_diff(seq, WriteKind::Kernel);
        } else if kernel {
            rec.full_diff(seq, WriteKind::Kernel);
        } else if plan.unsized_write {
            rec.full_diff(seq, WriteKind::Instr);
        } else if seq % PERIODIC_DIFF_STEPS == 0 {
            rec.full_diff(seq, WriteKind::Unattributed);
        }
        if maps_dirty {
            rec.refresh_maps(seq)?;
        }
        std::mem::swap(&mut cur, &mut new);
    }

    if end == End::MaxSteps {
        rec.t.kill();
    }

    let trace = Trace {
        meta: Meta {
            tool: format!("systemslab {}", env!("CARGO_PKG_VERSION")),
            program: opts.program.clone(),
            exe_path: exe_path.clone(),
            argv: std::iter::once(opts.program.clone()).chain(opts.args.iter().cloned()).collect(),
            from,
            aslr_disabled,
            rseq_disabled: true,
            max_steps: opts.max_steps,
            end,
            unattributed_bytes: rec.unattributed_bytes,
        },
        exe: Exe { path: exe_path, bytes: exe_bytes, bias },
        libs: rec.libs,
        sources,
        maps: rec.maps,
        insns: insns.list,
        steps,
        reg_checkpoints: checkpoints,
        reg_deltas,
        writes: rec.writes,
        events,
    };
    Ok(Recording { trace, unattributed: rec.unattributed })
}
