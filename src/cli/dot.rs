use std::fmt::Write as _;
use std::path::Path;

use anyhow::Result;

use super::{load, with_stdout};
use crate::model::{RootFrame, StackView};

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

const INNER: &str = r#"<TABLE BORDER="0" CELLBORDER="1" CELLSPACING="0" CELLPADDING="3">"#;

/// Graphviz text for a stack view, in the layout of the hand-drawn chapter figure.
pub fn render(v: &StackView) -> String {
    let mut out = String::new();
    out.push_str("digraph { node [shape=plaintext];\n");
    out.push_str("  t [label=<<TABLE BORDER=\"0\" CELLBORDER=\"0\" CELLSPACING=\"0\" CELLPADDING=\"2\">\n");
    for f in &v.frames {
        if f.collapsed {
            let _ = writeln!(out, "    <TR><TD></TD><TD>{INNER}");
            let _ = writeln!(
                out,
                "      <TR><TD WIDTH=\"160\">{}'s frame</TD><TD BORDER=\"0\" WIDTH=\"50\"></TD></TR></TABLE></TD></TR>",
                esc(&f.label)
            );
            continue;
        }
        let _ = writeln!(
            out,
            "    <TR><TD ALIGN=\"RIGHT\">{},<BR/>{} bytes</TD><TD>{INNER}",
            esc(&f.label),
            f.size.unwrap_or(0)
        );
        for (i, s) in f.slots.iter().enumerate() {
            let first = i == 0;
            let width = if first { " WIDTH=\"160\"" } else { "" };
            let cell_width = if first { " WIDTH=\"50\"" } else { "" };
            let (align, ptr) = if s.pointers.is_empty() {
                ("", String::new())
            } else {
                (" ALIGN=\"LEFT\"", format!("← {}", esc(&s.pointers.join(", "))))
            };
            let tail = if i + 1 == f.slots.len() { "</TABLE></TD></TR>" } else { "" };
            let _ = writeln!(
                out,
                "      <TR><TD{width}>{}</TD><TD BORDER=\"0\"{cell_width}{align}>{ptr}</TD></TR>{tail}",
                esc(&s.label)
            );
        }
    }
    out.push_str("  </TABLE>>];\n}\n");
    out
}

pub fn dot_stack(trace: &Path, loc: &str, frames: Option<usize>) -> Result<u8> {
    let lt = load(trace)?;
    let s = lt.locate(loc)?;
    let v = lt.stack_view(s, frames, RootFrame::WhenInnermost);
    with_stdout(|out| {
        write!(out, "{}", render(&v))?;
        Ok(0)
    })
}
