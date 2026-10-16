//! The viewer: an HTTP API over a `LoadedTrace` plus the embedded web frontend.
//!
//! The served trace can be replaced while the server runs: [`watch`] reloads it when the trace
//! file changes (`view`) or rebuilds and re-records the source when it changes (`run`), then bumps
//! the generation that `/api/status` reports.

mod watch;

use std::collections::HashMap;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};

use anyhow::{Context, Result};
use axum::Router;
use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{DefaultBodyLimit, FromRequestParts, Path as UrlPath, Query, Request, State};
use axum::http::request::Parts;
use axum::http::{StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use rust_embed::RustEmbed;
use serde::Serialize;
use serde_json::{Value, json};

use crate::model::{ApiInfo, LoadedTrace, NavOp, NavParams, WatchType};
use crate::{exit, paths, run};

pub use watch::Stamp;

#[derive(RustEmbed)]
#[folder = "web/dist"]
struct Assets;

type Params = Query<HashMap<String, String>>;

/// Largest request body: notes and preferences are small JSON objects.
const MAX_BODY: usize = 1 << 20;
/// Longest compiler message kept in `status.error`, in characters.
const MAX_ERROR_CHARS: usize = 4000;

/// How the viewer got its trace.
pub enum Mode {
    /// `systemslab view`: serve a trace file and reload it when the file changes.
    View,
    /// `systemslab run`: serve the recording of `source`, rebuilt and re-recorded when it changes.
    Run {
        source: PathBuf,
        cfg: run::Config,
        /// Rebuild when `source` changes (`--no-watch` clears it).
        watch: bool,
        /// `source`'s stamp from before the first build, so a save during that build is not missed.
        built_from: Option<Stamp>,
    },
}

/// What `serve` needs to start a viewer.
pub struct ServeOptions {
    /// The `.slt` trace to serve (for `Mode::Run`, the file `cfg` writes).
    pub trace_path: PathBuf,
    /// Markdown guide shown in the viewer's Guide tab.
    pub guide: Option<PathBuf>,
    /// A locator the printed and opened URL jumps to (`#step=N`).
    pub at: Option<String>,
    /// Port to listen on; 0 picks a free one.
    pub port: u16,
    pub no_open: bool,
    pub mode: Mode,
}

/// `/api/status`: lets the viewer notice that the recording behind it was replaced.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Status {
    pub building: bool,
    pub error: Option<String>,
}

#[derive(Serialize)]
struct StatusView {
    generation: u64,
    #[serde(flatten)]
    status: Status,
}

/// `/api/info`: the trace summary plus where the server keeps its files.
#[derive(Serialize)]
struct ServerInfo {
    #[serde(flatten)]
    info: ApiInfo,
    trace_path: String,
    notes_path: String,
    mode: &'static str,
    source_path: Option<String>,
    has_guide: bool,
}

/// Shared server state.
struct App {
    trace: RwLock<Arc<LoadedTrace>>,
    /// Starts at 1; bumped each time `trace` is replaced.
    generation: AtomicU64,
    status: Mutex<Status>,
    /// Canonical path of the trace file.
    trace_path: PathBuf,
    notes_path: PathBuf,
    prefs_path: PathBuf,
    guide: Option<PathBuf>,
    mode: Mode,
}

impl App {
    /// The trace being served right now. Requests work on this snapshot even if it is replaced meanwhile.
    fn trace(&self) -> Arc<LoadedTrace> {
        self.trace.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn status(&self) -> MutexGuard<'_, Status> {
        self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn status_view(&self) -> StatusView {
        let status = self.status();
        StatusView { generation: self.generation.load(Ordering::SeqCst), status: status.clone() }
    }

    /// Serves `trace` from now on and clears the error.
    fn publish(&self, trace: LoadedTrace) {
        let mut status = self.status();
        *self.trace.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(trace);
        self.generation.fetch_add(1, Ordering::SeqCst);
        *status = Status::default();
    }

    /// Records why the trace could not be replaced; the old one keeps being served.
    fn fail(&self, error: String) {
        *self.status() = Status { building: false, error: Some(error) };
    }

    fn mode_name(&self) -> &'static str {
        match self.mode {
            Mode::View => "view",
            Mode::Run { .. } => "run",
        }
    }
}

