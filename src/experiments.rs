//! `systemslab try`: small bundled programs with guides, copied to a user-editable place and run.

use std::path::Path;

use anyhow::{Context, Result};
use rust_embed::RustEmbed;

use crate::run::{self, RunOptions};
use crate::{exit, paths};

#[derive(RustEmbed)]
#[folder = "experiments"]
struct Files;

/// Experiment names and what each teaches, in listing order. `experiments/<name>/` holds the files.
pub const EXPERIMENTS: [(&str, &str); 4] = [
    ("widths", "Register-width writes: what a 32-, 16- or 8-bit write does to the rest of %rax"),
    ("fact", "Recursion, stack frames and return values"),
    ("align", "Stack alignment at calls: why %rsp mod 16 matters"),
    ("saved", "Callee-saved registers: which registers a function must give back"),
];

/// Options of the `try` command.
pub struct TryOptions {
    pub name: Option<String>,
    pub port: u16,
    pub no_open: bool,
    pub no_view: bool,
}

/// `systemslab try [NAME]`: lists the experiments, or copies `NAME` to `experiments_dir()/NAME`
/// (keeping files the user already has) and runs it with its guide.
pub fn try_experiment(o: TryOptions) -> Result<u8> {
    let Some(name) = o.name else {
        println!("Experiments (systemslab try NAME):");
        for (name, description) in EXPERIMENTS {
            println!("  {name}  {description}");
        }
        return Ok(0);
    };
    if !EXPERIMENTS.iter().any(|(n, _)| *n == name) {
        return Err(exit::usage(format!("unknown experiment '{name}'; run systemslab try to list them")));
    }
    let dir = paths::experiments_dir()?.join(&name);
    if dir.exists() {
        eprintln!("using your copy in {} (delete it to restore the original)", dir.display());
    }
    let prefix = format!("{name}/");
    let mut embedded: Vec<String> = Files::iter().filter(|f| f.starts_with(&prefix)).map(|f| f.into_owned()).collect();
    embedded.sort();
    let mut source = None;
    for file in &embedded {
        let dest = dir.join(&file[prefix.len()..]);
        if !dest.exists() {
            let data = Files::get(file).with_context(|| format!("reading the bundled {file}"))?.data;
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
            }
            std::fs::write(&dest, &data).with_context(|| format!("writing {}", dest.display()))?;
        }
        if is_source(&dest) && source.is_none() {
            source = Some(dest);
        }
    }
    let source = source.with_context(|| format!("experiment '{name}' has no source file"))?;
    run::run(RunOptions { port: o.port, no_open: o.no_open, no_view: o.no_view, ..RunOptions::new(source) })
}

fn is_source(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "c" || e == "s" || e == "S")
}
