//! The public model API the HTTP server routes to.

mod common;

use common::*;
use systemslab::model::{LoadedTrace, NavOp};

fn load() -> (Built, LoadedTrace) {
    let b = build("fact.c", FACT_FLAGS);
    let t = record(&b, &[]);
    let lt = LoadedTrace::load(&t).unwrap();
    (b, lt)
}

#[test]
fn loaded_trace_is_send_and_sync() {
    fn check<T: Send + Sync>() {}
    check::<LoadedTrace>();
}

#[test]
fn state_view_has_regs_flags_source_and_stack() {
    let (_b, lt) = load();
    let s = lt.locate("fact+0xc@3").unwrap();
    let v = lt.state_view(s).unwrap();
    assert_eq!((v.kind, v.loc.as_str(), v.bytes.as_str()), ("instr", "fact+0xc", "48837df801"));
    let src = v.source.unwrap();
    assert_eq!((src.file.as_str(), src.line), ("fact.c", 4));
    let names: Vec<&str> = v.regs.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(&names[..8], ["rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rbp", "rsp"]);
    assert_eq!(names.last(), Some(&"gs_base"));
    assert!(v.regs.iter().find(|r| r.name == "rip").unwrap().changed, "previous insn moved rip");
    assert_eq!(v.depth, 3);
    assert_eq!(v.stack.frames.len(), 4);
    assert_eq!(v.xmm.len(), 16);
    let first = lt.state_view(0).unwrap();
    assert!(first.regs.iter().all(|r| !r.changed));
    let end = lt.state_view(lt.steps()).unwrap();
    assert_eq!(end.kind, "end");
    assert!(lt.state_view(lt.steps() + 1).is_err());
}

#[test]
fn nav_ops_follow_the_depth_and_line_rules() {
    let (_b, lt) = load();
    let s = lt.locate("fact+0xc@3").unwrap();
    let out = lt.nav(s, &NavOp::Out).unwrap().unwrap();
    assert_eq!(lt.stack_view(out, None).frames.last().unwrap().label, "fact(4)");
    let over = lt.nav(s, &NavOp::Over).unwrap().unwrap();
    assert!(over > s && lt.state_view(over).unwrap().depth <= 3);
    let back_out = lt.nav(s, &NavOp::BackOut).unwrap().unwrap();
    assert_eq!(lt.state_view(back_out).unwrap().depth, 2);
    let next_line = lt.nav(s, &NavOp::NextLine).unwrap().unwrap();
    assert_eq!(lt.state_view(next_line).unwrap().source.unwrap().line, 5);
    let prev_line = lt.nav(s, &NavOp::PrevLine).unwrap().unwrap();
    assert!(prev_line < s);
    let rax_prev = lt.nav(s, &NavOp::RegPrev(0)).unwrap().unwrap();
    assert!(lt.deltas_of_step(rax_prev).iter().any(|d| d.reg == 0));
    let rax_next = lt.nav(s, &NavOp::RegNext(0)).unwrap().unwrap();
    assert!(rax_next >= s);
    // The slot holding n was last written by the `mov %rdi,-0x8(%rbp)` of fact(3).
    let n_slot = u64::from_str_radix(
        lt.stack_view(s, None).frames[3].slots[2].raw_addr.trim_start_matches("0x"),
        16,
    )
    .unwrap();
    let w = lt.nav(s, &NavOp::WritePrev(n_slot)).unwrap().unwrap();
    assert_eq!(lt.last_writer(s, n_slot), Some(w));
    assert_eq!(lt.step_view(w).asm, "mov %rdi,-0x8(%rbp)");
    let rip = lt.regs_at(s).rip;
    let at_next = lt.nav(s, &NavOp::AtNext(rip)).unwrap().unwrap();
    assert!(at_next > s && lt.loc(lt.regs_at(at_next).rip) == "fact+0xc");
    assert!(NavOp::parse("bogus", None, None).is_err());
    assert!(matches!(NavOp::parse("reg-prev", Some("rax"), None), Ok(NavOp::RegPrev(0))));
    assert!(lt.nav(lt.steps() + 1, &NavOp::Over).is_err());
}

#[test]
fn timeline_mem_disasm_and_source() {
    let (_b, lt) = load();
    let tl = lt.timeline();
    assert_eq!(tl.steps, lt.steps());
    assert_eq!(tl.depth.len(), 1000);
    assert!(*tl.depth.iter().max().unwrap() >= 5, "fact recurses five deep");
    for kind in ["call", "ret", "syscall"] {
        assert!(tl.markers.iter().any(|m| m.kind == kind), "{kind}");
    }
    let s = lt.locate("fact+0xc@3").unwrap();
    let rsp = lt.regs_at(s).rsp();
    let m = lt.mem_view(rsp, 16, s).unwrap();
    assert_eq!(m.bytes.len(), 16);
    assert!(m.bytes.iter().all(Option::is_some));
    assert!(m.writers.iter().any(Option::is_some));
    assert!(lt.mem_view(rsp, 0, s).is_err() && lt.mem_view(rsp, 4097, s).is_err());
    assert!(lt.mem_view(0x10, 4, s).unwrap().bytes.iter().all(Option::is_none));
    let d = lt.disasm_view(lt.regs_at(s).rip, s).unwrap();
    assert_eq!(d.func, "fact");
    assert_eq!(d.lines[0].loc, "fact");
    assert_eq!(d.lines[0].asm, "push %rbp");
    assert!(d.lines.iter().any(|l| l.asm == "call fact" && l.source_line == Some(5)));
    let src = lt.source_view("fact.c").unwrap();
    assert!(src.lines[3].contains("n <= 1"));
    assert!(lt.source_view("nope.c").is_none());
    let info = lt.api_info();
    assert!(info.has_dwarf && info.functions.iter().any(|f| f.name == "fact"));
    let addrs: Vec<&str> = info.functions.iter().map(|f| f.name.as_str()).collect();
    assert!(addrs.iter().position(|n| *n == "fact") < addrs.iter().position(|n| *n == "main"));
}
