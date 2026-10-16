use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use super::{load, with_stdout};
use crate::exit;
use crate::guide::{Directive, Entry, Kind, parse};
use crate::model::{LoadedTrace, WatchType};

/// What a guide directive resolved to; this is also the `--json` line of the directive.
#[derive(Serialize)]
struct Checked {
    line: usize,
    #[serde(rename = "directive")]
    kind: Kind,
    #[serde(skip)]
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    expr: Option<String>,
    /// The type the value was read as, else the one the guide declared.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    ty: Option<WatchType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    locator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    after: Option<bool>,
    /// The state the directive points at, `after` included.
    #[serde(skip_serializing_if = "Option::is_none")]
    step: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    raw: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl Checked {
    /// The text output after the two padded columns: the step and value, or the error.
    fn outcome(&self) -> String {
        if let Some(e) = &self.error {
            return format!("error: {e}");
        }
        let mut out = format!("step {}", self.step.expect("a directive without an error resolved to a step"));
        match (&self.value, &self.raw) {
            (Some(v), Some(raw)) => out.push_str(&format!("  {v} ({raw})")),
            (Some(v), None) => out.push_str(&format!("  {v}")),
            _ => {}
        }
        out
    }
}

/// `systemslab check-guide`: resolves every directive of `guide` in `trace` and shows what it
/// points at; exits 1 when any directive fails.
pub fn check_guide(trace: &Path, guide: &Path, json: bool) -> Result<u8> {
    let lt = load(trace)?;
    let text = std::fs::read_to_string(guide).map_err(|e| exit::usage(format!("cannot read {}: {e}", guide.display())))?;
    let rows: Vec<Checked> = parse(&text).iter().map(|e| check(&lt, e)).collect();
    let count = |kind: Kind| rows.iter().filter(|r| r.kind == kind).count();
    let (predictions, gos) = (count(Kind::Predict), count(Kind::Go));
    let errors = rows.iter().filter(|r| r.error.is_some()).count();
    with_stdout(|out| {
        if json {
            for r in &rows {
                writeln!(out, "{}", serde_json::to_string(r)?)?;
            }
            writeln!(out, "{}", serde_json::json!({ "predictions": predictions, "gos": gos, "errors": errors }))?;
        } else {
            let places: Vec<String> = rows.iter().map(|r| format!("{}:{}", guide.display(), r.line)).collect();
            let w_place = places.iter().map(|p| p.chars().count()).max().unwrap_or(0) + 2;
            let w_text = rows.iter().map(|r| r.text.chars().count()).max().unwrap_or(0) + 2;
            for (place, r) in places.iter().zip(&rows) {
                writeln!(out, "{place:<w_place$}{:<w_text$}{}", r.text, r.outcome())?;
            }
            writeln!(out, "{predictions} predictions, {gos} go, {errors} errors")?;
        }
        Ok(if errors == 0 { 0 } else { exit::FINDINGS })
    })
}

fn check(lt: &LoadedTrace, entry: &Entry) -> Checked {
    let mut row = Checked {
        line: entry.line,
        kind: entry.kind,
        text: entry.text.clone(),
        expr: None,
        ty: None,
        locator: None,
        after: None,
        step: None,
        value: None,
        raw: None,
        error: None,
    };
    match &entry.directive {
        Err(message) => row.error = Some(message.clone()),
        Ok(Directive::Go { locator }) => {
            row.locator = Some(locator.clone());
            match lt.locate(locator) {
                Ok(step) => row.step = Some(step),
                Err(e) => row.error = Some(format!("{e:#}")),
            }
        }
        Ok(Directive::Predict { expr, ty, locator, after, .. }) => {
            (row.expr, row.ty, row.locator, row.after) = (Some(expr.clone()), *ty, Some(locator.clone()), Some(*after));
            if let Err(message) = predict(lt, &mut row, expr, *ty, locator, *after) {
                row.error = Some(message);
            }
        }
    }
    row
}

/// Reads the value a prediction asks about into `row`.
fn predict(lt: &LoadedTrace, row: &mut Checked, expr: &str, ty: Option<WatchType>, locator: &str, after: bool) -> Result<(), String> {
    let step = lt.locate(locator).map_err(|e| format!("{e:#}"))?;
    let target = u64::from(step) + u64::from(after);
    if target > u64::from(lt.steps()) {
        return Err(format!("after: step {target} is past the end of the recording"));
    }
    row.step = Some(target as u32);
    let w = lt.watch(target as u32, &[(expr.to_string(), ty)]).swap_remove(0);
    if let Some(e) = w.error {
        return Err(e);
    }
    row.ty = Some(w.ty);
    let Some(value) = w.value else {
        return Err(format!("nothing is readable at {}", w.addr.as_deref().unwrap_or("that address")));
    };
    (row.value, row.raw) = (Some(value), w.raw);
    Ok(())
}
