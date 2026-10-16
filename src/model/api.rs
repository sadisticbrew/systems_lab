//! Views the HTTP API serves; the server only routes to these.

use anyhow::Result;
use iced_x86::{Decoder, DecoderOptions};

use super::asm::Fmt;
use super::views::*;
use super::{FLAGS, LoadedTrace, NavOp, RootFrame, flag_names};
use crate::exit;
use crate::trace::{EventKind, reg};

/// Largest symbol `disasm_view` decodes end to end.
const MAX_DECODED_SYMBOL: u64 = 64 * 1024;

const REG_ORDER: [u8; 20] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, reg::RIP, reg::RFLAGS, reg::FS_BASE, reg::GS_BASE];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl LoadedTrace {
    pub(super) fn check_step(&self, s: u32) -> Result<()> {
        if s > self.n {
            return Err(exit::usage(format!("step {s} is outside 0..={}", self.n)));
        }
        Ok(())
    }

    /// `GET /api/info`.
    pub fn api_info(&self) -> ApiInfo {
        let m = &self.trace.meta;
        ApiInfo {
            program: m.program.clone(),
            argv: m.argv.clone(),
            from: m.from.clone(),
            steps: self.n,
            end: EndView::of(&m.end),
            aslr_disabled: m.aslr_disabled,
            unattributed_bytes: m.unattributed_bytes,
            has_dwarf: self.has_dwarf(),
            functions: self
                .syms
                .exe_functions()
                .into_iter()
                .map(|f| FunctionView { name: f.name.clone(), loc: self.loc(f.addr) })
                .collect(),
        }
    }

    /// `GET /api/state/{S}`.
    pub fn state_view(&self, s: u32) -> Result<StateView> {
        self.check_step(s)?;
        let regs = self.regs_at(s);
        let prev_regs = (s > 0).then(|| self.regs_at(s - 1));
        let changed: Vec<u8> = if s == 0 { Vec::new() } else { self.deltas_of_step(s - 1).iter().map(|d| d.reg).collect() };
        let reg_views = REG_ORDER
            .iter()
            .map(|&id| {
                let v = regs.get(id) as u64;
                let rendered = match id {
                    reg::RIP => self.loc(v),
                    reg::RFLAGS => format!("[{}]", flag_names(v).join(" ")),
                    _ => self.render_value(v, s),
                };
                let changed = changed.contains(&id);
                let prev = prev_regs.as_ref().filter(|_| changed).map(|p| format!("0x{:x}", p.get(id) as u64));
                RegView { name: reg::name(id), value: format!("0x{v:x}"), rendered, changed, prev }
            })
            .collect();
        let flags = OrderedMap(FLAGS.iter().map(|&(n, bit)| (n.to_string(), (regs.rflags >> bit & 1) as u8)).collect());
        let xmm = (0..16u8)
            .map(|i| XmmView {
                name: format!("xmm{i}"),
                value: format!("0x{:032x}", regs.xmm[i as usize]),
                changed: changed.contains(&(reg::XMM0 + i)),
            })
            .collect();
        let bytes = if s < self.n {
            hex(&self.trace.insns[self.trace.steps[s as usize].insn as usize].bytes)
        } else {
            match self.insn_by_addr.get(&regs.rip) {
                Some(&i) => hex(&self.trace.insns[i as usize].bytes),
                None => String::new(),
            }
        };
        let prev_writes = if s > 0 { self.writes_of_step(s - 1).iter().map(|w| self.write_view(w, s)).collect() } else { Vec::new() };
        Ok(StateView {
            step: s,
            kind: self.step_kind(s),
            loc: self.loc(regs.rip),
            asm: self.asm_at(s),
            bytes,
            locator: self.locator_of(s),
            source: self.source_ref(self.line_of_state(s)),
            regs: reg_views,
            flags,
            xmm,
            prev_writes,
            effects: self.effects(s),
            depth: self.acts.depth[s as usize],
            stack: self.stack_view(s, None, RootFrame::Always),
        })
    }

    /// `GET /api/timeline`: the states `from..=to`, at most 1000 depth buckets, and the markers inside.
    pub fn timeline(&self, from: u32, to: u32) -> Result<Timeline> {
        if from > to || to > self.n {
            return Err(exit::usage(format!("timeline window {from}..={to} is not within 0..={}", self.n)));
        }
        let count = (to - from) as usize + 1;
        let buckets = count.min(1000);
        let depth = (0..buckets)
            .map(|b| {
                let lo = from as usize + b * count / buckets;
                let hi = from as usize + (b + 1) * count / buckets;
                self.acts.depth[lo..hi].iter().copied().max().unwrap_or(0)
            })
            .collect();
        let all = self.markers.get_or_init(|| self.compute_markers());
        let lo = all.partition_point(|m| m.step < from);
        let hi = all.partition_point(|m| m.step <= to);
        Ok(Timeline { steps: self.n, from, to, depth, markers: all[lo..hi].to_vec() })
    }

    /// `GET /api/nav`: `nav` plus the `loc` and `asm` of the state it lands on.
    pub fn nav_view(&self, from: u32, op: &NavOp) -> Result<NavView> {
        let step = self.nav(from, op)?;
        Ok(NavView { step, loc: step.map(|t| self.loc(self.rip_at(t))), asm: step.map(|t| self.asm_at(t)) })
    }

    /// Call, ret, syscall, signal and ABI markers ordered by step; the part of the timeline that does not
    /// depend on the window.
    fn compute_markers(&self) -> Vec<Marker> {
        let mut markers = Vec::new();
        for act in self.acts.list.iter().skip(1) {
            if !(self.syms.in_exe(self.rip_at(act.call_step)) || self.syms.in_exe(act.entry)) {
                continue;
            }
            let callee = self.loc(act.entry);
            markers.push(Marker { step: act.call_step, kind: "call", label: callee.clone() });
            if let Some(r) = act.ret_step {
                markers.push(Marker { step: r, kind: "ret", label: format!("ret {callee}") });
            }
        }
        for e in &self.trace.events {
            match e.kind {
                EventKind::Syscall { nr, .. } => markers.push(Marker { step: e.step, kind: "syscall", label: crate::syscalls::name(nr) }),
                EventKind::Signal { signo, .. } => markers.push(Marker { step: e.step, kind: "signal", label: crate::signals::name(signo) }),
            }
        }
        for f in self.abi_findings(false) {
            markers.push(Marker { step: f.step, kind: "abi", label: f.rule.to_string() });
        }
        markers.sort_by_key(|m| m.step);
        markers
    }

    /// `GET /api/mem`.
    pub fn mem_view(&self, addr: u64, len: usize, s: u32) -> Result<MemView> {
        self.check_step(s)?;
        if !(1..=4096).contains(&len) {
            return Err(exit::usage("len must be in 1..=4096"));
        }
        let (bytes, writers) = self.read_with_writers(s, addr, len);
        Ok(MemView { addr: format!("0x{addr:x}"), bytes, writers })
    }

    /// `GET /api/disasm`.
    pub fn disasm_view(&self, addr: u64, s: u32) -> Result<DisasmView> {
        self.check_step(s)?;
        let mut fmt = Fmt::new(&self.syms);
        let mut lines = Vec::new();
        let mut push = |this: &Self, fmt: &mut Fmt, at: u64, bytes: &[u8], ins: &iced_x86::Instruction| {
            let line = this.dwarf.line_at(at);
            lines.push(DisasmLine {
                addr: format!("0x{at:x}"),
                loc: this.loc(at),
                bytes: hex(bytes),
                asm: fmt.asm(ins),
                source_line: line.map(|(_, l)| l),
                source_file: line.map(|(f, _)| this.file_display[f as usize].clone()),
                hits: this.insn_by_addr.get(&at).map_or(0, |&i| this.occ[i as usize].len() as u32),
            });
        };
        let sym = self.syms.symbol_at(addr);
        let func = match sym {
            Some((s, Some(l))) => format!("{l}:{}", s.name),
            Some((s, None)) => s.name.clone(),
            None => self.loc(addr),
        };
        // A bigger symbol (a data object, a bogus `.size`) lists only the executed instructions near `addr`.
        if let Some((sym, _)) = sym.filter(|(s, _)| s.size <= MAX_DECODED_SYMBOL) {
            let code: Vec<u8> = self.read_mem(s, sym.addr, sym.size as usize).into_iter().map_while(|b| b).collect();
            let mut d = Decoder::with_ip(64, &code, sym.addr, DecoderOptions::NONE);
            let mut off = 0usize;
            while d.can_decode() {
                let ins = d.decode();
                let len = ins.len().max(1);
                let at = sym.addr + off as u64;
                push(self, &mut fmt, at, &code[off..(off + len).min(code.len())], &ins);
                off += len;
            }
        } else {
            for &i in &self.insns_sorted {
                let insn = &self.trace.insns[i as usize];
                if insn.addr + 256 < addr || insn.addr > addr + 256 {
                    continue;
                }
                if let Some(ins) = super::asm::decode(&insn.bytes, insn.addr) {
                    push(self, &mut fmt, insn.addr, &insn.bytes, &ins);
                }
            }
        }
        Ok(DisasmView { func, lines })
    }

    /// `GET /api/source`: the embedded source whose display path is `file`.
    pub fn source_view(&self, file: &str) -> Option<SourceView> {
        let s = self.trace.sources.iter().find(|s| s.display == file)?;
        Some(SourceView { file: s.display.clone(), lines: s.text.lines().map(str::to_string).collect() })
    }
}
