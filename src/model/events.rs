//! Syscall and signal events with their arguments, buffers, error codes and CPU exceptions,
//! as the viewer shows them (`GET /api/events`).

use iced_x86::Mnemonic;

use super::views::*;
use super::{LoadedTrace, asm};
use crate::trace::{Event, EventKind};

/// Registers carrying syscall arguments, in order.
const ARG_REGS: [&str; 6] = ["rdi", "rsi", "rdx", "r10", "r8", "r9"];
/// Most bytes of one buffer an event shows.
const MAX_BUFFER: u64 = 4096;
/// Longest string an event shows.
const MAX_STR: usize = 256;

/// How a syscall argument is shown.
#[derive(Debug, Clone, Copy)]
enum Kind {
    /// A plain value, rendered like any register.
    Int,
    /// A file descriptor, signed.
    Fd,
    Ptr,
    /// An unsigned byte count.
    Size,
    Hex,
    /// A signal number.
    Sig,
    /// A pointer to a NUL-terminated string the kernel reads.
    Str,
    /// A pointer to a buffer the kernel reads; its length is the value of this argument (0-based).
    In(usize),
    /// A pointer to a buffer the kernel fills; its length is the return value.
    Out,
}
use Kind::*;

/// A recorded syscall: where it ran, its name, the registers it was called with and what it returned.
struct Call<'a> {
    step: u32,
    name: &'a str,
    args: &'a [u64; 6],
    ret: Option<i64>,
}

/// Arguments of the syscalls the viewer knows, by name.
static SIGNATURES: &[(&str, &[(&str, Kind)])] = &[
    ("read", &[("fd", Fd), ("buf", Out), ("count", Size)]),
    ("write", &[("fd", Fd), ("buf", In(2)), ("count", Size)]),
    ("pread64", &[("fd", Fd), ("buf", Out), ("count", Size), ("offset", Int)]),
    ("pwrite64", &[("fd", Fd), ("buf", In(2)), ("count", Size), ("offset", Int)]),
    ("open", &[("pathname", Str), ("flags", Hex), ("mode", Hex)]),
    ("openat", &[("dirfd", Fd), ("pathname", Str), ("flags", Hex), ("mode", Hex)]),
    ("close", &[("fd", Fd)]),
    ("fstat", &[("fd", Fd), ("statbuf", Ptr)]),
    ("newfstatat", &[("dirfd", Fd), ("pathname", Str), ("statbuf", Ptr), ("flags", Hex)]),
    ("statx", &[("dirfd", Fd), ("pathname", Str), ("flags", Hex), ("mask", Hex), ("statxbuf", Ptr)]),
    ("lseek", &[("fd", Fd), ("offset", Int), ("whence", Int)]),
    ("mmap", &[("addr", Ptr), ("length", Size), ("prot", Hex), ("flags", Hex), ("fd", Fd), ("offset", Int)]),
    ("mprotect", &[("addr", Ptr), ("len", Size), ("prot", Hex)]),
    ("munmap", &[("addr", Ptr), ("length", Size)]),
    ("madvise", &[("addr", Ptr), ("length", Size), ("advice", Int)]),
    ("brk", &[("addr", Ptr)]),
    ("ioctl", &[("fd", Fd), ("request", Hex), ("arg", Ptr)]),
    ("fcntl", &[("fd", Fd), ("cmd", Int), ("arg", Hex)]),
    ("access", &[("pathname", Str), ("mode", Hex)]),
    ("faccessat", &[("dirfd", Fd), ("pathname", Str), ("mode", Hex)]),
    ("faccessat2", &[("dirfd", Fd), ("pathname", Str), ("mode", Hex), ("flags", Hex)]),
    ("readlink", &[("pathname", Str), ("buf", Out), ("bufsiz", Size)]),
    ("readlinkat", &[("dirfd", Fd), ("pathname", Str), ("buf", Out), ("bufsiz", Size)]),
    ("getcwd", &[("buf", Out), ("size", Size)]),
    ("dup", &[("oldfd", Fd)]),
    ("dup2", &[("oldfd", Fd), ("newfd", Fd)]),
    ("pipe", &[("pipefd", Ptr)]),
    ("pipe2", &[("pipefd", Ptr), ("flags", Hex)]),
    ("writev", &[("fd", Fd), ("iov", Ptr), ("iovcnt", Int)]),
    ("readv", &[("fd", Fd), ("iov", Ptr), ("iovcnt", Int)]),
    ("getpid", &[]),
    ("getppid", &[]),
    ("gettid", &[]),
    ("getuid", &[]),
    ("geteuid", &[]),
    ("getgid", &[]),
    ("getegid", &[]),
    ("sched_yield", &[]),
    ("rt_sigreturn", &[]),
    ("kill", &[("pid", Int), ("sig", Sig)]),
    ("tgkill", &[("tgid", Int), ("tid", Int), ("sig", Sig)]),
    ("rt_sigaction", &[("signum", Sig), ("act", Ptr), ("oldact", Ptr), ("sigsetsize", Size)]),
    ("rt_sigprocmask", &[("how", Int), ("set", Ptr), ("oldset", Ptr), ("sigsetsize", Size)]),
    ("exit", &[("status", Int)]),
    ("exit_group", &[("status", Int)]),
    ("uname", &[("buf", Ptr)]),
    ("getrandom", &[("buf", Out), ("buflen", Size), ("flags", Hex)]),
    ("arch_prctl", &[("code", Hex), ("addr", Ptr)]),
    ("set_tid_address", &[("tidptr", Ptr)]),
    ("set_robust_list", &[("head", Ptr), ("len", Size)]),
    ("rseq", &[("rseq", Ptr), ("rseq_len", Size), ("flags", Hex), ("sig", Hex)]),
    ("prlimit64", &[("pid", Int), ("resource", Int), ("new_limit", Ptr), ("old_limit", Ptr)]),
    ("clock_gettime", &[("clockid", Int), ("tp", Ptr)]),
    ("clock_nanosleep", &[("clockid", Int), ("flags", Hex), ("request", Ptr), ("remain", Ptr)]),
    ("nanosleep", &[("req", Ptr), ("rem", Ptr)]),
    ("futex", &[("uaddr", Ptr), ("op", Int), ("val", Int), ("timeout", Ptr), ("uaddr2", Ptr), ("val3", Int)]),
];

