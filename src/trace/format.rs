//! On-disk trace format (`.slt`), shared by the recorder and every reader.
//!
//! File = `MAGIC`, u32 LE `FORMAT_VERSION`, then `zstd(level 3, postcard(Trace))`.
//!
//! Step/state/seq model: step `i` is one single-step resume of the tracee;
//! `state(i)` is the machine state before step `i` runs. A delta or write with
//! `seq = i + 1` was produced by step `i`; the initial snapshot has `seq = 0`.
//! `state(S)` contains exactly the records with `seq <= S`.

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const MAGIC: &[u8; 8] = b"SLTRACE\0";
pub const FORMAT_VERSION: u32 = 1;
/// `reg_checkpoints[j]` is `state(j * CHECKPOINT_INTERVAL)`.
pub const CHECKPOINT_INTERVAL: u32 = 1024;
/// Every `MemWrite` lies within one page of this size.
pub const PAGE_SIZE: u64 = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
    pub meta: Meta,
    pub exe: Exe,
    pub libs: Vec<LibSyms>,
    pub sources: Vec<Source>,
    /// Sorted by seq; the first has seq 0.
    pub maps: Vec<MapsSnapshot>,
    /// Unique by addr.
    pub insns: Vec<Insn>,
    pub steps: Vec<Step>,
    pub reg_checkpoints: Vec<RegFile>,
    /// Sorted by seq.
    pub reg_deltas: Vec<RegDelta>,
    /// Sorted by seq; each record lies within one `PAGE_SIZE` page.
    pub writes: Vec<MemWrite>,
    /// Sorted by step.
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    pub tool: String,
    pub program: String,
    pub exe_path: String,
    pub argv: Vec<String>,
    pub from: String,
    pub aslr_disabled: bool,
    pub rseq_disabled: bool,
    pub max_steps: u64,
    pub end: End,
    pub unattributed_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum End {
    Exit { code: i32 },
    Signal { signo: i32 },
    MaxSteps,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exe {
    pub path: String,
    pub bytes: Vec<u8>,
    /// Runtime address = bias + link-time address.
    pub bias: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibSyms {
    pub path: String,
    pub file_name: String,
    /// Runtime address = bias + `Sym::addr`.
    pub bias: u64,
    pub syms: Vec<Sym>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sym {
    pub name: String,
    /// Link-time address.
    pub addr: u64,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub path: String,
    pub display: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapsSnapshot {
    /// In effect for every state `S >= seq` until the next snapshot.
    pub seq: u32,
    pub regions: Vec<Region>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region {
    pub start: u64,
    pub end: u64,
    /// As in /proc/pid/maps, e.g. "r-xp".
    pub perms: String,
    pub offset: u64,
    /// Path or pseudo-name ("[stack]", "[heap]"), empty for anonymous.
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Insn {
    pub addr: u64,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// Index into `Trace::insns`: the instruction at `state(i).rip`.
    pub insn: u32,
    pub kind: StepKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepKind {
    /// A normal instruction.
    Instr,
    /// The instruction raised a signal and did not complete.
    Fault { signo: i32 },
    /// This resume delivered a pending signal.
    Deliver { signo: i32 },
    /// The process exited during this step.
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RegFile {
    /// rax rbx rcx rdx rsi rdi rbp rsp r8 r9 r10 r11 r12 r13 r14 r15
    pub gpr: [u64; 16],
    pub rip: u64,
    pub rflags: u64,
    pub fs_base: u64,
    pub gs_base: u64,
    pub xmm: [u128; 16],
    pub mxcsr: u32,
}

/// Register ids used by `RegDelta::reg`.
pub mod reg {
    pub const RAX: u8 = 0;
    pub const RBX: u8 = 1;
    pub const RCX: u8 = 2;
    pub const RDX: u8 = 3;
    pub const RSI: u8 = 4;
    pub const RDI: u8 = 5;
    pub const RBP: u8 = 6;
    pub const RSP: u8 = 7;
    pub const R8: u8 = 8;
    pub const R12: u8 = 12;
    pub const R13: u8 = 13;
    pub const R14: u8 = 14;
    pub const R15: u8 = 15;
    pub const RIP: u8 = 16;
    pub const RFLAGS: u8 = 17;
    pub const FS_BASE: u8 = 18;
    pub const GS_BASE: u8 = 19;
    /// xmm0..=xmm15 are `XMM0 + n`.
    pub const XMM0: u8 = 20;
    pub const MXCSR: u8 = 36;
    pub const COUNT: u8 = 37;

    pub const GPR_NAMES: [&str; 16] = [
        "rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rbp", "rsp", "r8", "r9", "r10", "r11", "r12",
        "r13", "r14", "r15",
    ];

    /// Name of a register id, e.g. `rax`, `rflags`, `xmm3`.
    pub fn name(id: u8) -> String {
        match id {
            0..=15 => GPR_NAMES[id as usize].to_string(),
            RIP => "rip".into(),
            RFLAGS => "rflags".into(),
            FS_BASE => "fs_base".into(),
            GS_BASE => "gs_base".into(),
            20..=35 => format!("xmm{}", id - XMM0),
            MXCSR => "mxcsr".into(),
            _ => format!("reg{id}"),
        }
    }

    /// Inverse of `name`.
    pub fn by_name(name: &str) -> Option<u8> {
        (0..COUNT).find(|&id| self::name(id) == name)
    }
}

impl RegFile {
    pub fn get(&self, id: u8) -> u128 {
        match id {
            0..=15 => self.gpr[id as usize] as u128,
            reg::RIP => self.rip as u128,
            reg::RFLAGS => self.rflags as u128,
            reg::FS_BASE => self.fs_base as u128,
            reg::GS_BASE => self.gs_base as u128,
            20..=35 => self.xmm[(id - reg::XMM0) as usize],
            reg::MXCSR => self.mxcsr as u128,
            _ => panic!("unknown register id {id}"),
        }
    }

    pub fn set(&mut self, id: u8, value: u128) {
        match id {
            0..=15 => self.gpr[id as usize] = value as u64,
            reg::RIP => self.rip = value as u64,
            reg::RFLAGS => self.rflags = value as u64,
            reg::FS_BASE => self.fs_base = value as u64,
            reg::GS_BASE => self.gs_base = value as u64,
            20..=35 => self.xmm[(id - reg::XMM0) as usize] = value,
            reg::MXCSR => self.mxcsr = value as u32,
            _ => panic!("unknown register id {id}"),
        }
    }

    pub fn rsp(&self) -> u64 {
        self.gpr[reg::RSP as usize]
    }

    pub fn rbp(&self) -> u64 {
        self.gpr[reg::RBP as usize]
    }

    /// Ids whose values differ between `self` and `other`, ascending.
    pub fn diff(&self, other: &RegFile) -> Vec<u8> {
        (0..reg::COUNT).filter(|&id| self.get(id) != other.get(id)).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegDelta {
    pub seq: u32,
    pub reg: u8,
    pub value: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemWrite {
    pub seq: u32,
    pub addr: u64,
    pub data: Vec<u8>,
    pub kind: WriteKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WriteKind {
    /// Initial contents (seq 0) or newly mapped memory.
    Snapshot,
    /// Written by the instruction of step `seq - 1`.
    Instr,
    /// Written by the kernel during a syscall, fault, or signal delivery.
    Kernel,
    /// Found by a periodic full diff; no instruction or syscall accounts for it.
    Unattributed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub step: u32,
    pub kind: EventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    Syscall { nr: u64, args: [u64; 6], ret: Option<i64> },
    Signal { signo: i32, code: i32, addr: u64 },
}

pub fn save(path: &Path, trace: &Trace) -> Result<()> {
    let body = postcard::to_stdvec(trace).context("serializing trace")?;
    let packed = zstd::encode_all(&body[..], 3).context("compressing trace")?;
    let mut out = Vec::with_capacity(12 + packed.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&packed);
    std::fs::write(path, out).with_context(|| format!("writing {}", path.display()))
}

pub fn load(path: &Path) -> Result<Trace> {
    let raw = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if raw.len() < 12 || &raw[..8] != MAGIC {
        bail!("{} is not a SystemsLab trace", path.display());
    }
    let version = u32::from_le_bytes(raw[8..12].try_into().unwrap());
    if version != FORMAT_VERSION {
        bail!("trace format v{version}, this systemslab reads v{FORMAT_VERSION}; re-record");
    }
    let body = zstd::decode_all(&raw[12..]).context("decompressing trace")?;
    postcard::from_bytes(&body).with_context(|| format!("decoding {}", path.display()))
}
