//! Spawning and driving the tracee through raw ptrace calls.

use std::ffi::{CStr, c_void};
use std::os::unix::process::CommandExt;
use std::process::Command;

use anyhow::{Result, bail};

use crate::exit;
use crate::trace::RegFile;

/// How the tracee changed state after a wait.
#[derive(Debug, Clone, Copy)]
pub enum Stop {
    Exited(i32),
    Signaled(i32),
    /// `event` is the `PTRACE_EVENT_*` number, 0 for a plain signal stop.
    Stopped { sig: i32, event: i32 },
}

pub struct Tracee {
    pub pid: libc::pid_t,
    alive: bool,
}

fn os_reason(err: &std::io::Error) -> String {
    match err.raw_os_error() {
        Some(code) => unsafe { CStr::from_ptr(libc::strerror(code)) }.to_string_lossy().into_owned(),
        None => err.to_string(),
    }
}

fn last_os_error() -> std::io::Error {
    std::io::Error::last_os_error()
}

impl Tracee {
    /// Forks and execs `program` under PTRACE_TRACEME, returning at the exec
    /// stop together with whether ASLR was disabled.
    pub fn spawn(program: &str, args: &[String]) -> Result<(Tracee, bool)> {
        let mut cmd = Command::new(program);
        cmd.args(args);
        let tunables = match std::env::var("GLIBC_TUNABLES") {
            Ok(v) if !v.is_empty() => format!("{v}:glibc.pthread.rseq=0"),
            _ => "glibc.pthread.rseq=0".to_string(),
        };
        cmd.env("GLIBC_TUNABLES", tunables);
        unsafe {
            cmd.pre_exec(|| {
                if libc::ptrace(libc::PTRACE_TRACEME, 0, 0usize, 0usize) != 0 {
                    return Err(last_os_error());
                }
                // Failure is tolerated; the parent checks /proc/<pid>/personality.
                let cur = libc::personality(0xffff_ffff);
                if cur != -1 {
                    libc::personality((cur as libc::c_ulong) | libc::ADDR_NO_RANDOMIZE as libc::c_ulong);
                }
                Ok(())
            });
        }
        let child = cmd
            .spawn()
            .map_err(|e| exit::usage(format!("cannot start {program}: {}", os_reason(&e))))?;
        let mut t = Tracee { pid: child.id() as libc::pid_t, alive: true };
        drop(child);
        match t.wait()? {
            Stop::Stopped { .. } => {}
            Stop::Exited(code) => {
                t.alive = false;
                return Err(exit::usage(format!(
                    "cannot start {program}: the process exited with status {code} before it was traced"
                )));
            }
            Stop::Signaled(sig) => {
                t.alive = false;
                return Err(exit::usage(format!(
                    "cannot start {program}: the process was killed by {} before it was traced",
                    crate::signals::name(sig)
                )));
            }
        }
        let opts = libc::PTRACE_O_EXITKILL
            | libc::PTRACE_O_TRACECLONE
            | libc::PTRACE_O_TRACEFORK
            | libc::PTRACE_O_TRACEVFORK
            | libc::PTRACE_O_TRACEEXEC;
        if unsafe { libc::ptrace(libc::PTRACE_SETOPTIONS, t.pid, 0usize, opts as usize) } != 0 {
            bail!("PTRACE_SETOPTIONS failed: {}", last_os_error());
        }
        let aslr_disabled = std::fs::read_to_string(format!("/proc/{}/personality", t.pid))
            .ok()
            .and_then(|s| u32::from_str_radix(s.trim(), 16).ok())
            .is_some_and(|p| p & libc::ADDR_NO_RANDOMIZE as u32 != 0);
        Ok((t, aslr_disabled))
    }

    pub fn wait(&self) -> Result<Stop> {
        let mut status: libc::c_int = 0;
        loop {
            let r = unsafe { libc::waitpid(self.pid, &mut status, libc::__WALL) };
            if r == self.pid {
                break;
            }
            let err = last_os_error();
            if err.raw_os_error() != Some(libc::EINTR) {
                bail!("waitpid failed: {err}");
            }
        }
        Ok(if libc::WIFEXITED(status) {
            Stop::Exited(libc::WEXITSTATUS(status))
        } else if libc::WIFSIGNALED(status) {
            Stop::Signaled(libc::WTERMSIG(status))
        } else {
            Stop::Stopped { sig: libc::WSTOPSIG(status), event: status >> 16 }
        })
    }

    /// Records that the tracee is gone so `Drop` does not signal a recycled pid.
    pub fn mark_dead(&mut self) {
        self.alive = false;
    }

    pub fn singlestep(&self, sig: i32) -> Result<()> {
        if unsafe { libc::ptrace(libc::PTRACE_SINGLESTEP, self.pid, 0usize, sig as usize) } != 0 {
            bail!("PTRACE_SINGLESTEP failed: {}", last_os_error());
        }
        Ok(())
    }

    pub fn cont(&self, sig: i32) -> Result<()> {
        if unsafe { libc::ptrace(libc::PTRACE_CONT, self.pid, 0usize, sig as usize) } != 0 {
            bail!("PTRACE_CONT failed: {}", last_os_error());
        }
        Ok(())
    }

