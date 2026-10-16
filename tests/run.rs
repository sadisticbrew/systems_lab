mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use common::*;
use serde_json::Value;
use tempfile::TempDir;

/// A temp dir holding a copy of `tests/fixtures/<fixture>`.
fn project(fixture: &str) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join(fixture);
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(fixture), &src).unwrap();
    (dir, src)
}

/// `systemslab <args>` in `dir` with its XDG directories under `xdg` and `$CC` as given.
fn systemslab(dir: &Path, xdg: &Path, cc: Option<&str>, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_systemslab"));
    cmd.current_dir(dir).args(args).env("XDG_DATA_HOME", xdg.join("data")).env("XDG_CONFIG_HOME", xdg.join("config"));
    match cc {
        Some(cc) => cmd.env("CC", cc),
        None => cmd.env_remove("CC"),
    };
    cmd.output().unwrap()
}

fn run_ok(dir: &Path, args: &[&str]) -> Output {
    let o = systemslab(dir, &dir.join("xdg"), None, args);
    assert!(o.status.success(), "{args:?} failed ({:?}): {}", o.status.code(), stderr(&o));
    o
}

fn info_json(trace: &Path) -> Value {
    serde_json::from_str(&ok(&["info", "--json", p(trace)])).unwrap()
}

fn ended_with_exit_0(trace: &Path) {
    let text = ok(&["info", p(trace)]);
    let end = text.lines().find(|l| l.starts_with("end ")).expect("end line");
    assert_eq!(end.trim_end().strip_prefix("end").unwrap().trim(), "exit 0", "{text}");
}

#[test]
fn run_compiles_and_records_a_c_source_beside_it() {
    let (dir, _src) = project("fact.c");
    let o = run_ok(dir.path(), &["run", "fact.c", "--no-view"]);
    let err = stderr(&o);
    let bin = dir.path().join("fact");
    assert!(bin.is_file() && dir.path().join("fact.slt").is_file());
    assert!(err.contains(&format!("built {} (", bin.display())), "{err}");
    assert!(err.contains("-O0 -g -fcf-protection=none -o fact fact.c)"), "{err}");
    assert!(err.contains("recorded ") && err.contains("; ended: exit 0;"), "{err}");
    assert!(stdout(&o).contains("fact(5) = 120"), "the program ran: {}", stdout(&o));
    ended_with_exit_0(&dir.path().join("fact.slt"));
    // Sources are embedded under their short names, so line locators and the source panel work.
    assert!(run(&["stack", p(&dir.path().join("fact.slt")), "fact+0xc@3"]).status.success());
}

#[test]
fn run_passes_options_through() {
    let (dir, _src) = project("fact.c");
    let out = dir.path().join("o2.slt");
    let o = run_ok(dir.path(), &["run", "fact.c", "-O2", "--no-view", "-o", p(&out), "--max-steps", "40", "--from", "entry"]);
    assert!(stderr(&o).contains("-O2 -g -fcf-protection=none"), "{}", stderr(&o));
    assert!(out.is_file() && !dir.path().join("fact.slt").exists());
    let info = info_json(&out);
    assert_eq!(info["steps"], 40);
    assert_eq!(info["from"], "entry");
    let o = systemslab(dir.path(), &dir.path().join("xdg"), None, &["run", "fact.c", "-O9", "--no-view"]);
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));
    let o = run_ok(dir.path(), &["run", "fact.c", "--no-view", "-o", p(&out), "--", "ignored", "args"]);
    assert_eq!(info_json(&out)["argv"].as_array().unwrap().len(), 3, "{}", stderr(&o));
}

#[test]
fn run_builds_assembly_with_and_without_its_own_start() {
    let (dir, _src) = project("labels.s");
    let o = run_ok(dir.path(), &["run", "labels.s", "--no-view"]);
    assert!(stderr(&o).contains("-nostdlib -static -o labels labels.s"), "{}", stderr(&o));
    assert!(dir.path().join("labels").is_file());
    ended_with_exit_0(&dir.path().join("labels.slt"));
    assert_eq!(info_json(&dir.path().join("labels.slt"))["from"], "entry");

    // Without `_start:` the file links against libc, which supplies it and calls `main`.
    std::fs::write(dir.path().join("withmain.S"), "    .globl main\n    .text\nmain:\n    xor %eax, %eax\n    ret\n").unwrap();
    let o = run_ok(dir.path(), &["run", "withmain.S", "--no-view"]);
    assert!(!stderr(&o).contains("-nostdlib"), "{}", stderr(&o));
    ended_with_exit_0(&dir.path().join("withmain.slt"));
    assert_eq!(info_json(&dir.path().join("withmain.slt"))["from"], "main");
}

