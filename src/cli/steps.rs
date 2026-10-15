use std::path::PathBuf;

use anyhow::Result;

use super::{load, with_stdout};
use crate::exit;
use crate::model::{EventView, StepView};

pub struct StepsOptions {
    pub trace: PathBuf,
    pub from: Option<String>,
    pub to: Option<String>,
    pub func: Option<String>,
    pub limit: usize,
    pub json: bool,
}

fn event_text(e: &EventView) -> String {
    match e {
        EventView::Syscall { name, args, ret, .. } => {
            let shown = args.iter().rposition(|a| a != "0").map_or(1, |p| p + 1);
            format!("syscall {name}({}) = {}", args[..shown].join(", "), ret.as_deref().unwrap_or("?"))
        }
        EventView::Signal { signal, code, addr } => format!("signal {signal} code {code} at {addr}"),
    }
}

fn text_line(v: &StepView) -> String {
    let mut line = format!("#{} {}  {}", v.step, v.loc, v.asm);
    for (reg, val) in &v.regs.0 {
        line.push_str(&format!(" {reg}={val}"));
    }
    if let Some(flags) = &v.rflags {
        line.push_str(&format!(" rflags=[{}]", flags.join(" ")));
    }
    for w in &v.writes {
        line.push_str(&format!(" {}←{}/{}", w.addr, w.value, w.size));
    }
    if let Some(e) = &v.event {
        line.push(' ');
        line.push_str(&event_text(e));
    }
    line
}

pub fn steps(o: &StepsOptions) -> Result<u8> {
    let lt = load(&o.trace)?;
    let n = lt.steps();
    let from = o.from.as_deref().map_or(Ok(0), |l| lt.locate(l))?;
    let to = o.to.as_deref().map_or(Ok(n), |l| lt.locate(l))?.min(n);
    let func = match &o.func {
        Some(name) => Some(lt.symbol_range(name).ok_or_else(|| exit::usage(format!("unknown symbol '{name}'")))?),
        None => None,
    };
    with_stdout(|out| {
        let (mut shown, mut more) = (0usize, 0u64);
        for i in from..to {
            if let Some((lo, hi)) = func {
                let rip = lt.rip_at(i);
                if rip < lo || rip >= hi {
                    continue;
                }
            }
            if shown >= o.limit {
                more += 1;
                continue;
            }
            shown += 1;
            let v = lt.step_view(i);
            if o.json {
                writeln!(out, "{}", serde_json::to_string(&v)?)?;
            } else {
                writeln!(out, "{}", text_line(&v))?;
            }
        }
        if more > 0 {
            if o.json {
                writeln!(out, "{}", serde_json::json!({ "truncated": more }))?;
            } else {
                writeln!(out, "… {more} more steps")?;
            }
        }
        Ok(0)
    })
}
