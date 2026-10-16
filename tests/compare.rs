mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::*;
use serde_json::Value;

const O2_FLAGS: &[&str] = &["-O2", "-g", "-fcf-protection=none"];

/// Compiles C `source` (as `p.c`) with the fixture flags.
fn build_c(source: &str) -> Built {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("p.c"), source).unwrap();
    let out = Command::new("gcc").current_dir(dir.path()).args(FACT_FLAGS).args(["-o", "prog", "p.c"]).output().expect("gcc must be on PATH");
    assert!(out.status.success(), "gcc failed: {}", stderr(&out));
    let exe = dir.path().join("prog");
    Built { dir, exe }
}

fn compare_json(a: &Path, b: &Path) -> Value {
    serde_json::from_str(&ok(&["compare", "--json", p(a), p(b)])).expect("one JSON object")
}

fn row<'a>(v: &'a Value, name: &str) -> &'a Value {
    v["functions"].as_array().unwrap().iter().find(|f| f["name"] == name).unwrap_or_else(|| panic!("no row {name}: {v}"))
}

/// Resolves `locator` in `trace` through `steps --from`, returning the step it names.
fn step_of(trace: &Path, locator: &str) -> u64 {
    let text = ok(&["steps", "--json", p(trace), "--from", locator, "--limit", "1"]);
    json_lines(&text)[0]["step"].as_u64().unwrap()
}

fn info_steps(trace: &Path) -> u64 {
    serde_json::from_str::<Value>(&ok(&["info", "--json", p(trace)])).unwrap()["steps"].as_u64().unwrap()
}

fn fact_pair() -> (Built, PathBuf, Built, PathBuf) {
    let o0 = build("fact.c", FACT_FLAGS);
    let t0 = record(&o0, &[]);
    let o2 = build("fact.c", O2_FLAGS);
    let t2 = record(&o2, &[]);
    (o0, t0, o2, t2)
}

#[test]
fn fact_o0_against_o2_json() {
    let (_o0, t0, _o2, t2) = fact_pair();
    let v = compare_json(&t0, &t2);

    assert_eq!(v["a"]["trace"], p(&t0));
    assert_eq!(v["b"]["trace"], p(&t2));
    assert_eq!(v["a"]["steps"].as_u64().unwrap(), info_steps(&t0));
    assert_eq!(v["b"]["steps"].as_u64().unwrap(), info_steps(&t2));
    assert_eq!(v["a"]["end"], "exit 0");
    assert_eq!(v["b"]["end"], "exit 0");
    assert_eq!(v["a"]["output_bytes"], "fact(5) = 120\n".len());
    assert_eq!(v["b"]["output_bytes"], "fact(5) = 120\n".len());
    assert_eq!(v["output_identical"], true);
    assert_eq!(v["syscalls_identical"], true);
    assert_eq!(v["a"]["syscalls"], v["b"]["syscalls"]);

    // fact(5) recurses five times at -O0.
    assert_eq!(row(&v, "fact")["a_calls"], 5);
    assert!(row(&v, "fact")["a_steps"].as_u64().unwrap() > 0);
    assert_eq!(row(&v, "main")["a_calls"], 1);
    assert_eq!(row(&v, "main")["b_calls"], 1);

    // Every step of a trace is in exactly one function row or in `other_steps`.
    let sum = |side: &str| -> u64 { v["functions"].as_array().unwrap().iter().map(|f| f[format!("{side}_steps")].as_u64().unwrap()).sum() };
    assert_eq!(sum("a") + v["other_steps"]["a"].as_u64().unwrap(), info_steps(&t0));
    assert_eq!(sum("b") + v["other_steps"]["b"].as_u64().unwrap(), info_steps(&t2));

    // Rows are sorted by their larger step count and none is empty.
    let rows = v["functions"].as_array().unwrap();
    let key = |f: &Value| f["a_steps"].as_u64().unwrap().max(f["b_steps"].as_u64().unwrap());
    assert!(rows.windows(2).all(|w| key(&w[0]) >= key(&w[1])), "{rows:?}");
    assert!(rows.iter().all(|f| ["a_calls", "a_steps", "b_calls", "b_steps"].iter().any(|k| f[k].as_u64().unwrap() > 0)));

    // The first divergence names a call on each side and its locator resolves to the reported step.
    let d = &v["divergence"];
    assert!(d.is_object(), "{v}");
    for (side, trace) in [("a", &t0), ("b", &t2)] {
        let e = &d[side];
        assert!(e["event"].as_str().unwrap().starts_with("call "), "{d}");
        assert_eq!(step_of(trace, e["locator"].as_str().unwrap()), e["step"].as_u64().unwrap(), "{d}");
    }
    assert_eq!(d["a"]["event"], "call fact", "{d}");
}

