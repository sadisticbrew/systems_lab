//! Guide directives and `systemslab check-guide`, including the bundled experiments' guides.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::*;
use serde_json::Value;
use systemslab::guide::{Directive, Kind, parse};
use systemslab::model::WatchType;

#[test]
fn every_directive_form_parses() {
    let md = "\
# Title

@go w32
@go  fact.c:4@3   trailing words are ignored
@predict rax at w32 after
@predict rsp mod 16 at fact
@predict n at fact.c:4@3
@predict -0x8(%rbp) as i64 at fact+0x11@3 after
";
    let entries = parse(md);
    assert_eq!(entries.len(), 6);
    assert_eq!(entries.iter().map(|e| e.line).collect::<Vec<_>>(), [3, 4, 5, 6, 7, 8]);
    assert_eq!(entries[0].kind, Kind::Go);
    assert_eq!(entries[0].text, "@go w32");
    assert_eq!(entries[0].directive, Ok(Directive::Go { locator: "w32".into() }));
    assert_eq!(entries[1].directive, Ok(Directive::Go { locator: "fact.c:4@3".into() }));
    let predict = |expr: &str, ty: Option<WatchType>, locator: &str, after: bool| {
        Ok(Directive::Predict { expr: expr.into(), ty, locator: locator.into(), after, explanation: String::new() })
    };
    assert_eq!(entries[2].directive, predict("rax", None, "w32", true));
    assert_eq!(entries[3].directive, predict("rsp mod 16", None, "fact", false));
    assert_eq!(entries[4].directive, predict("n", None, "fact.c:4@3", false));
    assert_eq!(entries[5].directive, predict("-0x8(%rbp)", Some(WatchType::I64), "fact+0x11@3", true));
}

#[test]
fn an_explanation_is_the_quote_lines_after_a_prediction() {
    let md = "\
@predict rax at w32 after

> First line.
>
> Second `line`.

Not part of it.
> Neither is this.
@go w32
> Not an explanation: it follows a @go.
@predict rbx at w16
> Ends the file.";
    let entries = parse(md);
    let explanations: Vec<&str> = entries
        .iter()
        .map(|e| match e.directive.as_ref().unwrap() {
            Directive::Predict { explanation, .. } => explanation.as_str(),
            Directive::Go { .. } => "-",
        })
        .collect();
    assert_eq!(explanations, ["First line.\n\nSecond `line`.", "-", "Ends the file."]);
}

#[test]
fn fenced_code_and_indented_lines_are_not_directives() {
    let md = "```\n@go w32\n@predict rax at w32\n```\n    @go w32\ntext @go w32\n@gone x\n@go w32\n";
    let entries = parse(md);
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].line, 8);
}

#[test]
fn malformed_directives_are_entries_with_a_message() {
    let entries = parse("@go\n@predict rax\n@predict at w32\n@predict rax as quad at w32\n@predict rax at w32 later\n@predict rax at\n");
    let errors: Vec<&str> = entries.iter().map(|e| e.directive.as_ref().unwrap_err().as_str()).collect();
    assert_eq!(errors.len(), 6, "{entries:?}");
    assert_eq!(errors[0], "@go needs a locator");
    assert!(errors[1].contains("at LOCATOR"), "{}", errors[1]);
    assert!(errors[2].contains("expression"), "{}", errors[2]);
    assert_eq!(errors[3], "unknown type 'quad'");
    assert!(errors[4].contains("after"), "{}", errors[4]);
    assert!(entries.iter().all(|e| e.kind == if e.text.starts_with("@go") { Kind::Go } else { Kind::Predict }));
}

/// `systemslab <args>` with XDG directories under `xdg`.
fn systemslab(xdg: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_systemslab"))
        .args(args)
        .env("XDG_DATA_HOME", xdg.join("data"))
        .env("XDG_CONFIG_HOME", xdg.join("config"))
        .env_remove("CC")
        .output()
        .unwrap()
}

/// Records experiment `name` under `xdg` and returns its trace and guide paths.
fn try_experiment(xdg: &Path, name: &str) -> (PathBuf, PathBuf) {
    let o = systemslab(xdg, &["try", name, "--no-view"]);
    assert!(o.status.success(), "try {name}: {}", stderr(&o));
    let dir = xdg.join("data/systemslab/experiments").join(name);
    (dir.join(format!("{name}.slt")), dir.join(format!("{name}.guide.md")))
}

fn check_json(xdg: &Path, trace: &Path, guide: &Path) -> (Option<i32>, Vec<Value>) {
    let o = systemslab(xdg, &["check-guide", "--json", p(trace), p(guide)]);
    (o.status.code(), json_lines(&stdout(&o)))
}