/// `si_code` values valid for any signal.
const GENERIC_CODES: &[(i32, &str, &str)] = &[
    (0, "SI_USER", "sent by kill()"),
    (0x80, "SI_KERNEL", "sent by the kernel"),
    (-1, "SI_QUEUE", "sent by sigqueue()"),
    (-6, "SI_TKILL", "sent by tkill() or tgkill(), e.g. raise()"),
];

/// `si_code` values specific to one signal.
fn signal_codes(signo: i32) -> &'static [(i32, &'static str, &'static str)] {
    match signo {
        libc::SIGSEGV => &[
            (1, "SEGV_MAPERR", "address not mapped to an object"),
            (2, "SEGV_ACCERR", "invalid permissions for a mapped object"),
            (3, "SEGV_BNDERR", "failed address bound checks"),
            (4, "SEGV_PKUERR", "protection key check failed"),
        ],
        libc::SIGBUS => &[
            (1, "BUS_ADRALN", "invalid address alignment"),
            (2, "BUS_ADRERR", "nonexistent physical address"),
            (3, "BUS_OBJERR", "object-specific hardware error"),
        ],
        libc::SIGFPE => &[
            (1, "FPE_INTDIV", "integer divide by zero"),
            (2, "FPE_INTOVF", "integer overflow"),
            (3, "FPE_FLTDIV", "floating-point divide by zero"),
            (4, "FPE_FLTOVF", "floating-point overflow"),
            (5, "FPE_FLTUND", "floating-point underflow"),
            (6, "FPE_FLTRES", "floating-point inexact result"),
            (7, "FPE_FLTINV", "floating-point invalid operation"),
            (8, "FPE_FLTSUB", "subscript out of range"),
        ],
        libc::SIGILL => &[
            (1, "ILL_ILLOPC", "illegal opcode"),
            (2, "ILL_ILLOPN", "illegal operand"),
            (3, "ILL_ILLADR", "illegal addressing mode"),
            (4, "ILL_ILLTRP", "illegal trap"),
            (5, "ILL_PRVOPC", "privileged opcode"),
            (6, "ILL_PRVREG", "privileged register"),
            (7, "ILL_COPROC", "coprocessor error"),
            (8, "ILL_BADSTK", "internal stack error"),
        ],
        libc::SIGTRAP => &[(1, "TRAP_BRKPT", "process breakpoint"), (2, "TRAP_TRACE", "process trace trap")],
        _ => &[],
    }
}

impl LoadedTrace {
    /// Every recorded syscall and signal, in order.
    pub fn events(&self) -> Vec<EventDetail> {
        self.trace.events.iter().map(|e| self.event_detail(e)).collect()
    }

    /// `e` with arguments named and rendered, the data buffers it moved, and its error or CPU exception.
    pub fn event_detail(&self, e: &Event) -> EventDetail {
        match e.kind {
            EventKind::Syscall { nr, args, ret } => self.syscall_detail(e.step, nr, &args, ret),
            EventKind::Signal { signo, code, addr } => {
                let (code_name, code_text) = match signal_codes(signo).iter().chain(GENERIC_CODES).find(|c| c.0 == code) {
                    Some(&(_, name, text)) => (Some(name.to_string()), Some(text.to_string())),
                    None => (None, None),
                };
                EventDetail::Signal {
                    step: e.step,
                    signal: crate::signals::name(signo),
                    code,
                    code_name,
                    code_text,
                    addr: self.render_value(addr, e.step),
                    cpu_exception: self.cpu_exception(e.step, signo, code).map(str::to_string),
                }
            }
        }
    }