#[test]
fn trace_against_itself_has_no_divergence() {
    let b = build("fact.c", FACT_FLAGS);
    let t = record(&b, &[]);
    let v = compare_json(&t, &t);
    assert!(v["divergence"].is_null(), "{v}");
    assert_eq!(v["syscalls_identical"], true);
    assert_eq!(v["output_identical"], true);
    for f in v["functions"].as_array().unwrap() {
        assert_eq!(f["a_calls"], f["b_calls"]);
        assert_eq!(f["a_steps"], f["b_steps"]);
    }
    assert_eq!(v["other_steps"]["a"], v["other_steps"]["b"]);

    let text = ok(&["compare", p(&t), p(&t)]);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.last().unwrap().split_whitespace().collect::<Vec<_>>(), ["first", "divergence", "none"]);
    assert!(lines.iter().any(|l| l.starts_with("output") && l.trim_end().ends_with("14 bytes, identical")), "{text}");
    assert!(lines.iter().any(|l| l.starts_with("syscalls") && l.trim_end().ends_with("same sequence")), "{text}");
}

#[test]
fn text_layout() {
    let (_o0, t0, _o2, t2) = fact_pair();
    let text = ok(&["compare", p(&t0), p(&t2)]);
    let v = compare_json(&t0, &t2);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0].split_whitespace().collect::<Vec<_>>(), ["A", "B"], "{text}");
    assert!(!text.lines().any(|l| l != l.trim_end()), "trailing spaces: {text}");
    // The label column is 22 wide unless a function name needs more (name plus one space).
    let names = v["functions"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap().len());
    let label = names.chain(["(libraries, other)".len()]).map(|n| n + 1).fold(22, usize::max);
    assert_eq!(lines[0].len() - lines[0].trim_start().len(), label, "{text}");
    assert!(lines[1].starts_with("trace") && lines[1].contains(p(&t0)) && lines[1].contains(p(&t2)), "{text}");
    for (line, key) in lines[2..5].iter().zip(["steps", "end", "output"]) {
        assert!(line.starts_with(key), "{text}");
    }
    let header = lines.iter().position(|l| l.starts_with("function")).expect("function table");
    assert_eq!(lines[header].split_whitespace().collect::<Vec<_>>(), ["function", "A", "calls", "A", "steps", "B", "calls", "B", "steps"]);
    let fact = lines[header + 1..].iter().find(|l| l.starts_with("fact ")).expect("fact row");
    let f = row(&v, "fact");
    let cells: Vec<String> = ["a_calls", "a_steps", "b_calls", "b_steps"].iter().map(|k| f[k].to_string()).collect();
    assert_eq!(fact.split_whitespace().skip(1).collect::<Vec<_>>(), cells, "{text}");
    // Cells line up under their headers.
    let starts = |line: &str| -> Vec<usize> {
        line.char_indices().filter(|&(i, c)| c != ' ' && (i == 0 || line.as_bytes()[i - 1] == b' ')).map(|(i, _)| i).collect()
    };
    let (h, r) = (starts(lines[header]), starts(fact));
    assert_eq!([r[1], r[2], r[3], r[4]], [h[1], h[3], h[5], h[7]], "{text}");
    assert!(lines.iter().any(|l| l.starts_with("(libraries, other)")), "{text}");
    let div = lines.last().unwrap();
    assert!(div.starts_with("first divergence") && div.contains("  #0  A: call fact at step "), "{text}");
    assert!(div.contains("  B: call "), "{text}");
}

