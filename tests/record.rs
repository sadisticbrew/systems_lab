mod common;

use common::*;
use serde_json::Value;

#[test]
fn fact_records_strict_and_info_says_exit_0_from_main() {
    let b = build("fact.c", FACT_FLAGS);
    let t = record(&b, &[]);
    let info: Value = serde_json::from_str(&ok(&["info", "--json", p(&t)])).unwrap();
    assert_eq!(info["end"]["reason"], "exit");
    assert_eq!(info["end"]["code"], 0);
    assert_eq!(info["unattributed_bytes"], 0);
    assert_eq!(info["from"], "main");
    let text = ok(&["info", p(&t)]);
    assert!(text.contains(&format!("{:<20}{}", "end", "exit 0")), "{text}");
    assert!(text.contains(&format!("{:<20}{}", "unattributed bytes", "0")), "{text}");
    assert!(text.contains(&format!("{:<20}{}", "signals", "none")), "{text}");
}

#[test]
fn uname_syscall_writes_the_kernel_buffer_on_the_stack() {
    let b = build("uname.s", ASM_FLAGS);
    let t = record(&b, &[]);
    let lines = json_lines(&ok(&["steps", p(&t), "--json"]));
    let step = lines
        .iter()
        .find(|l| l["event"]["kind"] == "syscall" && l["event"]["name"] == "uname")
        .expect("uname step");
    assert_eq!(step["event"]["ret"], "0");
    let kernel_stack_writes = step["writes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|w| w["kind"] == "kernel" && w["addr"].as_str().unwrap().starts_with("stack0"))
        .count();
    assert!(kernel_stack_writes >= 1, "{step}");
}

#[test]
fn rep_stos_is_fully_attributed() {
    let b = build("repstos.s", ASM_FLAGS);
    let o = run(&["record", "--strict", "--quiet", "-o", p(&b.dir.path().join("t.slt")), "--", p(&b.exe)]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
}

#[test]
fn sigfpe_ends_with_the_signal_on_an_idiv() {
    let b = build("sigfpe.c", &["-O0", "-g"]);
    let t = record(&b, &[]);
    let info: Value = serde_json::from_str(&ok(&["info", "--json", p(&t)])).unwrap();
    assert_eq!(info["end"]["reason"], "signal");
    assert_eq!(info["end"]["signal"], "SIGFPE");
    let text = ok(&["info", p(&t)]);
    assert!(text.contains(&format!("{:<20}{}", "end", "signal SIGFPE")), "{text}");
    let lines = json_lines(&ok(&["steps", p(&t), "--json", "--limit", "100000"]));
    let step = lines.iter().find(|l| l["event"]["kind"] == "signal").expect("signal step");
    assert!(step["loc"].as_str().unwrap().starts_with("main+"), "{step}");
    assert!(step["asm"].as_str().unwrap().contains("idiv"), "{step}");
    assert_eq!(step["event"]["signal"], "SIGFPE");
    let plain = ok(&["steps", p(&t), "--limit", "100000"]);
    assert!(plain.lines().any(|l| l.contains("signal SIGFPE code") && l.contains(" at main+")), "{plain}");
}

#[test]
fn from_symbol_starts_at_that_symbol() {
    let b = build("fact.c", FACT_FLAGS);
    let t = record(&b, &["--from", "fact"]);
    let lines = json_lines(&ok(&["steps", p(&t), "--json", "--limit", "1"]));
    assert_eq!(lines[0]["loc"], "fact");
    assert_eq!(lines[0]["step"], 0);
}

#[test]
fn recording_twice_gives_identical_stack_text() {
    let b = build("fact.c", FACT_FLAGS);
    let t1 = record_named(&b, "a.slt", &[]);
    let t2 = record_named(&b, "b.slt", &[]);
    let a = ok(&["stack", p(&t1), "fact+0xc@3"]);
    let c = ok(&["stack", p(&t2), "fact+0xc@3"]);
    assert_eq!(a, c);
}

#[test]
fn from_entry_is_deterministic_for_check_abi_and_steps() {
    let b = build("fact.c", FACT_FLAGS);
    let t1 = record_named(&b, "a.slt", &["--from", "entry"]);
    let t2 = record_named(&b, "b.slt", &["--from", "entry"]);
    assert_eq!(ok(&["check-abi", p(&t1)]).trim(), "no findings");
    // The total step count of a from-entry run varies by about a hundred steps between runs
    // (the dynamic loader branches on random bytes), so only the listed steps are compared.
    let first = |t| ok(&["steps", p(t), "--limit", "5"]).lines().filter(|l| l.starts_with('#')).map(str::to_string).collect::<Vec<_>>();
    assert_eq!(first(&t1).len(), 5);
    assert_eq!(first(&t1), first(&t2));
    assert_eq!(ok(&["check-abi", p(&t1)]), ok(&["check-abi", p(&t2)]));
}
