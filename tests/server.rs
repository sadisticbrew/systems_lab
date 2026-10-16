mod common;

use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::*;
use serde_json::Value;

const MARK: (&str, &str) = ("X-SystemsLab", "1");

fn fact_trace() -> (Built, PathBuf) {
    let b = build("fact.c", FACT_FLAGS);
    let t = record(&b, &[]);
    (b, t)
}

fn get_json(v: &Viewer, path: &str) -> Value {
    let r = http_request(v, "GET", path, None, &[], &[]);
    assert_eq!(r.status, 200, "GET {path}: {}", r.body);
    r.json()
}

/// Polls `/api/status` until `done` accepts it (10 s at most) and returns it.
fn wait_status(v: &Viewer, what: &str, done: impl Fn(&Value) -> bool) -> Value {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let s = get_json(v, "/api/status");
        if done(&s) {
            return s;
        }
        assert!(Instant::now() < end, "timed out waiting for {what}; last status: {s}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn info_reports_the_server_side_fields() {
    let (_b, t) = fact_trace();
    let v = start_viewer(&t);
    let info = get_json(&v, "/api/info");
    let canon = std::fs::canonicalize(&t).unwrap();
    assert_eq!(info["trace_path"], canon.to_str().unwrap());
    let notes = info["notes_path"].as_str().unwrap();
    let expected = v
        .data_home()
        .join(format!("systemslab/notes/{:016x}.json", systemslab::paths::fnv1a64(canon.as_os_str().as_bytes())));
    assert_eq!(notes, expected.to_str().unwrap());
    assert_eq!(info["mode"], "view");
    assert!(info["source_path"].is_null());
    assert_eq!(info["has_guide"], false);
    assert!(info["steps"].as_u64().unwrap() > 1000, "the ApiInfo fields are still there: {info}");
    assert!(info["functions"].is_array());
    assert_eq!(get_json(&v, "/api/status"), serde_json::json!({"generation": 1, "building": false, "error": null}));
}

#[test]
fn requests_with_a_foreign_host_header_are_refused() {
    let (_b, t) = fact_trace();
    let v = start_viewer(&t);
    let own = format!("127.0.0.1:{}", v.port);
    let localhost = format!("localhost:{}", v.port);
    let other_port = format!("127.0.0.1:{}", u32::from(v.port) + 1);
    for path in ["/api/info", "/api/state/0", "/api/notes", "/", "/index.html", "/api/nothing"] {
        for host in ["evil.example", "evil.example:80", other_port.as_str(), "127.0.0.1", "localhost"] {
            let r = http_request(&v, "GET", path, Some(host), &[], &[]);
            assert_eq!(r.status, 403, "GET {path} with Host {host}: {}", r.body);
            assert_eq!(r.json()["error"], "bad Host header");
        }
        for host in [own.as_str(), localhost.as_str()] {
            let r = http_request(&v, "GET", path, Some(host), &[], &[]);
            assert_ne!(r.status, 403, "GET {path} with Host {host}");
        }
    }
    let put = http_request(&v, "PUT", "/api/notes", Some("evil.example"), &[MARK], b"{}");
    assert_eq!(put.status, 403);
    assert!(!v.data_home().exists(), "a refused PUT wrote something");
}

/// Exercises `GET`/`PUT` of `/api/notes` or `/api/prefs`; `file` is where the server must store it.
fn check_json_store(v: &Viewer, route: &str, file: &Path) {
    assert_eq!(get_json(v, route), serde_json::json!({}), "{route} starts empty");
    let body = br#"{"version":1,"text":"h\u00e9llo"}"#;
    let r = http_request(v, "PUT", route, None, &[], body);
    assert_eq!((r.status, r.json()["error"].as_str().unwrap()), (403, "missing X-SystemsLab header"));
    let r = http_request(v, "PUT", route, None, &[("X-SystemsLab", "0")], body);
    assert_eq!(r.status, 403);
    assert!(!file.exists(), "a refused PUT wrote {}", file.display());

    for (bad, why) in [("[1,2]", "array"), ("\"s\"", "string"), ("{\"a\":", "truncated"), ("", "empty")] {
        let r = http_request(v, "PUT", route, None, &[MARK], bad.as_bytes());
        assert_eq!(r.status, 400, "{why}: {}", r.body);
        assert!(r.json()["error"].is_string());
    }
    assert!(!file.exists());

    let r = http_request(v, "PUT", route, None, &[MARK], body);
    assert_eq!((r.status, r.body.as_str()), (204, ""));
    assert!(file.is_file(), "{} was not created", file.display());
    assert_eq!(std::fs::read(file).unwrap(), body);
    assert_eq!(get_json(v, route), serde_json::from_slice::<Value>(body).unwrap());
    let tmp = format!("{}.tmp", file.display());
    assert!(!Path::new(&tmp).exists(), "temp file left behind");

    // Over the 1 MiB limit nothing is stored.
    let mut big = br#"{"pad":""#.to_vec();
    big.extend(std::iter::repeat_n(b'a', (1 << 20) + 1024));
    big.extend(br#""}"#);
    let r = http_request(v, "PUT", route, None, &[MARK], &big);
    assert_eq!(r.status, 413, "{}", r.body);
    assert_eq!(std::fs::read(file).unwrap(), body);

    // A file that is not a JSON object is reported, never silently replaced by `{}`.
    std::fs::write(file, "not json").unwrap();
    assert_eq!(http_request(v, "GET", route, None, &[], &[]).status, 500);
    std::fs::remove_file(file).unwrap();
    assert_eq!(get_json(v, route), serde_json::json!({}));
}

#[test]
fn notes_and_prefs_are_stored_atomically_and_need_the_header() {
    let (_b, t) = fact_trace();
    let v = start_viewer(&t);
    let notes = PathBuf::from(get_json(&v, "/api/info")["notes_path"].as_str().unwrap());
    assert!(notes.starts_with(v.data_home()));
    check_json_store(&v, "/api/notes", &notes);
    check_json_store(&v, "/api/prefs", &v.config_home().join("systemslab/viewer.json"));
}

#[test]
fn guide_is_discovered_next_to_the_trace_or_in_a_lab_directory_and_read_fresh() {
    let (b, t) = fact_trace();
    let v = start_viewer(&t);
    let r = http_request(&v, "GET", "/api/guide", None, &[], &[]);
    assert_eq!((r.status, r.json()["error"].as_str().unwrap()), (404, "no guide"));
    drop(v);

    // `<trace stem>.guide.md` beside the trace.
    let guide = t.with_extension("guide.md");
    std::fs::write(&guide, "# One\n").unwrap();
    let v = start_viewer(&t);
    assert_eq!(get_json(&v, "/api/info")["has_guide"], true);
    let g = get_json(&v, "/api/guide");
    assert_eq!(g["markdown"], "# One\n");
    assert_eq!(g["path"], std::path::absolute(&guide).unwrap().to_str().unwrap());
    std::fs::write(&guide, "# Two\n").unwrap();
    assert_eq!(get_json(&v, "/api/guide")["markdown"], "# Two\n", "the guide is read on every request");
    drop(v);

    // `--guide` wins over discovery.
    let other = b.dir.path().join("other.md");
    std::fs::write(&other, "# Other\n").unwrap();
    let v = start_viewer_with(&["view", p(&t), "--no-open", "--port", "0", "--guide", p(&other)]);
    assert_eq!(get_json(&v, "/api/guide")["markdown"], "# Other\n");
    drop(v);

    // A lab directory: `lab/guide.md` and `lab/out/*.slt`.
    let lab = b.dir.path().join("lab");
    std::fs::create_dir_all(lab.join("out")).unwrap();
    std::fs::copy(&t, lab.join("out/fact.slt")).unwrap();
    std::fs::write(lab.join("guide.md"), "# Lab\n").unwrap();
    let v = start_viewer_with(&["view", p(&lab), "--no-open", "--port", "0"]);
    assert_eq!(get_json(&v, "/api/guide")["markdown"], "# Lab\n");
    drop(v);

    // A guide that does not exist is a usage error before anything is served.
    let o = run(&["view", p(&t), "--no-open", "--guide", "/nonexistent/guide.md"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("cannot read guide /nonexistent/guide.md"), "{}", stderr(&o));
}

#[test]
fn at_resolves_before_binding_and_extends_the_url() {
    let (_b, t) = fact_trace();
    let v = start_viewer_with(&["view", p(&t), "--no-open", "--port", "0", "--at", "fact+0xc@3"]);
    assert!(v.url.ends_with("/#step=28"), "{}", v.url);
    drop(v);
    let o = run(&["view", p(&t), "--no-open", "--at", "nosuch"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("unknown symbol 'nosuch'"), "{}", stderr(&o));
    assert!(stdout(&o).is_empty(), "nothing is served for a bad --at: {}", stdout(&o));
}

#[test]
fn a_replaced_trace_file_is_reloaded_and_a_bad_one_keeps_the_old_trace() {
    let (b, t) = fact_trace();
    let short = record_named(&b, "short.slt", &["--max-steps", "50"]);
    let v = start_viewer(&t);
    let steps = get_json(&v, "/api/info")["steps"].as_u64().unwrap();
    assert!(steps > 1000);

    // An atomic replacement (what `record -o` does) is picked up.
    std::fs::rename(&short, &t).unwrap();
    wait_status(&v, "generation 2", |s| s["generation"] == 2);
    let info = get_json(&v, "/api/info");
    assert_eq!(info["steps"], 50);
    assert_eq!(info["end"]["reason"], "max-steps");
    assert_eq!(get_json(&v, "/api/status"), serde_json::json!({"generation": 2, "building": false, "error": null}));

    // A file that is not a trace is reported; the old trace keeps being served.
    std::fs::write(&t, "this is not a trace").unwrap();
    let s = wait_status(&v, "a reload error", |s| s["error"].is_string());
    assert_eq!(s["generation"], 2);
    assert!(s["error"].as_str().unwrap().contains("not a SystemsLab trace"), "{s}");
    assert_eq!(get_json(&v, "/api/info")["steps"], 50);
    assert_eq!(http_request(&v, "GET", "/api/state/0", None, &[], &[]).status, 200);

    // Replacing it with a good one clears the error.
    let full = record_named(&b, "full.slt", &[]);
    std::fs::rename(&full, &t).unwrap();
    let s = wait_status(&v, "generation 3", |s| s["generation"] == 3);
    assert!(s["error"].is_null(), "{s}");
    assert_eq!(get_json(&v, "/api/info")["steps"].as_u64().unwrap(), steps);
}

#[test]
fn analysis_routes_return_the_documented_shapes() {
    let (_b, t) = fact_trace();
    let v = start_viewer(&t);
    let at = get_json(&v, "/api/locate?loc=fact.c:4@3")["step"].as_u64().unwrap();

    let events = get_json(&v, "/api/events");
    assert!(events["events"].as_array().unwrap().iter().any(|e| e["kind"] == "syscall" && e["name"] == "write"), "{events}");

    let rsp = get_json(&v, &format!("/api/eval?step={at}&expr=rsp+mod+16"));
    assert_eq!((rsp["default_type"].as_str(), rsp["rendered"].as_str()), (Some("value"), Some("0")));
    assert!(rsp["value"].as_str().unwrap().starts_with("0x"), "{rsp}");
    let n = get_json(&v, &format!("/api/eval?step={at}&expr=n"));
    assert_eq!(n["default_type"], "i64");
    let bad = http_request(&v, "GET", &format!("/api/eval?step={at}&expr=nosuch"), None, &[], &[]);
    assert_eq!(bad.status, 400);
    assert_eq!(bad.json()["error"], "unknown name 'nosuch'");
    assert_eq!(http_request(&v, "GET", "/api/eval?step=0", None, &[], &[]).status, 400);
    assert_eq!(http_request(&v, "GET", "/api/eval?step=99999999&expr=rsp", None, &[], &[]).status, 404);

    let watches = get_json(&v, &format!("/api/watch?step={at}&w=n%7Ci64&w=rax&w=nosuch&w=-0x8%28%25rbp%29"));
    let ws = watches["watches"].as_array().unwrap();
    assert_eq!(ws.len(), 4, "{watches}");
    assert_eq!((ws[0]["expr"].as_str(), ws[0]["type"].as_str(), ws[0]["value"].as_str(), ws[0]["raw"].as_str()), (Some("n"), Some("i64"), Some("3"), Some("0x3")));
    assert_eq!(ws[0]["addr"], n["value"]);
    assert!(ws[0]["error"].is_null() && ws[0]["changed"] == true);
    assert_eq!((ws[1]["type"].as_str(), ws[1]["addr"].is_null()), (Some("value"), true));
    assert_eq!(ws[2]["error"], "unknown name 'nosuch'");
    assert_eq!((ws[3]["type"].as_str(), ws[3]["value"].as_str()), (Some("ptr"), Some("3")));
    assert_eq!(http_request(&v, "GET", &format!("/api/watch?step={at}&w=n%7Clong"), None, &[], &[]).status, 400);
    assert_eq!(http_request(&v, "GET", "/api/watch?w=n", None, &[], &[]).status, 400);
    assert_eq!(get_json(&v, &format!("/api/watch?step={at}"))["watches"], serde_json::json!([]));

    let addr = n["value"].as_str().unwrap();
    let insp = get_json(&v, &format!("/api/inspect?step={at}&addr={addr}"));
    assert_eq!(insp["addr"], addr);
    assert!(insp["region"].as_str().unwrap().starts_with("[stack] (rw"), "{insp}");
    assert_eq!(insp["values"].as_array().unwrap().len(), 11);
    assert_eq!(insp["writer_asm"], "mov %rdi,-0x8(%rbp)");
    assert_eq!(insp["bytes"][0], 3);
    assert_eq!(http_request(&v, "GET", &format!("/api/inspect?step={at}&addr=zz"), None, &[], &[]).status, 400);

    let tl = get_json(&v, "/api/timeline?from=10&to=20");
    assert_eq!((tl["from"].as_u64(), tl["to"].as_u64(), tl["depth"].as_array().unwrap().len()), (Some(10), Some(20), 11));
    assert!(tl["markers"].as_array().unwrap().iter().all(|m| (10..=20).contains(&m["step"].as_u64().unwrap())));
    let whole = get_json(&v, "/api/timeline");
    assert_eq!((whole["from"].as_u64(), whole["depth"].as_array().unwrap().len()), (Some(0), 1000));
    assert_eq!(whole["to"], whole["steps"]);
    assert_eq!(http_request(&v, "GET", "/api/timeline?from=20&to=10", None, &[], &[]).status, 400);
    assert_eq!(http_request(&v, "GET", "/api/timeline?to=99999999", None, &[], &[]).status, 404);

    let nav = get_json(&v, &format!("/api/nav?from={at}&op=line-next&file=fact.c&line=5"));
    assert_eq!(nav["step"], get_json(&v, "/api/locate?loc=fact.c:5@3")["step"]);
    assert_eq!(nav["loc"], "fact+0x1a");
    assert!(nav["asm"].is_string());
    let last = get_json(&v, "/api/locate?loc=fact.c:4@5")["step"].as_u64().unwrap();
    let none = get_json(&v, &format!("/api/nav?from={last}&op=line-next&file=fact.c&line=4"));
    assert_eq!(none, serde_json::json!({"step": null, "loc": null, "asm": null}));
    assert_eq!(http_request(&v, "GET", "/api/nav?from=0&op=line-next&file=fact.c", None, &[], &[]).status, 400);

    let st = get_json(&v, &format!("/api/state/{at}"));
    assert_eq!((st["locator"].as_str(), st["effects"]["mnemonic"].as_str()), (Some("fact+0xc@3"), Some("cmp")));
    assert!(st.get("writes").is_none() && st.get("event").is_none());
}