#[test]
fn different_end_and_output_are_reported() {
    let fact = build("fact.c", FACT_FLAGS);
    let tf = record(&fact, &[]);
    let fpe = build("sigfpe.c", FACT_FLAGS);
    let ts = record(&fpe, &[]);
    let v = compare_json(&tf, &ts);
    assert_eq!(v["a"]["end"], "exit 0");
    assert_eq!(v["b"]["end"], "signal SIGFPE");
    assert_eq!(v["b"]["output_bytes"], 0);
    assert_eq!(v["output_identical"], false);
    assert_eq!(v["syscalls_identical"], false);
    assert_eq!(v["divergence"]["b"]["event"], "signal SIGFPE");

    let text = ok(&["compare", p(&tf), p(&ts)]);
    assert!(text.lines().any(|l| l.starts_with("end") && l.ends_with("signal SIGFPE")), "{text}");
    assert!(text.lines().any(|l| l.starts_with("output") && l.ends_with("0 bytes, differ")), "{text}");
    assert!(text.lines().any(|l| l.starts_with("syscalls") && l.contains(", differs at #0: A ") && l.ends_with(", B (ended)")), "{text}");
}

#[test]
fn output_compares_bytes_of_writes_to_stdout_and_stderr_only() {
    // Same length and syscalls: only the stderr text differs.
    let src = |tail: &str, other: &str| {
        format!(
            "#include <unistd.h>\nint main(void) {{ write(1, \"ab\", 2); write(2, \"x{tail}\", 2); write(1000, \"{other}\", 4); return 0; }}\n"
        )
    };
    let a = build_c(&src("y", "aaaa"));
    let b = build_c(&src("z", "bbbb"));
    let c = build_c(&src("y", "cccc"));
    let (ta, tb, tc) = (record(&a, &[]), record(&b, &[]), record(&c, &[]));

    let v = compare_json(&ta, &tb);
    assert_eq!(v["a"]["output_bytes"], 4);
    assert_eq!(v["b"]["output_bytes"], 4);
    assert_eq!(v["output_identical"], false);
    assert_eq!(v["syscalls_identical"], true);

    // fd 1000 is not open, so that write fails and its bytes are not output.
    let v = compare_json(&ta, &tc);
    assert_eq!(v["a"]["output_bytes"], 4);
    assert_eq!(v["output_identical"], true);
}

#[test]
fn unloadable_trace_exits_2() {
    let b = build("fact.c", FACT_FLAGS);
    let t = record(&b, &[]);
    let missing = b.dir.path().join("missing.slt");
    let o = run(&["compare", p(&missing), p(&t)]);
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));
    assert!(stderr(&o).contains("missing.slt"), "{}", stderr(&o));
    let o = run(&["compare", p(&t), p(&b.exe)]);
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));
    assert!(stderr(&o).contains("not a SystemsLab trace"), "{}", stderr(&o));
}

#[test]
fn partial_write_counts_only_the_bytes_the_kernel_accepted() {
    // A non-blocking pipe accepts at most its capacity, so both writes return less than they asked for;
    // the second write finds the pipe full and fails.
    let src = |count: usize| {
        format!(
            "#include <fcntl.h>\n#include <string.h>\n#include <unistd.h>\nstatic char big[70000];\n\
             int main(void) {{ int fd[2]; pipe(fd); fcntl(fd[1], F_SETFL, O_NONBLOCK); dup2(fd[1], 1);\n\
             memset(big, 'q', sizeof big); write(1, big, {count}); write(1, \"ZZ\", 2); return 0; }}\n"
        )
    };
    let a = build_c(&src(70000));
    let b = build_c(&src(66000));
    let (ta, tb) = (record(&a, &[]), record(&b, &[]));
    let v = compare_json(&ta, &tb);
    let (na, nb) = (v["a"]["output_bytes"].as_u64().unwrap(), v["b"]["output_bytes"].as_u64().unwrap());
    assert!(0 < na && na < 66000, "{v}");
    assert_eq!(na, nb, "{v}");
    assert_eq!(v["output_identical"], true, "{v}");
}
