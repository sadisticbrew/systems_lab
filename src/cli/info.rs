use std::path::Path;

use anyhow::Result;

use super::{load, with_stdout};
use crate::model::NameCount;

fn counts(list: &[NameCount]) -> String {
    if list.is_empty() {
        return "none".to_string();
    }
    list.iter().map(|c| format!("{}×{}", c.name, c.count)).collect::<Vec<_>>().join(", ")
}

pub fn info(trace: &Path, json: bool) -> Result<u8> {
    let v = load(trace)?.info_view();
    with_stdout(|out| {
        if json {
            writeln!(out, "{}", serde_json::to_string(&v)?)?;
        } else {
            let mut line = |k: &str, val: String| writeln!(out, "{k:<20}{val}");
            line("program", v.program.clone())?;
            line("argv", v.argv.join(" "))?;
            line("from", v.from.clone())?;
            line("steps", v.steps.to_string())?;
            line("end", v.end.to_string())?;
            line("aslr", v.aslr.to_string())?;
            line("unattributed bytes", v.unattributed_bytes.to_string())?;
            line("syscalls", counts(&v.syscalls))?;
            line("signals", counts(&v.signals))?;
        }
        Ok(0)
    })
}
