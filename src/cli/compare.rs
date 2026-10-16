use std::path::Path;

use anyhow::Result;

use super::{load, with_stdout};
use crate::model::compare::{Comparison, EventRef, compare as compare_traces};

/// Width of the label column; one more than the longest function name when that is wider.
const LABEL_WIDTH: usize = 22;

fn bytes(n: usize) -> String {
    format!("{n} {}", if n == 1 { "byte" } else { "bytes" })
}

/// A table whose first column is `label` wide and whose other columns are as wide as their widest
/// cell plus two spaces; lines carry no trailing spaces.
fn table(rows: &[Vec<String>], label: usize) -> Vec<String> {
    let cols = rows[0].len();
    let widths: Vec<usize> = (1..cols).map(|c| rows.iter().map(|r| r[c].chars().count()).max().unwrap_or(0) + 2).collect();
    rows.iter()
        .map(|r| {
            let mut line = format!("{:<label$}", r[0]);
            for (c, &w) in widths.iter().enumerate() {
                line.push_str(&format!("{:<w$}", r[c + 1]));
            }
            line.trim_end().to_string()
        })
        .collect()
}

fn row(cells: &[&str]) -> Vec<String> {
    cells.iter().map(|c| c.to_string()).collect()
}

fn side(name: &str, e: &Option<EventRef>) -> String {
    match e {
        Some(e) => format!("{name}: {} at step {} ({})", e.event, e.step, e.locator),
        None => format!("{name}: (ended)"),
    }
}

fn render(c: &Comparison) -> Vec<String> {
    let output = format!("{}, {}", bytes(c.b.output_bytes), if c.output_identical { "identical" } else { "differ" });
    let syscalls = match &c.syscall_diff {
        None => format!("{}, same sequence", c.b.syscalls),
        Some(d) => format!(
            "{}, differs at #{}: A {}, B {}",
            c.b.syscalls,
            d.index,
            d.a.as_deref().unwrap_or("(ended)"),
            d.b.as_deref().unwrap_or("(ended)")
        ),
    };
    let summary = vec![
        row(&["", "A", "B"]),
        row(&["trace", &c.a.trace, &c.b.trace]),
        row(&["steps", &c.a.steps.to_string(), &c.b.steps.to_string()]),
        row(&["end", &c.a.end, &c.b.end]),
        row(&["output", &bytes(c.a.output_bytes), &output]),
        row(&["syscalls", &c.a.syscalls.to_string(), &syscalls]),
    ];
    let mut funcs = vec![row(&["function", "A calls", "A steps", "B calls", "B steps"])];
    for f in &c.functions {
        funcs.push(row(&[&f.name, &f.a_calls.to_string(), &f.a_steps.to_string(), &f.b_calls.to_string(), &f.b_steps.to_string()]));
    }
    if c.other_steps.a + c.other_steps.b > 0 {
        funcs.push(row(&["(libraries, other)", "", &c.other_steps.a.to_string(), "", &c.other_steps.b.to_string()]));
    }
    let label = funcs.iter().map(|r| r[0].chars().count() + 1).fold(LABEL_WIDTH, usize::max);
    let divergence = match &c.divergence {
        None => "none".to_string(),
        Some(d) => format!("#{}  {}  {}", d.index, side("A", &d.a), side("B", &d.b)),
    };
    let mut lines = table(&summary, label);
    lines.push(String::new());
    lines.extend(table(&funcs, label));
    lines.push(String::new());
    lines.push(format!("{:<label$}{divergence}", "first divergence"));
    lines
}

/// `compare A B`: both recordings side by side. Exit 0; a trace that does not load is a usage error (2).
pub fn compare(a: &Path, b: &Path, json: bool) -> Result<u8> {
    let (ta, tb) = (load(a)?, load(b)?);
    let c = compare_traces(&a.display().to_string(), &ta, &b.display().to_string(), &tb);
    with_stdout(|out| {
        if json {
            writeln!(out, "{}", serde_json::to_string(&c)?)?;
        } else {
            for line in render(&c) {
                writeln!(out, "{line}")?;
            }
        }
        Ok(0)
    })
}