/// An API error rendered as `{"error": "..."}`.
struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, axum::Json(json!({ "error": self.1 }))).into_response()
    }
}

type ApiResult = Result<axum::Json<Value>, ApiError>;

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

fn from_model(err: anyhow::Error) -> ApiError {
    bad(format!("{err:#}"))
}

fn ok<T: serde::Serialize>(v: T) -> ApiResult {
    serde_json::to_value(v).map(axum::Json).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

fn required<'a>(p: &'a HashMap<String, String>, key: &str) -> Result<&'a str, ApiError> {
    p.get(key).map(String::as_str).ok_or_else(|| bad(format!("missing parameter '{key}'")))
}

fn number<T: std::str::FromStr>(p: &HashMap<String, String>, key: &str) -> Result<T, ApiError> {
    let v = required(p, key)?;
    v.parse().map_err(|_| bad(format!("bad {key} '{v}'")))
}

fn hex_addr(p: &HashMap<String, String>) -> Result<u64, ApiError> {
    let a = required(p, "addr")?;
    u64::from_str_radix(a.strip_prefix("0x").unwrap_or(a), 16).map_err(|_| bad(format!("bad address '{a}'")))
}

/// Parses a step number. One too big for `u32` is out of range (404), other text a bad parameter (400).
fn parse_step(t: &LoadedTrace, raw: &str, what: &str) -> Result<u32, ApiError> {
    match raw.parse::<u32>() {
        Ok(s) => check_step(t, s).map(|()| s),
        Err(_) if !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit()) => {
            Err(ApiError(StatusCode::NOT_FOUND, format!("step {raw} is outside 0..={}", t.steps())))
        }
        Err(_) => Err(bad(format!("bad {what} '{raw}'"))),
    }
}

fn step_param(t: &LoadedTrace, p: &HashMap<String, String>, key: &str) -> Result<u32, ApiError> {
    parse_step(t, required(p, key)?, key)
}

fn check_step(t: &LoadedTrace, s: u32) -> Result<(), ApiError> {
    if s > t.steps() {
        return Err(ApiError(StatusCode::NOT_FOUND, format!("step {s} is outside 0..={}", t.steps())));
    }
    Ok(())
}

async fn info(State(app): State<Arc<App>>) -> ApiResult {
    let lossy = |p: &Path| p.to_string_lossy().into_owned();
    ok(ServerInfo {
        info: app.trace().api_info(),
        trace_path: lossy(&app.trace_path),
        notes_path: lossy(&app.notes_path),
        mode: app.mode_name(),
        source_path: match &app.mode {
            Mode::Run { source, .. } => Some(lossy(source)),
            Mode::View => None,
        },
        has_guide: app.guide.is_some(),
    })
}

async fn status(State(app): State<Arc<App>>) -> ApiResult {
    ok(app.status_view())
}

async fn timeline(State(app): State<Arc<App>>, Query(p): Params) -> ApiResult {
    let t = app.trace();
    let from = p.get("from").map_or(Ok(0), |v| parse_step(&t, v, "from"))?;
    let to = p.get("to").map_or(Ok(t.steps()), |v| parse_step(&t, v, "to"))?;
    ok(t.timeline(from, to).map_err(from_model)?)
}

async fn state(State(app): State<Arc<App>>, UrlPath(s): UrlPath<String>) -> ApiResult {
    let t = app.trace();
    let s = parse_step(&t, &s, "step")?;
    ok(t.state_view(s).map_err(from_model)?)
}

