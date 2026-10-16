//! `systemslab run`: compile a C or assembly source, record it, and open the viewer, which
//! rebuilds and re-records whenever the source is saved.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::Result;

use crate::server::{self, ServeOptions, Stamp};
use crate::trace::{self, Trace};
use crate::{exit, record};

/// How a source is compiled and recorded; the viewer repeats this on every save.
#[derive(Debug, Clone)]
pub struct Config {
    /// Optimization level: `0 1 2 3 s g`.
    pub opt: String,
    /// Where recording starts (see `record --from`).
    pub from: String,
    pub max_steps: u64,
    /// Arguments of the recorded program.
    pub args: Vec<String>,
    /// Trace file to write.
    pub trace: PathBuf,
}

/// Options of the `run` command.
pub struct RunOptions {
    pub source: PathBuf,
    pub opt: String,
    pub from: String,
    pub max_steps: u64,
    /// Trace file; default `<source dir>/<stem>.slt`.
    pub output: Option<PathBuf>,
    /// Guide file; default `<source dir>/<stem>.guide.md` when it exists.
    pub guide: Option<PathBuf>,
    pub port: u16,
    pub no_open: bool,
    pub no_view: bool,
    pub no_watch: bool,
    pub args: Vec<String>,
}

impl RunOptions {
    /// The defaults of `systemslab run SOURCE`: `-O0`, record from `main`, view and watch.
    pub fn new(source: PathBuf) -> RunOptions {
        RunOptions {
            source,
            opt: "0".into(),
            from: "auto".into(),
            max_steps: 2_000_000,
            output: None,
            guide: None,
            port: 0,
            no_open: false,
            no_view: false,
            no_watch: false,
            args: Vec::new(),
        }
    }
}

/// The compiler rejected the source. `output` is its diagnostics.
#[derive(Debug)]
pub struct CompileError {
    pub output: String,
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.output)
    }
}

impl std::error::Error for CompileError {}

/// The result of one compile-and-record round.
pub struct Built {
    pub trace: Trace,
    /// The compiler command that was run, for display.
    pub command: String,
}

/// The executable that `source` compiles to: `<dir>/<stem>`.
fn binary_of(source: &Path) -> PathBuf {
    source.with_extension("")
}

/// Whether the assembly `text` defines its own `_start`, so it links without libc.
fn defines_start(text: &str) -> bool {
    text.lines().any(|l| l.trim_start().starts_with("_start:"))
}

/// Quotes `s` for a POSIX shell when it contains anything but plain path characters.
fn shell_word(s: &OsStr) -> String {
    let s = s.to_string_lossy();
    let plain = |c: char| c.is_ascii_alphanumeric() || "_./=+,:@%-".contains(c);
    if !s.is_empty() && s.chars().all(plain) { s.into_owned() } else { format!("'{}'", s.replace('\'', r"'\''")) }
}

/// Compiles `source` into `binary_of(source)` with debug info, running the compiler in the
/// source's directory so the recorded file names are short and stable. Returns the command line.
fn compile(source: &Path, cfg: &Config) -> Result<String> {
    let dir = source.parent().unwrap_or(Path::new("."));
    let file = source.file_name().unwrap_or_default();
    let bin = binary_of(source);
    let cc = std::env::var_os("CC").filter(|c| !c.is_empty()).unwrap_or_else(|| OsString::from("gcc"));
    let mut args: Vec<OsString> = vec![format!("-O{}", cfg.opt).into(), "-g".into(), "-fcf-protection=none".into()];
    if source.extension().is_some_and(|e| e == "s" || e == "S") {
        let text = std::fs::read(source).map_err(|e| exit::usage(format!("cannot read {}: {e}", source.display())))?;
        if defines_start(&String::from_utf8_lossy(&text)) {
            args.extend(["-nostdlib".into(), "-static".into()]);
        }
    }
    args.extend(["-o".into(), bin.file_name().unwrap_or_default().to_owned(), file.to_owned()]);

    let out = Command::new(&cc).current_dir(dir).args(&args).stdin(Stdio::null()).output().map_err(|e| {
        let hint = if e.kind() == std::io::ErrorKind::NotFound { " (is it installed? set $CC to use another compiler)" } else { "" };
        exit::usage(format!("cannot run {}: {e}{hint}", cc.to_string_lossy()))
    })?;
    if !out.status.success() {
        let mut output = String::from_utf8_lossy(&out.stdout).into_owned();
        output.push_str(&String::from_utf8_lossy(&out.stderr));
        let mut output = output.trim_end().to_string();
        if output.is_empty() {
            output = format!("{} failed: {}", cc.to_string_lossy(), out.status);
        }
        return Err(CompileError { output }.into());
    }

    let mut command = shell_word(&cc);
    for a in &args {
        command.push(' ');
        command.push_str(&shell_word(a));
    }
    if std::env::current_dir().ok().as_deref() != Some(dir) {
        command = format!("cd {} && {command}", shell_word(dir.as_os_str()));
    }
    Ok(command)
}

/// Compiles `source`, records the program and writes `cfg.trace`. A compiler failure is a
/// [`CompileError`]; recording errors keep their exit codes.
pub fn build_and_record(source: &Path, cfg: &Config) -> Result<Built> {
    let command = compile(source, cfg)?;
    let rec = record::record(&record::Options {
        from: cfg.from.clone(),
        max_steps: cfg.max_steps,
        strict: false,
        quiet: true,
        output: cfg.trace.clone(),
        program: binary_of(source).to_string_lossy().into_owned(),
        args: cfg.args.clone(),
    })?;
    trace::save(&cfg.trace, &rec.trace).map_err(|e| exit::usage(format!("{e:#}")))?;
    Ok(Built { trace: rec.trace, command })
}

/// `systemslab run`.
pub fn run(o: RunOptions) -> Result<u8> {
    if !o.source.extension().is_some_and(|e| e == "c" || e == "s" || e == "S") {
        return Err(exit::usage(
            "systemslab run supports .c, .s and .S sources; compile other languages yourself and use systemslab record",
        ));
    }
    let source = std::path::absolute(&o.source)?;
    if !source.is_file() {
        return Err(exit::usage(format!("cannot read {}: no such file", o.source.display())));
    }
    if let Some(g) = o.guide.as_ref().filter(|g| !g.is_file()) {
        return Err(exit::usage(format!("cannot read guide {}", g.display())));
    }
    let trace = match o.output {
        Some(t) => std::path::absolute(t)?,
        None => source.with_extension("slt"),
    };
    let guide = o.guide.or_else(|| Some(source.with_extension("guide.md")).filter(|g| g.is_file()));
    let cfg = Config { opt: o.opt, from: o.from, max_steps: o.max_steps, args: o.args, trace };

    let built_from = Stamp::of(&source);
    let built = build_and_record(&source, &cfg).map_err(|e| match e.downcast_ref::<CompileError>() {
        Some(c) => exit::usage(&c.output),
        None => e,
    })?;
    eprintln!("built {} ({})", binary_of(&source).display(), built.command);
    record::print_summary(&built.trace, &cfg.trace);
    drop(built);
    if o.no_view {
        return Ok(0);
    }
    server::serve(ServeOptions {
        trace_path: cfg.trace.clone(),
        guide,
        at: None,
        port: o.port,
        no_open: o.no_open,
        mode: server::Mode::Run { source, cfg, watch: !o.no_watch, built_from },
    })
}
