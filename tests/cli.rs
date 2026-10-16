mod common;

use common::*;
use serde_json::Value;

fn fact_trace() -> (Built, std::path::PathBuf) {
    let b = build("fact.c", FACT_FLAGS);
    let t = record(&b, &[]);
    (b, t)
}

fn labels(slots: &Value) -> Vec<String> {
    slots.as_array().unwrap().iter().map(|s| s["label"].as_str().unwrap().to_string()).collect()
}

#[test]
fn stack_json_at_fact_cmpq_third_time() {
    let (_b, t) = fact_trace();
    let v: Value = serde_json::from_str(&ok(&["stack", "--json", p(&t), "fact+0xc@3"])).unwrap();
    assert_eq!(v["loc"], "fact+0xc");
    assert!(v["asm"].as_str().unwrap().starts_with("cmpq "), "{}", v["asm"]);
    let frames = v["frames"].as_array().unwrap();
    let names: Vec<&str> = frames.iter().map(|f| f["label"].as_str().unwrap()).collect();
    assert_eq!(names, ["main", "fact(5)", "fact(4)", "fact(3)"]);
    assert_eq!(frames[0]["collapsed"], true);
    assert!(frames[0]["size"].is_null());
    for f in &frames[1..] {
        assert_eq!(f["collapsed"], false);
        assert_eq!(f["func"], "fact");
        assert_eq!(f["size"], 32);
    }
    let f3 = &frames[3]["slots"];
    assert_eq!(labels(f3), ["return address: imulq in fact", "saved %rbp: fact(4)'s", "n = 3", "unused"]);
    let ptrs: Vec<Vec<&str>> =
        f3.as_array().unwrap().iter().map(|s| s["pointers"].as_array().unwrap().iter().map(|p| p.as_str().unwrap()).collect()).collect();
    assert_eq!(ptrs, [vec![], vec!["%rbp"], vec![], vec!["%rsp"]]);
    let rbp_offs: Vec<&str> = f3.as_array().unwrap().iter().map(|s| s["rbp_off"].as_str().unwrap()).collect();
    assert_eq!(rbp_offs, ["8(%rbp)", "0(%rbp)", "-8(%rbp)", "-16(%rbp)"]);
    let f5 = labels(&frames[1]["slots"]);
    assert_eq!(f5[0], "return address: movq in main");
    assert_eq!(f5[1], "saved %rbp: main's");
    assert!(frames[3]["slots"][2]["writer"].is_u64(), "n was written by a step");
    assert!(frames[3]["slots"][3]["writer"].is_null(), "unused slot was never written");
    assert!(frames[3]["slots"][0]["raw_addr"].as_str().unwrap().starts_with("0x"));
}

#[test]
fn stack_text_layout() {
    let (_b, t) = fact_trace();
    let text = ok(&["stack", p(&t), "fact+0xc@3"]);
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("step ") && lines[0].contains(" at fact+0xc (cmpq "), "{text}");
    assert_eq!(lines[1], "main's frame");
    assert_eq!(lines[2], "fact(5), 32 bytes");
    assert!(lines[3].starts_with("     8(%rbp)  return address: movq in main  "), "{}", lines[3]);
    let last = lines.last().unwrap();
    assert!(last.starts_with("   -16(%rbp)  unused"), "{last}");
    assert!(last.contains("  unused  ") && last.ends_with("  ← %rsp"), "{last}");
    assert!(!text.lines().any(|l| l != l.trim_end()), "trailing spaces");
    assert!(!text.contains("0x7f"), "raw stack address leaked: {text}");
}

