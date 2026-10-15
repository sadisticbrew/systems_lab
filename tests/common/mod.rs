//! Shared helpers: compile fixtures, run the built binary.
#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

/// A compiled fixture; the directory is removed on drop.
pub struct Built {
    pub dir: TempDir,
    pub exe: PathBuf,
}

pub const FACT_FLAGS: &[&str] = &["-O0", "-g", "-fcf-protection=none"];
pub const ASM_FLAGS: &[&str] = &["-nostdlib", "-static"];

/// Compiles `tests/fixtures/<fixture>` with gcc and `flags` into a fresh temp dir.
pub fn build(fixture: &str, flags: &[&str]) -> Built {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(fixture);
    // Compile from inside the temp dir with a relative name so DWARF paths are stable.
    let local = dir.path().join(fixture);
    std::fs::copy(&src, &local).expect("copy fixture");
    let exe = dir.path().join("prog");
    let out = Command::new("gcc")
        .current_dir(dir.path())
        .args(flags)
        .arg("-o")
        .arg("prog")
        .arg(fixture)
        .output()
        .expect("gcc must be on PATH");
    assert!(out.status.success(), "gcc failed: {}", String::from_utf8_lossy(&out.stderr));
    Built { dir, exe }
}

pub fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_systemslab")).args(args).output().expect("run systemslab")
}

pub fn run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_systemslab")).current_dir(dir).args(args).output().expect("run systemslab")
}

pub fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

pub fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Runs `args`, asserts exit 0 and returns stdout.
pub fn ok(args: &[&str]) -> String {
    let o = run(args);
    assert!(o.status.success(), "{args:?} failed ({:?}): {}", o.status.code(), stderr(&o));
    stdout(&o)
}

/// Records `built` with `--strict --quiet` plus `extra`, returning the trace path.
pub fn record(built: &Built, extra: &[&str]) -> PathBuf {
    record_named(built, "trace.slt", extra)
}

pub fn record_named(built: &Built, name: &str, extra: &[&str]) -> PathBuf {
    let out = built.dir.path().join(name);
    let mut args = vec!["record", "--strict", "--quiet", "-o", out.to_str().unwrap()];
    args.extend_from_slice(extra);
    args.push("--");
    args.push(built.exe.to_str().unwrap());
    let o = run(&args);
    assert!(o.status.success(), "record failed ({:?}): {}", o.status.code(), stderr(&o));
    out
}

pub fn p(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// Runs `dot -T<format>` on `input`, returning the output.
pub fn dot(format: &str, input: &str) -> Output {
    let mut child = Command::new("dot")
        .arg(format!("-T{format}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("dot must be on PATH");
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

/// Parses one JSON value per non-empty line.
pub fn json_lines(text: &str) -> Vec<serde_json::Value> {
    text.lines().filter(|l| !l.is_empty()).map(|l| serde_json::from_str(l).expect("json line")).collect()
}
