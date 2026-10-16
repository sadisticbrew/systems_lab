//! Serde view structs shared by the CLI renderers and the HTTP API.

use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};

use super::expr::WatchType;
use crate::trace::End;

/// A JSON object whose keys keep their insertion order.
#[derive(Debug, Clone, Default)]
pub struct OrderedMap<V>(pub Vec<(String, V)>);

impl<V: Serialize> Serialize for OrderedMap<V> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WriteView {
    /// `render_value` of the address.
    pub addr: String,
    pub raw_addr: String,
    pub size: usize,
    /// `render_value` of up to the first 8 bytes, little endian.
    pub value: String,
    /// `snapshot`, `instr`, `kernel` or `unattributed`.
    pub kind: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum EventView {
    Syscall { nr: u64, name: String, args: Vec<String>, ret: Option<String> },
    Signal { signal: String, code: i32, addr: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct StepView {
    pub step: u32,
    pub loc: String,
    pub asm: String,
    pub regs: OrderedMap<String>,
    pub rflags: Option<Vec<String>>,
    pub writes: Vec<WriteView>,
    pub event: Option<EventView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EndView {
    /// `exit`, `signal` or `max-steps`.
    pub reason: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,
}

impl EndView {
    pub fn of(end: &End) -> EndView {
        match *end {
            End::Exit { code } => EndView { reason: "exit", code: Some(code), signal: None },
            End::Signal { signo } => EndView { reason: "signal", code: None, signal: Some(crate::signals::name(signo)) },
            End::MaxSteps => EndView { reason: "max-steps", code: None, signal: None },
        }
    }
}

impl std::fmt::Display for EndView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.reason, self.code, &self.signal) {
            ("exit", Some(c), _) => write!(f, "exit {c}"),
            ("signal", _, Some(s)) => write!(f, "signal {s}"),
            (r, _, _) => f.write_str(r),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NameCount {
    pub name: String,
    pub count: u64,
}

/// The `info` command.
#[derive(Debug, Clone, Serialize)]
pub struct InfoView {
    pub program: String,
    pub argv: Vec<String>,
    pub from: String,
    pub steps: u32,
    pub end: EndView,
    /// `disabled` or `enabled`.
    pub aslr: &'static str,
    pub unattributed_bytes: u64,
    /// By first occurrence.
    pub syscalls: Vec<NameCount>,
    pub signals: Vec<NameCount>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionView {
    pub name: String,
    pub loc: String,
}

/// `GET /api/info`.
#[derive(Debug, Clone, Serialize)]
pub struct ApiInfo {
    pub program: String,
    pub argv: Vec<String>,
    pub from: String,
    pub steps: u32,
    pub end: EndView,
    pub aslr_disabled: bool,
    pub unattributed_bytes: u64,
    pub has_dwarf: bool,
    pub functions: Vec<FunctionView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceRef {
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegView {
    pub name: String,
    /// `0x…`.
    pub value: String,
    pub rendered: String,
    pub changed: bool,
    /// `value` at the previous state (`0x…`); `Some` only when `changed`.
    pub prev: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct XmmView {
    pub name: String,
    pub value: String,
    pub changed: bool,
}

/// `GET /api/state/{S}`.
#[derive(Debug, Clone, Serialize)]
pub struct StateView {
    pub step: u32,
    /// `instr`, `fault`, `deliver`, `exit`, or `end` for the final state.
    pub kind: &'static str,
    pub loc: String,
    pub asm: String,
    /// Instruction bytes as hex.
    pub bytes: String,
    /// A locator that resolves back to this state (`LoadedTrace::locator_of`).
    pub locator: String,
    pub source: Option<SourceRef>,
    pub regs: Vec<RegView>,
    pub flags: OrderedMap<u8>,
    pub xmm: Vec<XmmView>,
    /// Writes of the previous step, which produced `state(s)`; they match the registers' `changed` flags.
    pub prev_writes: Vec<WriteView>,
    /// What the instruction about to run does; `None` at the final state.
    pub effects: Option<EffectsView>,
    pub depth: u32,
    pub stack: super::stack::StackView,
}

#[derive(Debug, Clone, Serialize)]
pub struct Marker {
    pub step: u32,
    /// `call`, `ret`, `syscall`, `signal` or `abi`.
    pub kind: &'static str,
    pub label: String,
}

/// `GET /api/timeline`: the states `from..=to`.
#[derive(Debug, Clone, Serialize)]
pub struct Timeline {
    /// Number of steps `N` of the whole recording.
    pub steps: u32,
    pub from: u32,
    pub to: u32,
    /// Maximum call depth per bucket (at most 1000 buckets over the window).
    pub depth: Vec<u32>,
    /// Markers within the window, ordered by step.
    pub markers: Vec<Marker>,
}

/// `GET /api/mem`.
#[derive(Debug, Clone, Serialize)]
pub struct MemView {
    pub addr: String,
    pub bytes: Vec<Option<u8>>,
    pub writers: Vec<Option<u32>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DisasmLine {
    pub addr: String,
    pub loc: String,
    pub bytes: String,
    pub asm: String,
    pub source_line: Option<u32>,
    /// Display name of the source file `source_line` belongs to.
    pub source_file: Option<String>,
    /// How many steps execute this instruction; 0 when it never ran.
    pub hits: u32,
}

/// `GET /api/disasm`.
#[derive(Debug, Clone, Serialize)]
pub struct DisasmView {
    pub func: String,
    pub lines: Vec<DisasmLine>,
}

/// `GET /api/source`.
#[derive(Debug, Clone, Serialize)]
pub struct SourceView {
    pub file: String,
    pub lines: Vec<String>,
}

/// `GET /api/nav`: where the navigation lands (all `None` when it does not move).
#[derive(Debug, Clone, Serialize)]
pub struct NavView {
    pub step: Option<u32>,
    /// `loc` of the target state.
    pub loc: Option<String>,
    /// `asm` of the target state.
    pub asm: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegReadView {
    /// Lowercase register name as the instruction spells it (`eax`, `rbp`, `fs_base`, `xmm0`).
    pub name: String,
    /// `0x…` value of that register in `state(s)`.
    pub value: String,
    pub rendered: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegWriteView {
    /// The full register (`rax`, `xmm1`, `fs_base`).
    pub name: String,
    /// The narrower register the instruction names (`eax`), when it is not a 64-bit one.
    pub via: Option<String>,
    /// `0x…` full-register values in `state(s)` and `state(s+1)`.
    pub before: String,
    pub after: String,
    pub before_rendered: String,
    pub after_rendered: String,
    pub changed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemReadView {
    /// `render_value` of the address.
    pub addr: String,
    pub raw_addr: String,
    pub size: usize,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemChangeView {
    pub addr: String,
    pub raw_addr: String,
    pub size: usize,
    /// `instr`, `kernel`, `snapshot` or `unattributed`.
    pub kind: &'static str,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlagReadView {
    pub name: &'static str,
    pub value: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlagChangeView {
    pub name: &'static str,
    pub before: u8,
    pub after: u8,
    /// The manual calls the flag undefined after this instruction.
    pub undefined: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConditionView {
    /// `g`, `ne`, `be`, …
    pub code: &'static str,
    /// `ZF = 0 and SF = OF`.
    pub text: &'static str,
    /// `greater (signed)`.
    pub meaning: &'static str,
    /// Evaluated on the flags before the instruction.
    pub holds: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ControlView {
    Branch { conditional: bool, taken: bool, target: String, fallthrough: String },
    Call { target: String, return_address: String, slot: String, rsp_before: String, rsp_after: String, rsp_mod16: u8 },
    Ret { to: String, slot: String, rsp_before: String, rsp_after: String, call_step: Option<u32> },
}

#[derive(Debug, Clone, Serialize)]
pub struct DeliveredView {
    pub signal: String,
    /// Where the handler starts; `None` when the signal ended the process or did not enter a handler.
    pub handler: Option<String>,
}

/// What the instruction at `state(s).rip` does, from decoding it and from the recording.
#[derive(Debug, Clone, Serialize)]
pub struct EffectsView {
    /// Lowercase iced mnemonic (`mov`, `jg`); `(bad)` when the bytes do not decode.
    pub mnemonic: String,
    /// `instr`, `fault`, `deliver` or `exit`.
    pub kind: &'static str,
    /// `loc` of `state(s+1).rip`.
    pub next_loc: String,
    pub reg_reads: Vec<RegReadView>,
    pub reg_writes: Vec<RegWriteView>,
    pub mem_reads: Vec<MemReadView>,
    pub mem_writes: Vec<MemChangeView>,
    pub flags_read: Vec<FlagReadView>,
    pub flags_written: Vec<FlagChangeView>,
    pub condition: Option<ConditionView>,
    pub control: Option<ControlView>,
    pub event: Option<EventDetail>,
    pub delivered: Option<DeliveredView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyscallArgView {
    /// The register holding it (`rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9`).
    pub reg: &'static str,
    /// The kernel's name for the argument (`fd`, `buf`, `count`); `arg0`… for unknown syscalls.
    pub name: String,
    pub value: String,
    /// `0x…` register value.
    pub raw: String,
    /// `stdout`, `AT_FDCWD`, a signal name…
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BufferView {
    /// The name of the argument that points at it.
    pub arg: String,
    /// `in` (read by the kernel at the syscall) or `out` (filled by it).
    pub dir: &'static str,
    pub addr: String,
    pub len: usize,
    /// Lossy UTF-8 of the bytes; `None` when part of the buffer is not mapped.
    pub text: Option<String>,
    pub truncated: bool,
}

/// A syscall or signal event with everything the viewer shows about it.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum EventDetail {
    Syscall {
        step: u32,
        nr: u64,
        name: String,
        args: Vec<SyscallArgView>,
        ret: Option<String>,
        ret_raw: Option<String>,
        errno: Option<String>,
        errno_text: Option<String>,
        buffers: Vec<BufferView>,
    },
    Signal {
        step: u32,
        signal: String,
        code: i32,
        code_name: Option<String>,
        code_text: Option<String>,
        addr: String,
        cpu_exception: Option<String>,
    },
}

/// `GET /api/eval`.
#[derive(Debug, Clone, Serialize)]
pub struct EvalView {
    /// `0x…`, the 64-bit result.
    pub value: String,
    /// `render_value` of the result.
    pub rendered: String,
    pub default_type: WatchType,
}

/// A value read from memory as a `WatchType`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypedValue {
    pub text: Option<String>,
    pub raw: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InspectValue {
    #[serde(rename = "type")]
    pub ty: WatchType,
    pub value: Option<String>,
}

/// `GET /api/inspect`.
#[derive(Debug, Clone, Serialize)]
pub struct InspectView {
    /// `0x…`.
    pub addr: String,
    pub rendered: String,
    /// `[stack] (rw-p)`, `libc.so.6 (r-xp)`, `anonymous (rw-p)` or `unmapped`.
    pub region: String,
    /// The 8 bytes at `addr`; `None` for an unmapped byte.
    pub bytes: Vec<Option<u8>>,
    /// `u8 i8 u16 i16 u32 i32 u64 i64 f32 f64 ptr` read at `addr`, in that order.
    pub values: Vec<InspectValue>,
    pub cstring: Option<String>,
    /// The step that last wrote the byte at `addr`; `None` for initial contents.
    pub writer: Option<u32>,
    pub writer_asm: Option<String>,
    /// The next step after this state that writes the byte.
    pub next_writer: Option<u32>,
}

/// `GET /api/watch`, one entry per watched expression.
#[derive(Debug, Clone, Serialize)]
pub struct WatchView {
    pub expr: String,
    #[serde(rename = "type")]
    pub ty: WatchType,
    /// `0x…` address the value was read from; `None` for `value` watches and errors.
    pub addr: Option<String>,
    pub value: Option<String>,
    pub raw: Option<String>,
    /// `value` as it was at the previous state.
    pub prev: Option<String>,
    pub changed: bool,
    pub error: Option<String>,
}