async fn mem(State(app): State<Arc<App>>, Query(p): Params) -> ApiResult {
    let t = app.trace();
    let addr = hex_addr(&p)?;
    let len: usize = number(&p, "len")?;
    let s = step_param(&t, &p, "step")?;
    ok(t.mem_view(addr, len, s).map_err(from_model)?)
}

async fn disasm(State(app): State<Arc<App>>, Query(p): Params) -> ApiResult {
    let t = app.trace();
    let addr = hex_addr(&p)?;
    let s = step_param(&t, &p, "step")?;
    ok(t.disasm_view(addr, s).map_err(from_model)?)
}

async fn source(State(app): State<Arc<App>>, Query(p): Params) -> ApiResult {
    let file = required(&p, "file")?;
    match app.trace().source_view(file) {
        Some(v) => ok(v),
        None => Err(ApiError(StatusCode::NOT_FOUND, format!("no embedded source '{file}'"))),
    }
}

async fn nav(State(app): State<Arc<App>>, Query(p): Params) -> ApiResult {
    let t = app.trace();
    let from = step_param(&t, &p, "from")?;
    let param = |key: &str| p.get(key).map(String::as_str);
    let params = NavParams { reg: param("reg"), addr: param("addr"), file: param("file"), line: param("line") };
    let op = NavOp::parse(required(&p, "op")?, &params).map_err(from_model)?;
    ok(t.nav_view(from, &op).map_err(from_model)?)
}

async fn events(State(app): State<Arc<App>>) -> ApiResult {
    ok(json!({ "events": app.trace().events() }))
}

async fn eval_expr(State(app): State<Arc<App>>, Query(p): Params) -> ApiResult {
    let t = app.trace();
    let s = step_param(&t, &p, "step")?;
    ok(t.eval(s, required(&p, "expr")?).map_err(from_model)?)
}

async fn inspect(State(app): State<Arc<App>>, Query(p): Params) -> ApiResult {
    let t = app.trace();
    let addr = hex_addr(&p)?;
    let s = step_param(&t, &p, "step")?;
    ok(t.inspect(s, addr).map_err(from_model)?)
}

/// `GET /api/watch?step=S&w=EXPR|TYPE&w=…`: `w` repeats, `|TYPE` is optional.
async fn watches(State(app): State<Arc<App>>, Query(p): Query<Vec<(String, String)>>) -> ApiResult {
    let t = app.trace();
    let step = p.iter().find(|(k, _)| k == "step").map(|(_, v)| v.as_str()).ok_or_else(|| bad("missing parameter 'step'"))?;
    let s = parse_step(&t, step, "step")?;
    let items = p
        .iter()
        .filter(|(k, _)| k == "w")
        .map(|(_, v)| match v.rsplit_once('|') {
            Some((expr, ty)) => Ok((expr.to_string(), Some(ty.parse::<WatchType>().map_err(bad)?))),
            None => Ok((v.clone(), None)),
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    ok(json!({ "watches": t.watch(s, &items) }))
}

async fn locate(State(app): State<Arc<App>>, Query(p): Params) -> ApiResult {
    let step = app.trace().locate(required(&p, "loc")?).map_err(from_model)?;
    ok(json!({ "step": step }))
}

async fn abi(State(app): State<Arc<App>>) -> ApiResult {
    ok(json!({ "findings": app.trace().abi_findings(false) }))
}

/// `GET /api/guide`: the guide file, read fresh so edits show up on the next fetch.
async fn guide(State(app): State<Arc<App>>) -> ApiResult {
    let Some(path) = &app.guide else {
        return Err(ApiError(StatusCode::NOT_FOUND, "no guide".into()));
    };
    let markdown = std::fs::read_to_string(path)
        .map_err(|e| ApiError(StatusCode::NOT_FOUND, format!("cannot read guide {}: {e}", path.display())))?;
    ok(json!({ "path": path.to_string_lossy(), "markdown": markdown }))
}

/// Rejects a request without `X-SystemsLab: 1`: a custom header cannot be sent cross-origin
/// without a CORS preflight the server never answers, so web pages cannot write the viewer's files.
struct Marked;

impl<S: Send + Sync> FromRequestParts<S> for Marked {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, ApiError> {
        match parts.headers.get("x-systemslab") {
            Some(v) if v == "1" => Ok(Marked),
            _ => Err(ApiError(StatusCode::FORBIDDEN, "missing X-SystemsLab header".into())),
        }
    }
}

/// The JSON object stored in `path`, or `{}` when there is no file yet.
fn read_object(path: &Path) -> ApiResult {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return ok(json!({})),
        Err(e) => return Err(ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("cannot read {}: {e}", path.display()))),
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(v @ Value::Object(_)) => ok(v),
        _ => Err(ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{} is not a JSON object; fix or delete it", path.display()),
        )),
    }
}