#[test]
fn stack_in_main_shows_mains_own_frame() {
    let (_b, t) = fact_trace();
    let v: Value = serde_json::from_str(&ok(&["stack", "--json", p(&t), "main+0x4"])).unwrap();
    let frames = v["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 1, "{v}");
    assert_eq!(frames[0]["collapsed"], false);
    assert_eq!(frames[0]["label"], "main");
    let slots = frames[0]["slots"].as_array().unwrap();
    assert_eq!(slots.len(), 2);
    let l = labels(&frames[0]["slots"]);
    assert!(l[0].starts_with("return address: "), "{l:?}");
    assert!(l[1].starts_with("saved %rbp: "), "{l:?}");
    assert_eq!(slots[1]["pointers"], serde_json::json!(["%rsp", "%rbp"]));
    let text = ok(&["stack", p(&t), "main+0x4"]);
    assert_eq!(text.lines().nth(1), Some("main, 16 bytes"), "{text}");
}

#[test]
fn stack_frames_option_collapses_outer_activations() {
    let (_b, t) = fact_trace();
    let v: Value = serde_json::from_str(&ok(&["stack", "--json", "--frames", "1", p(&t), "fact+0xc@3"])).unwrap();
    let frames = v["frames"].as_array().unwrap();
    let names: Vec<&str> = frames.iter().map(|f| f["label"].as_str().unwrap()).collect();
    assert_eq!(names, ["fact(4)", "fact(3)"]);
    let text = ok(&["stack", "--frames", "1", p(&t), "fact+0xc@3"]);
    assert!(text.contains("fact(4)'s frame\n"), "{text}");
}

#[test]
fn dot_stack_is_valid_graphviz_with_the_saved_rbp_label() {
    let (_b, t) = fact_trace();
    let src = ok(&["dot", "stack", p(&t), "fact+0xc@3"]);
    assert!(src.starts_with("digraph { node [shape=plaintext];\n"), "{src}");
    assert!(src.ends_with("</TABLE>>];\n}\n"), "{src}");
    assert!(src.contains(r#"<TR><TD ALIGN="RIGHT">fact(5),<BR/>32 bytes</TD>"#));
    assert!(src.contains(r#"<TD BORDER="0" ALIGN="LEFT">← %rbp</TD>"#));
    assert!(!src.contains("color") && !src.contains("font"));
    let o = dot("svg", &src);
    assert!(o.status.success(), "{}", stderr(&o));
    let svg = stdout(&o);
    assert!(svg.contains("saved %rbp: fact(4)&#39;s") || svg.contains("saved %rbp: fact(4)'s"), "{svg}");
}

#[test]
fn check_abi_is_clean_on_fact() {
    let (_b, t) = fact_trace();
    let o = run(&["check-abi", p(&t)]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(stdout(&o), "no findings\n");
    let o = run(&["check-abi", "--all", "--json", p(&t)]);
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));
    assert_eq!(stdout(&o).trim(), r#"{"findings":0}"#);
}

#[test]
fn check_abi_reports_alignment_and_clobbered_callee_saved_register() {
    let b = build("abi_bad.s", ASM_FLAGS);
    let t = record(&b, &[]);
    let o = run(&["check-abi", "--json", p(&t)]);
    assert_eq!(o.status.code(), Some(1), "{}", stdout(&o));
    let lines = json_lines(&stdout(&o));
    let (last, findings) = lines.split_last().unwrap();
    assert_eq!(last["findings"], 2);
    let align = findings.iter().find(|f| f["rule"] == "call-alignment").expect("call-alignment");
    assert_eq!(align["loc"], "_start+0x9");
    assert_eq!(align["message"], "%rsp mod 16 = 8 before call to misaligned (must be 0)");
    let saved = findings.iter().find(|f| f["rule"] == "callee-saved").expect("callee-saved");
    let msg = saved["message"].as_str().unwrap();
    assert!(msg.contains("clobber") && msg.contains("%rbx"), "{msg}");
    assert!(msg.starts_with("clobber changed %rbx: 7 at the call, 99 after its ret (step "), "{msg}");
    let steps: Vec<u64> = findings.iter().map(|f| f["step"].as_u64().unwrap()).collect();
    assert!(steps.windows(2).all(|w| w[0] <= w[1]), "{steps:?}");
    let text = stdout(&run(&["check-abi", p(&t)]));
    assert!(text.starts_with("call-alignment step "), "{text}");
    assert!(text.ends_with("2 findings\n"), "{text}");
}

#[test]
fn locator_errors_exit_2_with_contract_messages() {
    let (_b, t) = fact_trace();
    let cases: [(&str, String); 6] = [
        ("fact+0xc@9", "fact+0xc occurs 5 times; @9 asked".into()),
        ("fact+0xc@6", "fact+0xc occurs 5 times; @6 asked".into()),
        ("nosuch", "unknown symbol 'nosuch'".into()),
        ("step:999999", "step:999999 is outside 0..=".into()),
        ("fact+zz", "cannot parse locator 'fact+zz'".into()),
        ("0x1@1", "0x1 occurs 0 times; @1 asked".into()),
    ];
    for (loc, expect) in cases {
        let o = run(&["stack", p(&t), loc]);
        assert_eq!(o.status.code(), Some(2), "{loc}");
        assert!(stderr(&o).contains(&expect), "{loc}: {}", stderr(&o));
    }
    // @5 is the last occurrence and resolves.
    assert!(run(&["stack", p(&t), "fact+0xc@5"]).status.success());
    assert!(run(&["stack", p(&t), "step:0"]).status.success());
}

#[test]
fn steps_shows_push_rbp_with_a_stack_write_and_truncates() {
    let (_b, t) = fact_trace();
    let text = ok(&["steps", p(&t), "--func", "fact", "--limit", "12"]);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 13, "{text}");
    assert!(lines[0].starts_with("#4 fact  push %rbp rsp=stack0-"), "{}", lines[0]);
    assert!(lines[0].contains(" stack0-0x18←stack0-0x8/8"), "{}", lines[0]);
    assert!(lines[2].contains("rflags=["), "{}", lines[2]);
    assert!(lines[12].starts_with("… ") && lines[12].ends_with(" more steps"), "{}", lines[12]);
    let json = json_lines(&ok(&["steps", p(&t), "--func", "fact", "--limit", "2", "--json"]));
    assert_eq!(json.len(), 3);
    assert!(json[2]["truncated"].as_u64().unwrap() > 0);
    // --from/--to bound the range; --to is exclusive.
    let ranged = ok(&["steps", p(&t), "--from", "step:4", "--to", "step:6"]);
    assert_eq!(ranged.lines().count(), 2, "{ranged}");
    let by_sym = ok(&["steps", p(&t), "--from", "fact+0xc@3", "--limit", "1"]);
    assert!(by_sym.starts_with("#28 fact+0xc  cmpq $0x1,-0x8(%rbp)"), "{by_sym}");
}

#[test]
fn steps_prints_syscall_events_without_raw_addresses() {
    let b = build("uname.s", ASM_FLAGS);
    let t = record(&b, &[]);
    let text = ok(&["steps", p(&t)]);
    let line = text.lines().find(|l| l.contains("syscall uname(")).expect("uname line");
    assert!(line.contains("syscall uname(stack0-0x190) = 0"), "{line}");
    assert!(text.lines().any(|l| l.contains("syscall exit(0) = ?")), "{text}");
    assert!(!text.contains("0x7ffe") && !text.contains("0x7fff"), "{text}");
}

#[test]
fn info_text_has_the_contract_keys() {
    let (_b, t) = fact_trace();
    let text = ok(&["info", p(&t)]);
    let keys: Vec<&str> = text.lines().map(|l| l[..20].trim_end()).collect();
    assert_eq!(keys, ["program", "argv", "from", "steps", "end", "aslr", "unattributed bytes", "syscalls", "signals"]);
    let sys = text.lines().find(|l| l.starts_with("syscalls")).unwrap();
    assert!(sys.contains("write×1") && sys.contains("exit_group×1"), "{sys}");
}

/// A running `systemslab view`, killed on drop.
struct Viewer {
    child: std::process::Child,
    port: u16,
}

impl Drop for Viewer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_viewer(trace: &std::path::Path) -> Viewer {
    use std::io::BufRead;
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_systemslab"))
        .args(["view", p(trace), "--no-open", "--port", "0"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn view");
    let mut line = String::new();
    std::io::BufReader::new(child.stdout.as_mut().unwrap()).read_line(&mut line).unwrap();
    let port = line.split("127.0.0.1:").nth(1).and_then(|r| r.split_whitespace().next()).and_then(|p| p.parse().ok()).expect(&line);
    Viewer { child, port }
}

/// One `GET` over a fresh connection: status code and body. Fails if the server stays silent for 20 s.
fn http_get(v: &Viewer, path: &str) -> (u16, String) {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", v.port)).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(20))).unwrap();
    write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n", v.port).unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).unwrap_or_else(|e| panic!("GET {path}: {e}"));
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or_else(|| panic!("GET {path}: no response"));
    (head.split_whitespace().nth(1).unwrap().parse().unwrap(), body.to_string())
}

#[test]
fn viewer_answers_hostile_requests_with_the_documented_status() {
    let (_b, t) = fact_trace();
    let v = start_viewer(&t);
    // Out-of-range steps are 404 even when they do not fit in a u32; unparsable ones are 400.
    assert_eq!(http_get(&v, "/api/state/99999").0, 404);
    assert_eq!(http_get(&v, "/api/state/5000000000").0, 404);
    assert_eq!(http_get(&v, "/api/state/abc").0, 400);
    assert_eq!(http_get(&v, "/api/state/-1").0, 400);
    assert_eq!(http_get(&v, "/api/mem?addr=0x10&len=4&step=5000000000").0, 404);
    assert_eq!(http_get(&v, "/api/nav?from=0&op=nope").0, 400);
    // The top of the address space reads as unmapped instead of wrapping around (which spun forever).
    let (code, body) = http_get(&v, "/api/mem?addr=0xfffffffffffff000&len=4096&step=0");
    assert_eq!(code, 200, "{body}");
    assert!(body.contains(r#""bytes":[null,null"#), "{}", &body[..body.len().min(80)]);
    // Embedded assets cannot be escaped with `..`, plain or percent-encoded.
    for path in ["/../Cargo.toml", "/%2e%2e/Cargo.toml", "/assets/..%2f..%2fCargo.toml", "/../../../../etc/passwd"] {
        assert_eq!(http_get(&v, path).0, 404, "{path}");
    }
    assert_eq!(http_get(&v, "/api/info").0, 200, "the server is still answering");
}
