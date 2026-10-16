//! Keeps the served trace current: reloads it when its file changes (`view`) or rebuilds and
//! re-records the source when that changes (`run`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::Result;
use tokio::time::{MissedTickBehavior, interval};

use super::{App, MAX_ERROR_CHARS, Mode};
use crate::model::LoadedTrace;
use crate::{record, run};

/// How often the watched file is checked.
const POLL: Duration = Duration::from_millis(500);

/// A file's modification time and length: when either differs, the file was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    mtime: SystemTime,
    len: u64,
}

impl Stamp {
    /// The current stamp of `path`; `None` while it does not exist.
    pub fn of(path: &Path) -> Option<Stamp> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Stamp { mtime: meta.modified().ok()?, len: meta.len() })
    }
}

/// Polls `path` (the trace in `view`, the source in `run`) and refreshes the app when it changes.
/// `seen` is its stamp from before the content in `app` was produced.
///
/// A change counts once the file shows the same new stamp on two consecutive polls, so a writer
/// that is still busy is not read half done. A change made while a rebuild runs shows up in the
/// poll after it and starts one more rebuild.
pub async fn watch(app: Arc<App>, path: PathBuf, mut seen: Option<Stamp>) {
    let mut tick = interval(POLL);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut pending: Option<Stamp> = None;
    loop {
        tick.tick().await;
        // A file that is briefly missing (replaced by delete + create) is waited for.
        let Some(now) = Stamp::of(&path) else {
            pending = None;
            continue;
        };
        if Some(now) == seen {
            pending = None;
        } else if pending != Some(now) {
            pending = Some(now);
        } else {
            pending = None;
            seen = Some(now);
            match &app.mode {
                Mode::View => reload(&app, &path).await,
                Mode::Run { source, cfg, .. } => rebuild(&app, source, cfg).await,
            }
        }
    }
}

/// `view`: loads the replaced trace file from `path`, the path `view` was given (a symlink there
/// may now point elsewhere).
async fn reload(app: &Arc<App>, path: &Path) {
    let file = path.to_path_buf();
    let loaded = tokio::task::spawn_blocking(move || LoadedTrace::load(&file)).await;
    match loaded.map_err(anyhow::Error::from).and_then(|r| r) {
        Ok(trace) => {
            eprintln!("reloaded {} ({} steps)", app.trace_path.display(), trace.steps());
            app.publish(trace);
        }
        Err(err) => {
            eprintln!("reload failed: {err:#}");
            app.fail(format!("{err:#}"));
        }
    }
}

/// `run`: compiles and records the changed source, serving the new recording on success and
/// keeping the old one (plus the error) on failure.
async fn rebuild(app: &Arc<App>, source: &Path, cfg: &run::Config) {
    app.status().building = true;
    let (source_owned, cfg_owned) = (source.to_path_buf(), cfg.clone());
    let built = tokio::task::spawn_blocking(move || -> Result<(LoadedTrace, String)> {
        let built = run::build_and_record(&source_owned, &cfg_owned)?;
        let end = record::end_text(&built.trace.meta.end);
        let trace = LoadedTrace::from_trace(built.trace)?;
        let line = format!("rebuilt {} ({} steps; ended: {end})", source_owned.display(), trace.steps());
        Ok((trace, line))
    })
    .await;
    match built.map_err(anyhow::Error::from).and_then(|r| r) {
        Ok((trace, line)) => {
            eprintln!("{line}");
            app.publish(trace);
        }
        Err(err) => {
            let message = match err.downcast_ref::<run::CompileError>() {
                Some(c) => c.output.chars().take(MAX_ERROR_CHARS).collect(),
                None => format!("{err:#}"),
            };
            eprintln!("build failed: {message}");
            app.fail(message);
        }
    }
}
