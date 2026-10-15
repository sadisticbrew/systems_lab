//! The viewer: an HTTP API over a `LoadedTrace` plus the embedded web frontend.

use std::collections::HashMap;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use rust_embed::RustEmbed;
use serde_json::{Value, json};

use crate::exit;
use crate::model::{LoadedTrace, NavOp};

#[derive(RustEmbed)]
#[folder = "web/dist"]
struct Assets;

type Shared = Arc<LoadedTrace>;
type Params = Query<HashMap<String, String>>;

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

fn step_param(t: &LoadedTrace, p: &HashMap<String, String>, key: &str) -> Result<u32, ApiError> {
    let s: u32 = number(p, key)?;
    check_step(t, s)?;
    Ok(s)
}

fn check_step(t: &LoadedTrace, s: u32) -> Result<(), ApiError> {
    if s > t.steps() {
        return Err(ApiError(StatusCode::NOT_FOUND, format!("step {s} is outside 0..={}", t.steps())));
    }
    Ok(())
}

async fn info(State(t): State<Shared>) -> ApiResult {
    ok(t.api_info())
}

async fn timeline(State(t): State<Shared>) -> ApiResult {
    ok(t.timeline())
}

async fn state(State(t): State<Shared>, UrlPath(s): UrlPath<String>) -> ApiResult {
    let s: u32 = s.parse().map_err(|_| bad(format!("bad step '{s}'")))?;
    check_step(&t, s)?;
    ok(t.state_view(s).map_err(from_model)?)
}

async fn mem(State(t): State<Shared>, Query(p): Params) -> ApiResult {
    let addr = hex_addr(&p)?;
    let len: usize = number(&p, "len")?;
    let s = step_param(&t, &p, "step")?;
    ok(t.mem_view(addr, len, s).map_err(from_model)?)
}

async fn disasm(State(t): State<Shared>, Query(p): Params) -> ApiResult {
    let addr = hex_addr(&p)?;
    let s = step_param(&t, &p, "step")?;
    ok(t.disasm_view(addr, s).map_err(from_model)?)
}

async fn source(State(t): State<Shared>, Query(p): Params) -> ApiResult {
    let file = required(&p, "file")?;
    match t.source_view(file) {
        Some(v) => ok(v),
        None => Err(ApiError(StatusCode::NOT_FOUND, format!("no embedded source '{file}'"))),
    }
}

async fn nav(State(t): State<Shared>, Query(p): Params) -> ApiResult {
    let from = step_param(&t, &p, "from")?;
    let op = NavOp::parse(required(&p, "op")?, p.get("reg").map(String::as_str), p.get("addr").map(String::as_str))
        .map_err(from_model)?;
    ok(json!({ "step": t.nav(from, &op).map_err(from_model)? }))
}

async fn locate(State(t): State<Shared>, Query(p): Params) -> ApiResult {
    let step = t.locate(required(&p, "loc")?).map_err(from_model)?;
    ok(json!({ "step": step }))
}

async fn abi(State(t): State<Shared>) -> ApiResult {
    ok(json!({ "findings": t.abi_findings(false) }))
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

/// The router serving the API for `trace` and the embedded frontend.
pub fn router(trace: Arc<LoadedTrace>) -> Router {
    Router::new()
        .route("/api/info", get(info))
        .route("/api/timeline", get(timeline))
        .route("/api/state/{s}", get(state))
        .route("/api/mem", get(mem))
        .route("/api/disasm", get(disasm))
        .route("/api/source", get(source))
        .route("/api/nav", get(nav))
        .route("/api/locate", get(locate))
        .route("/api/abi", get(abi))
        .route("/api/{*rest}", get(api_missing))
        .fallback(assets)
        .with_state(trace)
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

/// `systemslab view`: serves the trace in the foreground until Ctrl-C.
pub fn view(path: &Path, port: u16, no_open: bool) -> Result<u8> {
    let file = resolve_trace(path)?;
    let trace = Arc::new(LoadedTrace::load(&file)?);
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().context("starting the async runtime")?;
    rt.block_on(async {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| exit::usage(format!("cannot bind {addr}: {e}")))?;
        let url = format!("http://{}", listener.local_addr()?);
        println!("SystemsLab viewer: {url}  (Ctrl-C to stop)");
        std::io::stdout().flush()?;
        if !no_open {
            let _ = std::process::Command::new("xdg-open")
                .arg(&url)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
        axum::serve(listener, router(trace))
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await?;
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(0)
}