/// Serializes writes to the notes and preferences files.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Validates `body` as a JSON object and stores it atomically at `path`.
fn write_object(path: &Path, body: Result<Bytes, BytesRejection>) -> Result<StatusCode, ApiError> {
    let body = body.map_err(|e| ApiError(e.status(), format!("cannot read the request body: {}", e.body_text())))?;
    match serde_json::from_slice::<Value>(&body) {
        Ok(Value::Object(_)) => {}
        Ok(_) => return Err(bad("body must be a JSON object")),
        Err(e) => return Err(bad(format!("body is not valid JSON: {e}"))),
    }
    let _guard = WRITE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let stored = match path.parent() {
        Some(dir) => std::fs::create_dir_all(dir),
        None => Ok(()),
    }
    .and_then(|()| paths::write_atomic(path, &body));
    stored.map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("cannot write {}: {e}", path.display())))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_notes(State(app): State<Arc<App>>) -> ApiResult {
    read_object(&app.notes_path)
}

async fn put_notes(State(app): State<Arc<App>>, _: Marked, body: Result<Bytes, BytesRejection>) -> Result<StatusCode, ApiError> {
    write_object(&app.notes_path, body)
}

async fn get_prefs(State(app): State<Arc<App>>) -> ApiResult {
    read_object(&app.prefs_path)
}

async fn put_prefs(State(app): State<Arc<App>>, _: Marked, body: Result<Bytes, BytesRejection>) -> Result<StatusCode, ApiError> {
    write_object(&app.prefs_path, body)
}

async fn api_missing() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "no such API route".into())
}

async fn assets(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Assets::get(path) {
        Some(file) => {
            let mime = file.metadata.mimetype().to_string();
            ([(header::CONTENT_TYPE, mime)], file.data.into_owned()).into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

/// Answers 403 unless the `Host` header names this server: a web page on another site that
/// resolves its own name to 127.0.0.1 (DNS rebinding) must not read the recording or the notes.
async fn check_host(State(hosts): State<Arc<[String; 2]>>, req: Request, next: Next) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .or_else(|| req.uri().authority().map(|a| a.as_str()));
    if host.is_some_and(|h| hosts.iter().any(|allowed| allowed.eq_ignore_ascii_case(h))) {
        next.run(req).await
    } else {
        ApiError(StatusCode::FORBIDDEN, "bad Host header".into()).into_response()
    }
}

/// The router serving the API and the embedded frontend of `app`, listening on `port`.
fn router(app: Arc<App>, port: u16) -> Router {
    let hosts = Arc::new([format!("127.0.0.1:{port}"), format!("localhost:{port}")]);
    Router::new()
        .route("/api/info", get(info))
        .route("/api/status", get(status))
        .route("/api/timeline", get(timeline))
        .route("/api/state/{s}", get(state))
        .route("/api/mem", get(mem))
        .route("/api/disasm", get(disasm))
        .route("/api/source", get(source))
        .route("/api/nav", get(nav))
        .route("/api/locate", get(locate))
        .route("/api/abi", get(abi))
        .route("/api/events", get(events))
        .route("/api/eval", get(eval_expr))
        .route("/api/inspect", get(inspect))
        .route("/api/watch", get(watches))
        .route("/api/guide", get(guide))
        .route("/api/notes", get(get_notes).put(put_notes))
        .route("/api/prefs", get(get_prefs).put(put_prefs))
        .route("/api/{*rest}", get(api_missing))
        .fallback(assets)
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(middleware::from_fn_with_state(hosts, check_host))
        .with_state(app)
}

/// Resolves `PATH` of `view`: a `.slt` file, or a lab directory with a single `out/*.slt`.
pub fn resolve_trace(path: &Path) -> Result<PathBuf> {
    if !path.is_dir() {
        return Ok(path.to_path_buf());
    }
    let out = path.join("out");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&out)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "slt")).collect())
        .unwrap_or_default();
    found.sort();
    match found.len() {
        0 => Err(exit::usage(format!("no trace in {}; run tools/lab.py run first", out.display()))),
        1 => Ok(found.remove(0)),
        _ => {
            let list: Vec<String> = found.iter().map(|p| format!("  {}", p.display())).collect();
            Err(exit::usage(format!("several traces in {}; pass one of:\n{}", out.display(), list.join("\n"))))
        }
    }
}