#[test]
fn run_rejects_other_sources_and_reports_compile_errors() {
    let dir = tempfile::tempdir().unwrap();
    let xdg = dir.path().join("xdg");
    for name in ["x.txt", "x.cpp", "x.rs", "noextension"] {
        let o = systemslab(dir.path(), &xdg, None, &["run", name, "--no-view"]);
        assert_eq!(o.status.code(), Some(2), "{name}");
        assert_eq!(
            stderr(&o).trim(),
            "systemslab run supports .c, .s and .S sources; compile other languages yourself and use systemslab record"
        );
    }
    let o = systemslab(dir.path(), &xdg, None, &["run", "missing.c", "--no-view"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("cannot read missing.c"), "{}", stderr(&o));

    std::fs::write(dir.path().join("bad.c"), "int main(void) { return 0 }\n").unwrap();
    let o = systemslab(dir.path(), &xdg, None, &["run", "bad.c", "--no-view"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("bad.c:1") && stderr(&o).contains("error"), "{}", stderr(&o));
    assert!(!dir.path().join("bad.slt").exists());

    let o = systemslab(dir.path(), &xdg, Some("/nonexistent/cc"), &["run", "bad.c", "--no-view"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("cannot run /nonexistent/cc"), "{}", stderr(&o));
    // An empty $CC means "unset".
    let (fact, _src) = project("fact.c");
    let o = systemslab(fact.path(), &xdg, Some(""), &["run", "fact.c", "--no-view"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stderr(&o).contains("(gcc -O0"), "{}", stderr(&o));
}

#[test]
fn try_lists_the_experiments_and_rejects_unknown_names() {
    let dir = tempfile::tempdir().unwrap();
    let o = run_ok(dir.path(), &["try"]);
    let out = stdout(&o);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "Experiments (systemslab try NAME):");
    let names: Vec<&str> = lines[1..].iter().map(|l| l.strip_prefix("  ").unwrap().split("  ").next().unwrap()).collect();
    assert_eq!(names, ["widths", "fact", "align", "saved"], "{out}");
    assert!(lines[1].starts_with("  widths  Register-width writes: what a 32-, 16- or 8-bit write does"), "{}", lines[1]);
    assert!(!dir.path().join("xdg").exists(), "listing wrote files");

    let o = systemslab(dir.path(), &dir.path().join("xdg"), None, &["try", "nosuch", "--no-view"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(stderr(&o).trim(), "unknown experiment 'nosuch'; run systemslab try to list them");
}

#[test]
fn try_copies_an_experiment_once_and_keeps_your_edits() {
    let dir = tempfile::tempdir().unwrap();
    let o = run_ok(dir.path(), &["try", "widths", "--no-view"]);
    let copy = dir.path().join("xdg/data/systemslab/experiments/widths");
    assert!(copy.join("widths.s").is_file() && copy.join("widths.guide.md").is_file());
    assert!(copy.join("widths").is_file() && copy.join("widths.slt").is_file());
    assert!(!stderr(&o).contains("using your copy"), "{}", stderr(&o));
    ended_with_exit_0(&copy.join("widths.slt"));
    assert!(ok(&["steps", p(&copy.join("widths.slt")), "--limit", "3"]).contains("0x11223344"));

    let edited = std::fs::read_to_string(copy.join("widths.s")).unwrap().replace("$0x11223344", "$0x55667788");
    std::fs::write(copy.join("widths.s"), &edited).unwrap();
    let o = run_ok(dir.path(), &["try", "widths", "--no-view"]);
    assert_eq!(std::fs::read_to_string(copy.join("widths.s")).unwrap(), edited, "the edit was overwritten");
    assert!(stderr(&o).contains(&format!("using your copy in {} (delete it to restore the original)", copy.display())), "{}", stderr(&o));
    let steps = ok(&["steps", p(&copy.join("widths.slt")), "--limit", "3"]);
    assert!(steps.contains("0x55667788") && !steps.contains("0x11223344"), "the edited source was rebuilt: {steps}");

    // Deleting the copy restores the original.
    std::fs::remove_dir_all(&copy).unwrap();
    run_ok(dir.path(), &["try", "widths", "--no-view"]);
    assert!(std::fs::read_to_string(copy.join("widths.s")).unwrap().contains("$0x11223344"));
}

#[test]
fn every_bundled_experiment_builds_and_records() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["widths", "fact", "align", "saved"] {
        run_ok(dir.path(), &["try", name, "--no-view"]);
        let copy = dir.path().join("xdg/data/systemslab/experiments").join(name);
        ended_with_exit_0(&copy.join(format!("{name}.slt")));
        assert!(copy.join(format!("{name}.guide.md")).is_file(), "{name} has a guide");
    }
    let fact = std::fs::read(dir.path().join("xdg/data/systemslab/experiments/fact/fact.c")).unwrap();
    assert_eq!(fact, std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fact.c")).unwrap());
}

fn wait_for(v: &Viewer, what: &str, done: impl Fn(&Value, &Value) -> bool) -> (Value, Value) {
    let end = Instant::now() + Duration::from_secs(40);
    loop {
        let (_, status) = http_get(v, "/api/status");
        let (_, info) = http_get(v, "/api/info");
        let (status, info): (Value, Value) = (serde_json::from_str(&status).unwrap(), serde_json::from_str(&info).unwrap());
        if done(&status, &info) {
            return (status, info);
        }
        assert!(Instant::now() < end, "timed out waiting for {what}; status {status}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn steps_of(source: &str) -> u64 {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("fact.c"), source).unwrap();
    run_ok(dir.path(), &["run", "fact.c", "--no-view"]);
    info_json(&dir.path().join("fact.slt"))["steps"].as_u64().unwrap()
}

#[test]
fn run_rebuilds_on_save_and_keeps_the_old_recording_when_the_build_fails() {
    let (dir, src) = project("fact.c");
    let original = std::fs::read_to_string(&src).unwrap();
    let v = start_viewer_with(&["run", p(&src), "--no-open", "--port", "0"]);
    let info = serde_json::from_str::<Value>(&http_get(&v, "/api/info").1).unwrap();
    assert_eq!(info["mode"], "run");
    assert_eq!(info["source_path"], src.to_str().unwrap());
    assert_eq!(info["trace_path"], std::fs::canonicalize(dir.path().join("fact.slt")).unwrap().to_str().unwrap());
    let steps5 = info["steps"].as_u64().unwrap();
    assert_eq!(steps5, steps_of(&original));

    // A save is rebuilt and re-recorded.
    let fact6 = original.replace("fact(5)", "fact(6)");
    std::fs::write(&src, &fact6).unwrap();
    let (status, info) = wait_for(&v, "generation 2", |s, _| s["generation"] == 2 && s["building"] == false);
    assert!(status["error"].is_null(), "{status}");
    let steps6 = info["steps"].as_u64().unwrap();
    assert!(steps6 > steps5, "fact(6) runs more steps: {steps5} -> {steps6}");
    assert_eq!(steps6, steps_of(&fact6));

    // A syntax error is reported with the compiler's message; the old recording keeps working.
    std::fs::write(&src, original.replace("return 0;", "return 0")).unwrap();
    let (status, info) = wait_for(&v, "a build error", |s, _| s["error"].is_string());
    assert_eq!(status["generation"], 2);
    assert_eq!(status["building"], false);
    assert!(status["error"].as_str().unwrap().contains("fact.c:") && status["error"].as_str().unwrap().contains("error"), "{status}");
    assert_eq!(info["steps"].as_u64().unwrap(), steps6);
    assert_eq!(http_get(&v, "/api/state/0").0, 200);

    // Fixing it clears the error. A second save made while the first rebuild runs is not lost.
    let fact7 = original.replace("fact(5)", "fact(7)");
    std::fs::write(&src, original.replace("fact(5)", "fact(8)")).unwrap();
    let end = Instant::now() + Duration::from_secs(20);
    while http_get(&v, "/api/status").1.contains("\"building\":false") && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::fs::write(&src, &fact7).unwrap();
    let steps7 = steps_of(&fact7);
    let (status, _) = wait_for(&v, "the last save to be served", |s, i| s["building"] == false && s["error"].is_null() && i["steps"].as_u64() == Some(steps7));
    assert!(status["generation"].as_u64().unwrap() >= 3, "{status}");
}

#[test]
fn no_watch_serves_one_recording_and_ignores_saves() {
    let (_dir, src) = project("fact.c");
    let v = start_viewer_with(&["run", p(&src), "--no-open", "--port", "0", "--no-watch"]);
    let steps = serde_json::from_str::<Value>(&http_get(&v, "/api/info").1).unwrap()["steps"].clone();
    std::fs::write(&src, std::fs::read_to_string(&src).unwrap().replace("fact(5)", "fact(6)")).unwrap();
    std::thread::sleep(Duration::from_millis(2500));
    let status: Value = serde_json::from_str(&http_get(&v, "/api/status").1).unwrap();
    assert_eq!(status, serde_json::json!({"generation": 1, "building": false, "error": null}));
    assert_eq!(serde_json::from_str::<Value>(&http_get(&v, "/api/info").1).unwrap()["steps"], steps);
}
