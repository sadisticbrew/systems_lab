use std::path::Path;

use anyhow::Result;

use super::{load, with_stdout};
use crate::exit;

pub fn check_abi(trace: &Path, all: bool, json: bool) -> Result<u8> {
    let lt = load(trace)?;
    let findings = lt.abi_findings(all);
    with_stdout(|out| {
        for f in &findings {
            if json {
                writeln!(out, "{}", serde_json::to_string(f)?)?;
            } else {
                writeln!(out, "{} step {} {}: {}", f.rule, f.step, f.loc, f.message)?;
            }
        }
        if json {
            writeln!(out, "{}", serde_json::json!({ "findings": findings.len() }))?;
        } else if findings.is_empty() {
            writeln!(out, "no findings")?;
        } else {
            writeln!(out, "{} findings", findings.len())?;
        }
        Ok(if findings.is_empty() { 0 } else { exit::FINDINGS })
    })
}