/// The guide `view` shows when `--guide` is absent: `PATH/guide.md` for a lab directory, else
/// `<trace stem>.guide.md` next to the trace.
pub fn discover_guide(path: &Path, trace: &Path) -> Option<PathBuf> {
    let lab = path.join("guide.md");
    if path.is_dir() && lab.is_file() {
        return Some(lab);
    }
    let beside = trace.with_extension("guide.md");
    beside.is_file().then_some(beside)
}

/// `systemslab view` and `systemslab run`: serves the trace in the foreground until Ctrl-C.
pub fn serve(o: ServeOptions) -> Result<u8> {
    // The watched file and its stamp from before the content being served was produced.
    let watched = match &o.mode {
        Mode::View => Some((o.trace_path.clone(), Stamp::of(&o.trace_path))),
        Mode::Run { source, watch: true, built_from, .. } => Some((source.clone(), *built_from)),
        Mode::Run { .. } => None,
    };
    let trace = LoadedTrace::load(&o.trace_path)?;
    let step = o.at.as_deref().map(|at| trace.locate(at)).transpose()?;
    let guide = match o.guide {
        Some(g) if !g.is_file() => return Err(exit::usage(format!("cannot read guide {}", g.display()))),
        Some(g) => Some(std::path::absolute(g)?),
        None => None,
    };
    let trace_path = std::fs::canonicalize(&o.trace_path).with_context(|| format!("resolving {}", o.trace_path.display()))?;
    let notes_path = paths::notes_file(&trace_path)?;
    let prefs_path = paths::prefs_file()?;
    let app = Arc::new(App {
        trace: RwLock::new(Arc::new(trace)),
        generation: AtomicU64::new(1),
        status: Mutex::new(Status::default()),
        trace_path,
        notes_path,
        prefs_path,
        guide,
        mode: o.mode,
    });

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().context("starting the async runtime")?;
    let served = rt.block_on(async {
        let addr = SocketAddr::from(([127, 0, 0, 1], o.port));
        let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| exit::usage(format!("cannot bind {addr}: {e}")))?;
        let local = listener.local_addr()?;
        if let Some((path, seen)) = watched {
            tokio::spawn(watch::watch(app.clone(), path, seen));
        }
        let mut url = format!("http://{local}");
        if let Some(step) = step {
            url.push_str(&format!("/#step={step}"));
        }
        println!("SystemsLab viewer: {url}  (Ctrl-C to stop)");
        std::io::stdout().flush()?;
        if !o.no_open {
            let _ = std::process::Command::new("xdg-open")
                .arg(&url)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
        axum::serve(listener, router(app, local.port()))
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await?;
        Ok::<(), anyhow::Error>(())
    });
    // A rebuild may still be running on the blocking pool; do not wait for it.
    rt.shutdown_background();
    served?;
    Ok(0)
}
