//! The stack view shared by `stack`, `dot stack` and the viewer.

use serde::Serialize;

use super::dwarf::{DwFunc, DwVar, Enc, FrameBase};
use super::LoadedTrace;
use crate::trace::RegFile;

#[derive(Debug, Clone, Serialize)]
pub struct SlotView {
    /// `render_value` of the slot address.
    pub addr: String,
    pub raw_addr: String,
    /// `8(%rbp)`-style offset from the frame's saved `%rbp`, when the frame has one.
    pub rbp_off: Option<String>,
    pub label: String,
    /// `render_value` of the 8 bytes in the slot.
    pub value: String,
    pub raw_value: String,
    /// `%rsp` and/or `%rbp`, in that order.
    pub pointers: Vec<String>,
    /// The step that last wrote the slot's first byte; `None` for the initial snapshot.
    pub writer: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FrameView {
    pub label: String,
    pub func: String,
    pub collapsed: bool,
    /// Bytes spanned by the slots; `None` for the collapsed row.
    pub size: Option<u64>,
    pub slots: Vec<SlotView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StackView {
    pub step: u32,
    pub loc: String,
    pub asm: String,
    pub frames: Vec<FrameView>,
}

/// An activation chain at one state, outermost (root) first.
struct Ctx<'a> {
    lt: &'a LoadedTrace,
    s: u32,
    chain: Vec<u32>,
    now: RegFile,
}

impl Ctx<'_> {
    fn act(&self, k: usize) -> &super::Activation {
        &self.lt.acts.list[self.chain[k] as usize]
    }

    fn innermost(&self, k: usize) -> bool {
        k + 1 == self.chain.len()
    }

    /// The pc used to find the function and DWARF scope of activation `k`.
    fn scope_pc(&self, k: usize) -> u64 {
        if self.innermost(k) { self.now.rip } else { self.act(k + 1).ret_addr.wrapping_sub(1) }
    }

    fn regs(&self, k: usize) -> RegFile {
        if self.innermost(k) { self.now } else { self.lt.regs_at(self.act(k + 1).call_step) }
    }

    fn frame_base(&self, k: usize, f: &DwFunc) -> Option<u64> {
        match f.frame_base {
            FrameBase::CallFrameCfa => self.act(k).ret_slot.map(|r| r + 8),
            FrameBase::Rbp(o) => Some(self.regs(k).rbp().wrapping_add_signed(o)),
            FrameBase::Rsp(o) => Some(self.regs(k).rsp().wrapping_add_signed(o)),
            FrameBase::Unsupported => None,
        }
    }

    fn func_name(&self, k: usize) -> String {
        if k == 0 { self.lt.func_name_at(self.lt.acts.list[0].entry) } else { self.lt.func_name_at(self.scope_pc(k)) }
    }

    fn label(&self, k: usize) -> String {
        let name = self.func_name(k);
        if k == 0 {
            return name;
        }
        let pc = self.scope_pc(k);
        let Some(f) = self.lt.dwarf.func_at(pc) else { return name };
        let Some(fb) = self.frame_base(k, f) else { return name };
        let call_step = self.act(k).call_step;
        let params: Vec<String> = f
            .vars_at(pc)
            .filter(|v| v.param)
            .map(|v| {
                let addr = fb.wrapping_add_signed(v.fbreg);
                match self.lt.last_writer(self.s, addr) {
                    Some(w) if w >= call_step => self.lt.var_value(v, addr, self.s),
                    _ => "?".to_string(),
                }
            })
            .collect();
        format!("{name}({})", params.join(", "))
    }
}

impl LoadedTrace {
    pub(crate) fn func_name_at(&self, addr: u64) -> String {
        self.syms.func_name(addr)
    }

    /// Text of a DWARF variable's value read at `state(s)`.
    pub(crate) fn var_value(&self, v: &DwVar, addr: u64, s: u32) -> String {
        let size = v.size as usize;
        if size == 0 {
            return "?".into();
        }
        let Some(b) = self.read_mem(s, addr, size).into_iter().collect::<Option<Vec<u8>>>() else { return "?".into() };
        let mut raw = [0u8; 8];
        let n = size.min(8);
        raw[..n].copy_from_slice(&b[..n]);
        let unsigned = u64::from_le_bytes(raw);
        match (v.enc, size) {
            (Enc::Signed, 1 | 2 | 4 | 8) => {
                let shift = 64 - 8 * size as u32;
                (((unsigned << shift) as i64) >> shift).to_string()
            }
            (Enc::Unsigned, 1 | 2 | 4 | 8) => unsigned.to_string(),
            (Enc::Bool, _) => (b[0] != 0).to_string(),
            (Enc::Float, 4) => f32::from_bits(unsigned as u32).to_string(),
            (Enc::Float, 8) => f64::from_bits(unsigned).to_string(),
            (Enc::Pointer, 8) => self.render_value(unsigned, s),
            _ => {
                let mut out = b.iter().take(16).map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ");
                if b.len() > 16 {
                    out.push_str(" …");
                }
                out
            }
        }
    }

