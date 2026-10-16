//! Guide directives: the `@go` and `@predict` lines of a Markdown guide.
//!
//! A guide is Markdown. Outside fenced code blocks (lines starting with three backticks toggle),
//! a line starting with `@go ` or `@predict ` at column 0 is a directive; trailing whitespace is
//! ignored.
//!
//! - `@go LOCATOR`: jump to the moment `LOCATOR` names.
//! - `@predict EXPR [as TYPE] at LOCATOR [after]`: ask the learner for `EXPR` (read as `TYPE`)
//!   at that moment, or one step later with `after`. The `> ` lines that follow, blank lines
//!   allowed, explain the answer; the first line that is neither blank nor `> ` ends them.

use std::str::FromStr;

use serde::Serialize;

use crate::model::WatchType;

/// A directive's kind, known even when the rest of the line is malformed. Serializes as `go` or `predict`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Go,
    Predict,
}

/// A well-formed directive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Directive {
    /// `@go LOCATOR`.
    Go { locator: String },
    /// `@predict EXPR [as TYPE] at LOCATOR [after]`.
    Predict {
        expr: String,
        ty: Option<WatchType>,
        locator: String,
        /// The value after the instruction at `locator` ran, i.e. at the next step.
        after: bool,
        /// The `> ` lines that follow, without the `> `, joined by newlines.
        explanation: String,
    },
}

/// One directive line of a guide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 1-based line number.
    pub line: usize,
    pub kind: Kind,
    /// The line, trimmed.
    pub text: String,
    /// The parsed directive, or what is wrong with the line.
    pub directive: Result<Directive, String>,
}

/// Every directive of `markdown` in order; a malformed one is an entry carrying its error message.
pub fn parse(markdown: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut in_fence = false;
    // The explanation lines of the last entry while it is a well-formed `@predict` that is still collecting them.
    let mut explanation: Option<Vec<&str>> = None;
    for (i, raw) in markdown.lines().enumerate() {
        let line = raw.trim_end();
        if let Some(lines) = &mut explanation {
            if line == ">" || line.starts_with("> ") {
                lines.push(line.strip_prefix("> ").unwrap_or(""));
                continue;
            }
            if line.is_empty() {
                continue;
            }
            if let (Some(lines), Some(Entry { directive: Ok(Directive::Predict { explanation: slot, .. }), .. })) = (explanation.take(), entries.last_mut()) {
                *slot = lines.join("\n");
            }
        }
        let fence = line.starts_with("```");
        if fence {
            in_fence = !in_fence;
        }
        if fence || in_fence {
            continue;
        }
        let (kind, rest) = if let Some(rest) = keyword(line, "@go") {
            (Kind::Go, rest)
        } else if let Some(rest) = keyword(line, "@predict") {
            (Kind::Predict, rest)
        } else {
            continue;
        };
        let directive = match kind {
            Kind::Go => parse_go(rest),
            Kind::Predict => parse_predict(rest),
        };
        if directive.is_ok() && kind == Kind::Predict {
            explanation = Some(Vec::new());
        }
        entries.push(Entry { line: i + 1, kind, text: line.to_string(), directive });
    }
    if let (Some(lines), Some(Entry { directive: Ok(Directive::Predict { explanation: slot, .. }), .. })) = (explanation, entries.last_mut()) {
        *slot = lines.join("\n");
    }
    entries
}

/// What follows `word` on `line` when `word` is the line's first word, with its leading space.
fn keyword<'a>(line: &'a str, word: &str) -> Option<&'a str> {
    line.strip_prefix(word).filter(|rest| rest.is_empty() || rest.starts_with(' '))
}

fn parse_go(rest: &str) -> Result<Directive, String> {
    match rest.split_whitespace().next() {
        Some(locator) => Ok(Directive::Go { locator: locator.to_string() }),
        None => Err("@go needs a locator".to_string()),
    }
}

/// `rest` is the line after `@predict`, starting with its space: ` EXPR [as TYPE] at LOCATOR [after]`.
fn parse_predict(rest: &str) -> Result<Directive, String> {
    let Some((left, right)) = rest.rsplit_once(" at ") else {
        return Err("@predict needs `at LOCATOR`".to_string());
    };
    let words: Vec<&str> = right.split_whitespace().collect();
    let (locator, after) = match words.as_slice() {
        [locator] => (*locator, false),
        [locator, "after"] => (*locator, true),
        [_, extra @ ..] => return Err(format!("only `after` may follow the locator, found '{}'", extra.join(" "))),
        [] => return Err("@predict needs a locator after `at`".to_string()),
    };
    let (expr, ty) = match left.rsplit_once(" as ").filter(|(_, ty)| !ty.contains(char::is_whitespace)) {
        Some((expr, ty)) => (expr.trim(), Some(WatchType::from_str(ty)?)),
        None => (left.trim(), None),
    };
    if expr.is_empty() {
        return Err("@predict needs an expression".to_string());
    }
    Ok(Directive::Predict { expr: expr.to_string(), ty, locator: locator.to_string(), after, explanation: String::new() })
}