#[test]
fn bundled_guides_resolve_and_show_the_documented_values() {
    let xdg = tempfile::tempdir().unwrap();
    let expected: [(&str, &[&str]); 4] = [
        ("widths", &["0x11223344", "0xffffffffffff5566", "0xffffffffffff5577", "0xffffffffffff8877", "0x80", "0xffffffffffffff80"]),
        ("fact", &["0x8", "0x3", "0x6"]),
        ("align", &["0x0", "0x8", "0x0", "0x0"]),
        ("saved", &["0x2a", "0x7", "0x63"]),
    ];
    for (name, raws) in expected {
        let (trace, guide) = try_experiment(xdg.path(), name);
        let (code, lines) = check_json(xdg.path(), &trace, &guide);
        let (summary, rows) = lines.split_last().unwrap();
        assert_eq!(code, Some(0), "{name}: {lines:?}");
        assert_eq!(summary["errors"], 0, "{name}: {lines:?}");
        assert!(rows.iter().all(|r| r.get("error").is_none()), "{name}: {rows:?}");
        let predictions: Vec<&Value> = rows.iter().filter(|r| r["directive"] == "predict").collect();
        assert_eq!(summary["predictions"], predictions.len(), "{name}");
        assert_eq!(summary["gos"], rows.len() - predictions.len(), "{name}");
        assert_eq!(predictions.iter().map(|r| r["raw"].as_str().unwrap()).collect::<Vec<_>>(), raws, "{name}");
        assert!(rows.iter().all(|r| r["step"].is_u64()), "{name}: {rows:?}");
    }
}

#[test]
fn the_fact_guide_reads_variables_as_their_declared_type() {
    let xdg = tempfile::tempdir().unwrap();
    let (trace, guide) = try_experiment(xdg.path(), "fact");
    let (_, lines) = check_json(xdg.path(), &trace, &guide);
    let n = lines.iter().find(|r| r["expr"] == "n").unwrap();
    assert_eq!((n["type"].as_str(), n["value"].as_str(), n["raw"].as_str()), (Some("i64"), Some("3"), Some("0x3")));
    let rsp = lines.iter().find(|r| r["expr"] == "rsp mod 16").unwrap();
    assert_eq!((rsp["type"].as_str(), rsp["value"].as_str()), (Some("value"), Some("8")));
    assert_eq!(rsp["locator"], "fact");
    assert!(rsp["after"] == false && rsp["line"].as_u64().unwrap() > 0);
}

#[test]
fn text_output_is_one_aligned_line_per_directive() {
    let xdg = tempfile::tempdir().unwrap();
    let (trace, guide) = try_experiment(xdg.path(), "widths");
    let o = systemslab(xdg.path(), &["check-guide", p(&trace), p(&guide)]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let text = stdout(&o);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.last(), Some(&"6 predictions, 1 go, 0 errors"));
    let rows = &lines[..lines.len() - 1];
    assert_eq!(rows.len(), 7);
    assert!(rows[0].starts_with(&format!("{}:", guide.display())), "{}", rows[0]);
    assert!(rows[0].contains("@go w32") && rows[0].contains("step "), "{}", rows[0]);
    let first = rows.iter().find(|l| l.contains("@predict rax at w32 after")).unwrap();
    assert!(first.ends_with("287454020 (0x11223344)"), "{first}");
    // Aligned: the `step` column starts at the same offset in every row.
    let offsets: Vec<usize> = rows.iter().map(|l| l.find("  step ").unwrap()).collect();
    assert!(offsets.windows(2).all(|w| w[0] == w[1]), "{offsets:?}");
}

#[test]
fn unresolved_directives_are_errors_and_exit_1() {
    let xdg = tempfile::tempdir().unwrap();
    let (trace, _) = try_experiment(xdg.path(), "widths");
    let bad = xdg.path().join("bad.guide.md");
    std::fs::write(
        &bad,
        "@go nowhere\n@predict rax at w32 after\n@predict rax at step:end after\n@predict rax as quad at w32\n@predict nosuch at w32\n@predict rax at w32 later\n",
    )
    .unwrap();
    let o = systemslab(xdg.path(), &["check-guide", p(&trace), p(&bad)]);
    assert_eq!(o.status.code(), Some(1));
    let text = stdout(&o);
    let errors: Vec<&str> = text.lines().filter(|l| l.contains("error: ")).collect();
    assert_eq!(errors.len(), 5, "{text}");
    assert!(errors[0].contains("unknown symbol 'nowhere'"), "{}", errors[0]);
    assert!(errors[1].contains("past the end"), "{}", errors[1]);
    assert!(errors[2].contains("unknown type 'quad'"), "{}", errors[2]);
    assert!(errors[3].contains("unknown name 'nosuch'"), "{}", errors[3]);
    assert!(errors[4].contains("after"), "{}", errors[4]);
    assert_eq!(text.lines().last(), Some("5 predictions, 1 go, 5 errors"));
    let (code, lines) = check_json(xdg.path(), &trace, &bad);
    assert_eq!(code, Some(1));
    assert_eq!(lines.iter().filter(|l| l.get("error").is_some()).count(), 5);
    assert_eq!(lines.last().unwrap()["errors"], 5);
    // The step the first, good prediction reads is the state after `mov $0x11223344,%eax`.
    assert_eq!(lines[1]["raw"], "0x11223344");
    // A guide that cannot be read is a usage error.
    let o = systemslab(xdg.path(), &["check-guide", p(&trace), "/nonexistent/guide.md"]);
    assert_eq!(o.status.code(), Some(2));
}