    /// The stack at `state(s)`. `frames` limits the shown non-root activations (default: all).
    pub fn stack_view(&self, s: u32, frames: Option<usize>) -> StackView {
        let s = s.min(self.n);
        let mut chain = Vec::new();
        let mut id = Some(self.acts.act_at[s as usize]);
        while let Some(i) = id {
            chain.push(i);
            id = self.acts.list[i as usize].parent;
        }
        chain.reverse();
        let now = self.regs_at(s);
        let ctx = Ctx { lt: self, s, chain, now };
        let nonroot = ctx.chain.len() - 1;
        let shown = frames.map_or(nonroot, |f| f.min(nonroot));
        let collapsed = ctx.chain.len() - shown;
        let kc = collapsed - 1;
        let mut out = vec![FrameView { label: ctx.label(kc), func: ctx.func_name(kc), collapsed: true, size: None, slots: Vec::new() }];
        let rsp_slot = now.rsp() & !7;
        for k in collapsed..ctx.chain.len() {
            out.push(self.frame_view(&ctx, k, rsp_slot));
        }
        StackView { step: s, loc: self.loc(now.rip), asm: self.asm_at(s), frames: out }
    }

    fn frame_view(&self, ctx: &Ctx<'_>, k: usize, rsp_slot: u64) -> FrameView {
        let s = ctx.s;
        let act = ctx.act(k);
        let top = act.ret_slot.unwrap_or(rsp_slot);
        let bottom = if ctx.innermost(k) { rsp_slot } else { ctx.act(k + 1).ret_slot.map_or(top, |r| r + 8) };
        let count = if bottom <= top { (top - bottom) / 8 + 1 } else { 1 };
        let pc = ctx.scope_pc(k);
        let dw = self.dwarf.func_at(pc);
        let fb = dw.and_then(|f| ctx.frame_base(k, f));
        let vars: Vec<(&DwVar, u64)> = match (dw, fb) {
            (Some(f), Some(fb)) => f.vars_at(pc).map(|v| (v, fb.wrapping_add_signed(v.fbreg))).collect(),
            _ => Vec::new(),
        };
        let mut slots: Vec<SlotView> = Vec::new();
        let mut saved_rbp_addr = None;
        for i in 0..count {
            let addr = top - 8 * i;
            let (bytes, writers) = self.read_with_writers(s, addr, 8);
            let mut raw = [0u8; 8];
            for (j, b) in bytes.iter().enumerate() {
                raw[j] = b.unwrap_or(0);
            }
            let value = u64::from_le_bytes(raw);
            let w = writers[0];
            let written = w.is_some_and(|w| w >= act.call_step);
            let push_reg = w.filter(|_| written).and_then(|w| self.insn_info[self.trace.steps[w as usize].insn as usize].push_reg.clone());
            let label = if Some(addr) == act.ret_slot {
                format!(
                    "return address: {} in {}",
                    self.mnemonic_at(act.ret_addr, s),
                    self.func_name_at(act.ret_addr.wrapping_sub(1))
                )
            } else if let Some(reg) = push_reg {
                if reg == "rbp" {
                    saved_rbp_addr.get_or_insert(addr);
                    format!("saved %rbp: {}'s", ctx.label(k - 1))
                } else {
                    format!("saved %{reg}")
                }
            } else if let Some(l) = self.var_label(&vars, addr, written, s) {
                l
            } else if written {
                self.render_value(value, s)
            } else {
                "unused".to_string()
            };
            let mut pointers = Vec::new();
            if addr == rsp_slot {
                pointers.push("%rsp".to_string());
            }
            if addr == ctx.now.rbp() {
                pointers.push("%rbp".to_string());
            }
            slots.push(SlotView {
                addr: self.render_value(addr, s),
                raw_addr: format!("0x{addr:x}"),
                rbp_off: None,
                label,
                value: self.render_value(value, s),
                raw_value: format!("0x{value:x}"),
                pointers,
                writer: w,
            });
        }
        if let Some(fp) = saved_rbp_addr {
            for (i, slot) in slots.iter_mut().enumerate() {
                let addr = top - 8 * i as u64;
                slot.rbp_off = Some(format!("{}(%rbp)", addr.wrapping_sub(fp) as i64));
            }
        }
        FrameView { label: ctx.label(k), func: ctx.func_name(k), collapsed: false, size: Some(slots.len() as u64 * 8), slots }
    }

    /// Rule 3: DWARF variables overlapping the slot at `addr`.
    fn var_label(&self, vars: &[(&DwVar, u64)], addr: u64, written: bool, s: u32) -> Option<String> {
        let parts: Vec<String> = vars
            .iter()
            .filter(|(v, va)| *va < addr + 8 && *va + v.size.max(1) > addr)
            .map(|(v, va)| {
                if written { format!("{} = {}", v.name, self.var_value(v, *va, s)) } else { format!("{}, not written yet", v.name) }
            })
            .collect();
        (!parts.is_empty()).then(|| parts.join(", "))
    }
}
