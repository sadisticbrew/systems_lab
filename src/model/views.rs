//! Serde view structs shared by the CLI renderers and the HTTP API.

use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};

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
    pub source: Option<SourceRef>,
    pub regs: Vec<RegView>,
    pub flags: OrderedMap<u8>,
    pub xmm: Vec<XmmView>,
    /// Writes of the step about to run (`state(s)` to `state(s+1)`).
    pub writes: Vec<WriteView>,
    /// Writes of the previous step, which produced `state(s)`; they match the registers' `changed` flags.
    pub prev_writes: Vec<WriteView>,
    pub event: Option<EventView>,
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

/// `GET /api/timeline`.
#[derive(Debug, Clone, Serialize)]
pub struct Timeline {
    pub steps: u32,
    pub depth: Vec<u32>,
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
