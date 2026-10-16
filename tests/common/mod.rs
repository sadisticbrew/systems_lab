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

/// A running `systemslab view` or `systemslab run`, killed on drop. Its XDG directories live in
/// a temp dir of its own, so notes and preferences never touch the real home directory.
pub struct Viewer {
    child: std::process::Child,
    pub port: u16,
    /// The URL the server printed, including any `#step=N`.
    pub url: String,
    pub xdg: TempDir,
}

impl Viewer {
    /// `$XDG_DATA_HOME` of this viewer.
    pub fn data_home(&self) -> PathBuf {
        self.xdg.path().join("data")
    }

    /// `$XDG_CONFIG_HOME` of this viewer.
    pub fn config_home(&self) -> PathBuf {
        self.xdg.path().join("config")
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Runs `systemslab <args>` (`view` or `run`, with `--port 0 --no-open` added by the caller) with
/// private XDG directories, waits for its `SystemsLab viewer: URL` line and returns the handle.
pub fn start_viewer_with(args: &[&str]) -> Viewer {
    use std::io::BufRead;
    let xdg = tempfile::tempdir().expect("tempdir");
    let mut child = Command::new(env!("CARGO_BIN_EXE_systemslab"))
        .args(args)
        .env("XDG_DATA_HOME", xdg.path().join("data"))
        .env("XDG_CONFIG_HOME", xdg.path().join("config"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn systemslab");
    let mut reader = std::io::BufReader::new(child.stdout.take().unwrap());
    let url = loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).expect("read the viewer's stdout");
        assert!(n > 0, "systemslab {args:?} exited before printing its URL");
        if let Some(rest) = line.strip_prefix("SystemsLab viewer: ") {
            break rest.split_whitespace().next().unwrap().to_string();
        }
    };
    // The server writes nothing more to stdout; keep the pipe open so it never sees a closed reader.
    child.stdout = Some(reader.into_inner());
    let port = url.split("127.0.0.1:").nth(1).map(|r| r.chars().take_while(char::is_ascii_digit).collect::<String>())
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("no port in {url}"));
    Viewer { child, port, url, xdg }
}

pub fn start_viewer(trace: &Path) -> Viewer {
    start_viewer_with(&["view", p(trace), "--no-open", "--port", "0"])
}

/// An HTTP response: status code, raw header block and body.
pub struct Response {
    pub status: u16,
    pub headers: String,
    pub body: String,
}

impl Response {
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("not JSON ({e}): {}", self.body))
    }
}

/// One HTTP/1.1 request over a fresh connection. `host` overrides the `Host` header (default:
/// the viewer's own address). Fails if the server stays silent for 20 s.
pub fn http_request(v: &Viewer, method: &str, path: &str, host: Option<&str>, headers: &[(&str, &str)], body: &[u8]) -> Response {
    use std::io::Read;
    let mut s = std::net::TcpStream::connect(("127.0.0.1", v.port)).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(20))).unwrap();
    let host = host.map_or_else(|| format!("127.0.0.1:{}", v.port), str::to_string);
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
    for (k, val) in headers {
        req.push_str(&format!("{k}: {val}\r\n"));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).unwrap();
    s.write_all(body).unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).unwrap_or_else(|e| panic!("{method} {path}: {e}"));
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or_else(|| panic!("{method} {path}: no response"));
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    Response { status, headers: head.to_string(), body: body.to_string() }
}

/// One `GET` over a fresh connection: status code and body.
pub fn http_get(v: &Viewer, path: &str) -> (u16, String) {
    let r = http_request(v, "GET", path, None, &[], &[]);
    (r.status, r.body)
}
