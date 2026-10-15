use std::path::Path;

use anyhow::Result;

use super::{load, with_stdout};
use crate::model::StackView;

fn value_col(label: &str, value: &str) -> String {
    if label == "unused" { "-".to_string() } else { value.to_string() }
}

pub fn render_text(v: &StackView) -> String {
    let slots = || v.frames.iter().flat_map(|f| f.slots.iter());
    let lw = slots().map(|s| s.label.chars().count()).max().unwrap_or(0);
    let vw = slots().map(|s| value_col(&s.label, &s.value).chars().count()).max().unwrap_or(0);
    let mut out = format!("step {} at {} ({})\n", v.step, v.loc, v.asm);
    for f in &v.frames {
        if f.collapsed {
            out.push_str(&format!("{}'s frame\n", f.label));
            continue;
        }
        out.push_str(&format!("{}, {} bytes\n", f.label, f.size.unwrap_or(0)));
        for s in &f.slots {
            let mut line = format!(
                "  {:>10}  {:<lw$}  {:<vw$}",
                s.rbp_off.as_deref().unwrap_or(""),
                s.label,
                value_col(&s.label, &s.value)
            );
            if !s.pointers.is_empty() {
                line.push_str(&format!("  ← {}", s.pointers.join(", ")));
            }
            out.push_str(line.trim_end());
            out.push('\n');
        }
    }
    out
}

pub fn stack(trace: &Path, loc: &str, frames: Option<usize>, json: bool) -> Result<u8> {
    let lt = load(trace)?;
    let s = lt.locate(loc)?;
    let v = lt.stack_view(s, frames);
    with_stdout(|out| {
        if json {
            writeln!(out, "{}", serde_json::to_string(&v)?)?;
        } else {
            write!(out, "{}", render_text(&v))?;
        }
        Ok(0)
    })
}
