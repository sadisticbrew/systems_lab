//! x86-64 Linux signal names.

/// Name of signal `signo`, e.g. `SIGFPE`; `SIG<n>` for numbers without a name.
pub fn name(signo: i32) -> String {
    const NAMES: [&str; 31] = [
        "SIGHUP", "SIGINT", "SIGQUIT", "SIGILL", "SIGTRAP", "SIGABRT", "SIGBUS", "SIGFPE",
        "SIGKILL", "SIGUSR1", "SIGSEGV", "SIGUSR2", "SIGPIPE", "SIGALRM", "SIGTERM", "SIGSTKFLT",
        "SIGCHLD", "SIGCONT", "SIGSTOP", "SIGTSTP", "SIGTTIN", "SIGTTOU", "SIGURG", "SIGXCPU",
        "SIGXFSZ", "SIGVTALRM", "SIGPROF", "SIGWINCH", "SIGIO", "SIGPWR", "SIGSYS",
    ];
    match usize::try_from(signo - 1).ok().and_then(|i| NAMES.get(i)) {
        Some(n) => (*n).to_string(),
        None => format!("SIG{signo}"),
    }
}