    fn syscall_detail(&self, step: u32, nr: u64, raw: &[u64; 6], ret: Option<i64>) -> EventDetail {
        let name = crate::syscalls::name(nr);
        let params: Vec<(String, Kind)> = match SIGNATURES.iter().find(|s| s.0 == name) {
            Some((_, params)) => params.iter().map(|&(n, k)| (n.to_string(), k)).collect(),
            None => (0..6).map(|i| (format!("arg{i}"), Int)).collect(),
        };
        let args = params
            .iter()
            .zip(ARG_REGS.iter().zip(raw))
            .map(|((pname, kind), (&reg, &v))| self.arg_view(step, reg, pname, *kind, v))
            .collect();
        let call = Call { step, name: &name, args: raw, ret };
        let buffers = params.iter().zip(raw).filter_map(|((pname, kind), &v)| self.buffer(&call, pname, *kind, v)).collect();
        // `rt_sigreturn` returns nothing: %rax afterwards is the register the signal frame restored.
        let ret = ret.filter(|_| name != "rt_sigreturn");
        let errno = ret.filter(|r| (-4095..0).contains(r)).and_then(|r| crate::errno::lookup(-r));
        EventDetail::Syscall {
            step,
            nr,
            name,
            args,
            ret: ret.map(|r| self.render_value(r as u64, step + 1)),
            ret_raw: ret.map(|r| format!("0x{:x}", r as u64)),
            errno: errno.map(|(n, _)| n.to_string()),
            errno_text: errno.map(|(_, t)| t.to_string()),
            buffers,
        }
    }

    fn arg_view(&self, step: u32, reg: &'static str, name: &str, kind: Kind, v: u64) -> SyscallArgView {
        let ptr = |v: u64| if v == 0 { "NULL".to_string() } else { self.render_value(v, step) };
        let (value, note) = match kind {
            Int => (self.render_value(v, step), None),
            Fd => {
                let fd = v as i32;
                let note = match fd {
                    0 => Some("stdin"),
                    1 => Some("stdout"),
                    2 => Some("stderr"),
                    -100 => Some("AT_FDCWD"),
                    _ => None,
                };
                (fd.to_string(), note.map(str::to_string))
            }
            Size => (v.to_string(), None),
            Hex => (format!("0x{v:x}"), None),
            Sig => ((v as i32).to_string(), Some(crate::signals::name(v as i32))),
            Ptr | Str | In(_) | Out => (ptr(v), None),
        };
        SyscallArgView { reg, name: name.to_string(), value, raw: format!("0x{v:x}"), note }
    }

    /// The data behind pointer argument `pname` (value `ptr`), if it is a buffer.
    fn buffer(&self, call: &Call, pname: &str, kind: Kind, ptr: u64) -> Option<BufferView> {
        let Call { step, name: syscall, args, ret } = *call;
        let addr = self.render_value(ptr, step);
        let view = |dir, len: u64, requested: u64, at: u32| {
            let len = len.min(MAX_BUFFER);
            let bytes: Option<Vec<u8>> = self.read_mem(at, ptr, len as usize).into_iter().collect();
            BufferView {
                arg: pname.to_string(),
                dir,
                addr: addr.clone(),
                len: len as usize,
                text: bytes.map(|b| String::from_utf8_lossy(&b).into_owned()),
                truncated: len < requested,
            }
        };
        match kind {
            In(n) => {
                let count = args[n];
                // A short write: only what the kernel accepted was written.
                let accepted = ret.filter(|&r| matches!(syscall, "write" | "pwrite64") && r >= 0 && (r as u64) < count);
                Some(view("in", accepted.map_or(count, |r| r as u64), count, step))
            }
            Out => {
                let n = ret.filter(|&r| r > 0)? as u64;
                Some(view("out", n, n, step + 1))
            }
            Str => {
                let mut text = Vec::new();
                let (mut mapped, mut terminated) = (true, false);
                for b in self.read_mem(step, ptr, MAX_STR) {
                    match b {
                        Some(0) => {
                            terminated = true;
                            break;
                        }
                        Some(c) => text.push(c),
                        None => {
                            mapped = false;
                            break;
                        }
                    }
                }
                Some(BufferView {
                    arg: pname.to_string(),
                    dir: "in",
                    addr,
                    len: text.len(),
                    text: mapped.then(|| String::from_utf8_lossy(&text).into_owned()),
                    truncated: mapped && !terminated,
                })
            }
            _ => None,
        }
    }

    /// The x86 exception behind a fault step's signal.
    fn cpu_exception(&self, step: u32, signo: i32, code: i32) -> Option<&'static str> {
        if self.step_kind(step) != "fault" {
            return None;
        }
        let insn = &self.trace.insns[self.trace.steps[step as usize].insn as usize];
        let mnemonic = asm::decode(&insn.bytes, insn.addr).map(|d| d.mnemonic());
        match (signo, code) {
            (libc::SIGFPE, _) if matches!(mnemonic, Some(Mnemonic::Div | Mnemonic::Idiv)) => Some("#DE (divide error)"),
            (libc::SIGSEGV, 1 | 2) => Some("#PF (page fault)"),
            (libc::SIGSEGV, 0x80) => Some("#GP (general-protection fault)"),
            (libc::SIGILL, 2) => Some("#UD (invalid opcode)"),
            (libc::SIGTRAP, 0x80) if mnemonic == Some(Mnemonic::Int3) => Some("#BP (breakpoint)"),
            _ => None,
        }
    }
}