    pub fn kill(&mut self) {
        if self.alive {
            unsafe { libc::kill(self.pid, libc::SIGKILL) };
            let mut status = 0;
            unsafe { libc::waitpid(self.pid, &mut status, libc::__WALL) };
            self.alive = false;
        }
    }

    /// Reads all registers (GPRs, rip, rflags, bases, xmm0-15, mxcsr) into `out`.
    pub fn regs(&self, out: &mut RegFile) -> Result<()> {
        let mut u: libc::user_regs_struct = unsafe { std::mem::zeroed() };
        let mut f: libc::user_fpregs_struct = unsafe { std::mem::zeroed() };
        if unsafe { libc::ptrace(libc::PTRACE_GETREGS, self.pid, 0usize, &mut u as *mut _ as *mut c_void) } != 0 {
            bail!("PTRACE_GETREGS failed: {}", last_os_error());
        }
        if unsafe { libc::ptrace(libc::PTRACE_GETFPREGS, self.pid, 0usize, &mut f as *mut _ as *mut c_void) } != 0 {
            bail!("PTRACE_GETFPREGS failed: {}", last_os_error());
        }
        out.gpr = [
            u.rax, u.rbx, u.rcx, u.rdx, u.rsi, u.rdi, u.rbp, u.rsp, u.r8, u.r9, u.r10, u.r11, u.r12,
            u.r13, u.r14, u.r15,
        ];
        out.rip = u.rip;
        out.rflags = u.eflags;
        out.fs_base = u.fs_base;
        out.gs_base = u.gs_base;
        for (i, x) in out.xmm.iter_mut().enumerate() {
            let w = &f.xmm_space[i * 4..i * 4 + 4];
            *x = w[0] as u128 | (w[1] as u128) << 32 | (w[2] as u128) << 64 | (w[3] as u128) << 96;
        }
        out.mxcsr = f.mxcsr;
        Ok(())
    }

    /// Sets rip (used to back up over the start breakpoint).
    pub fn set_rip(&self, rip: u64) -> Result<()> {
        let mut u: libc::user_regs_struct = unsafe { std::mem::zeroed() };
        if unsafe { libc::ptrace(libc::PTRACE_GETREGS, self.pid, 0usize, &mut u as *mut _ as *mut c_void) } != 0 {
            bail!("PTRACE_GETREGS failed: {}", last_os_error());
        }
        u.rip = rip;
        if unsafe { libc::ptrace(libc::PTRACE_SETREGS, self.pid, 0usize, &mut u as *mut _ as *mut c_void) } != 0 {
            bail!("PTRACE_SETREGS failed: {}", last_os_error());
        }
        Ok(())
    }

    pub fn rip(&self) -> Result<u64> {
        let mut u: libc::user_regs_struct = unsafe { std::mem::zeroed() };
        if unsafe { libc::ptrace(libc::PTRACE_GETREGS, self.pid, 0usize, &mut u as *mut _ as *mut c_void) } != 0 {
            bail!("PTRACE_GETREGS failed: {}", last_os_error());
        }
        Ok(u.rip)
    }

    /// `(signo, si_code, si_addr)` of the signal that stopped the tracee.
    pub fn siginfo(&self) -> Result<(i32, i32, u64)> {
        let mut si: libc::siginfo_t = unsafe { std::mem::zeroed() };
        if unsafe { libc::ptrace(libc::PTRACE_GETSIGINFO, self.pid, 0usize, &mut si as *mut _ as *mut c_void) } != 0 {
            bail!("PTRACE_GETSIGINFO failed: {}", last_os_error());
        }
        Ok((si.si_signo, si.si_code, unsafe { si.si_addr() } as u64))
    }

    /// Reads as many bytes as are readable from the start of the range; returns the count.
    pub fn read(&self, addr: u64, buf: &mut [u8]) -> usize {
        if buf.is_empty() {
            return 0;
        }
        let local = libc::iovec { iov_base: buf.as_mut_ptr() as *mut c_void, iov_len: buf.len() };
        let remote = libc::iovec { iov_base: addr as *mut c_void, iov_len: buf.len() };
        let n = unsafe { libc::process_vm_readv(self.pid, &local, 1, &remote, 1, 0) };
        if n < 0 { 0 } else { n as usize }
    }

    pub fn peek(&self, addr: u64) -> Result<u64> {
        unsafe { *libc::__errno_location() = 0 };
        let v = unsafe { libc::ptrace(libc::PTRACE_PEEKDATA, self.pid, addr as usize, 0usize) };
        if v == -1 && unsafe { *libc::__errno_location() } != 0 {
            bail!("PTRACE_PEEKDATA at {addr:#x} failed: {}", last_os_error());
        }
        Ok(v as u64)
    }

    pub fn poke(&self, addr: u64, word: u64) -> Result<()> {
        if unsafe { libc::ptrace(libc::PTRACE_POKEDATA, self.pid, addr as usize, word as usize) } != 0 {
            bail!("PTRACE_POKEDATA at {addr:#x} failed: {}", last_os_error());
        }
        Ok(())
    }
}

impl Drop for Tracee {
    fn drop(&mut self) {
        self.kill();
    }
}
