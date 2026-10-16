//! The public model API the HTTP server routes to.

mod common;

use common::*;
use systemslab::model::{ControlView, EventDetail, LoadedTrace, NavOp, NavParams, RootFrame, WatchType};

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
    assert_eq!(lt.stack_view(out, None, RootFrame::WhenInnermost).frames.last().unwrap().label, "fact(4)");
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
    assert!(rax_next > s);
    // The instruction at `rax_next` is itself the change: asking again must move on, not stay.
    assert!(lt.nav(rax_next, &NavOp::RegNext(0)).unwrap().unwrap() > rax_next);
    // The slot holding n was last written by the `mov %rdi,-0x8(%rbp)` of fact(3).
    let n_slot = u64::from_str_radix(
        lt.stack_view(s, None, RootFrame::WhenInnermost).frames[3].slots[2].raw_addr.trim_start_matches("0x"),
        16,
    )
    .unwrap();
    let w = lt.nav(s, &NavOp::WritePrev(n_slot)).unwrap().unwrap();
    assert_eq!(lt.last_writer(s, n_slot), Some(w));
    assert_eq!(lt.step_view(w).asm, "mov %rdi,-0x8(%rbp)");
    let rip = lt.regs_at(s).rip;
    let at_next = lt.nav(s, &NavOp::AtNext(rip)).unwrap().unwrap();
    assert!(at_next > s && lt.loc(lt.regs_at(at_next).rip) == "fact+0xc");
    assert!(NavOp::parse("bogus", &NavParams::default()).is_err());
    assert!(matches!(NavOp::parse("reg-prev", &NavParams { reg: Some("rax"), ..Default::default() }), Ok(NavOp::RegPrev(0))));
    assert!(lt.nav(lt.steps() + 1, &NavOp::Over).is_err());
}

#[test]
fn timeline_mem_disasm_and_source() {
    let (_b, lt) = load();
    let tl = lt.timeline(0, lt.steps()).unwrap();
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

#[test]
fn state_view_expands_the_root_frame() {
    let (_b, lt) = load();
    let s = lt.locate("fact+0xc@3").unwrap();
    let main = &lt.state_view(s).unwrap().stack.frames[0];
    assert!(!main.collapsed, "{main:?}");
    assert_eq!(main.slots.len(), 2);
    let at_main = lt.state_view(lt.locate("main+0x4").unwrap()).unwrap();
    assert_eq!(at_main.stack.frames.len(), 1);
    assert!(at_main.stack.frames[0].slots[0].label.starts_with("return address: "));
}

#[test]
fn prev_line_walks_back_over_nested_calls_to_the_start_of_the_line() {
    let (_b, lt) = load();
    let from = lt.locate("fact+0x2f@2").unwrap();
    assert_eq!(lt.nav(from, &NavOp::PrevLine).unwrap(), Some(lt.locate("fact+0x1a@4").unwrap()));
}

#[test]
fn state_view_prev_writes_are_the_previous_steps_writes() {
    let (_b, lt) = load();
    let s = lt.locate("fact+0x1@1").unwrap();
    let v = lt.state_view(s).unwrap();
    // fact+0x0 is `push %rbp`; at fact+0x1 its 8-byte write at the new %rsp is the previous step's.
    assert_eq!(v.prev_writes.len(), 1, "{:?}", v.prev_writes);
    let w = &v.prev_writes[0];
    assert_eq!((w.size, w.kind), (8, "instr"));
    assert_eq!(w.raw_addr, format!("0x{:x}", lt.regs_at(s).rsp()));
    // The instruction about to run (`mov %rsp,%rbp`) writes no memory, which its effects say as well.
    assert!(lt.effects(s).unwrap().mem_writes.is_empty());
    assert!(lt.state_view(0).unwrap().prev_writes.is_empty());
}

/// Compiles C `source` as `name` with the fact flags, records it and loads the trace.
fn load_source(name: &str, source: &str) -> (Built, LoadedTrace) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(name), source).unwrap();
    let out = std::process::Command::new("gcc").current_dir(dir.path()).args(FACT_FLAGS).args(["-o", "prog", name]).output().expect("gcc on PATH");
    assert!(out.status.success(), "gcc failed: {}", String::from_utf8_lossy(&out.stderr));
    let b = Built { exe: dir.path().join("prog"), dir };
    let t = record(&b, &[]);
    let lt = LoadedTrace::load(&t).unwrap();
    (b, lt)
}

const LONGJMP_C: &str = r#"#include <setjmp.h>
static jmp_buf jb;
static void deep(int n) { if (n == 0) longjmp(jb, 1); deep(n - 1); }
int main(void) { if (setjmp(jb) == 0) deep(3); return 0; }
"#;

#[test]
fn longjmp_discards_the_unwound_activations() {
    let (_b, lt) = load_source("lj.c", LONGJMP_C);
    let in_deep = lt.locate("deep@1").unwrap();
    assert_eq!(lt.state_view(in_deep).unwrap().depth, 1);
    // The first step back in main is setjmp returning a second time; `deep` never returned.
    let back = (in_deep..lt.steps()).find(|&s| lt.loc(lt.rip_at(s)).starts_with("main")).unwrap();
    assert_eq!(lt.state_view(back).unwrap().depth, 0);
    let frames = lt.stack_view(back, None, RootFrame::WhenInnermost).frames;
    let labels: Vec<&str> = frames.iter().map(|f| f.label.as_str()).collect();
    assert_eq!(labels, ["main"], "{frames:?}");
    assert!(!frames[0].collapsed);
}

const SIGNAL_C: &str = r#"#include <signal.h>
#include <string.h>
static volatile int got;
static int sq(int x) { return x * x; }
static void h(int s) { got = sq(s); }
int main(void) {
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = h;
#ifdef ALT
    static char alt[64 * 1024];
    stack_t ss = { .ss_sp = alt, .ss_size = sizeof alt };
    sigaltstack(&ss, 0);
    sa.sa_flags = SA_ONSTACK;
#endif
    sigaction(SIGUSR1, &sa, 0);
    raise(SIGUSR1);
    return got == 100 ? 0 : 1;
}
"#;

#[test]
fn a_signal_handler_is_an_activation_with_its_own_frame() {
    let (_b, lt) = load_source("sig.c", SIGNAL_C);
    let in_sq = lt.locate("sq@1").unwrap() + 3; // after `push %rbp; mov %rsp,%rbp; mov %edi,-4(%rbp)`
    let frames = lt.stack_view(in_sq, None, RootFrame::WhenInnermost).frames;
    let labels: Vec<&str> = frames.iter().map(|f| f.label.as_str()).collect();
    assert_eq!(&labels[labels.len() - 2..], ["h(10)", "sq(10)"], "{labels:?}");
    assert_eq!(frames.iter().filter(|f| f.func == "h").count(), 1, "{labels:?}");
    let h = &frames[frames.len() - 2];
    assert!(h.slots[0].label.starts_with("return address: "), "{:?}", h.slots[0]);
    assert!(h.slots[1].label.starts_with("saved %rbp: "), "{:?}", h.slots[1]);
    // The handler's `ret` closes its activation, and stepping out lands back in the interrupted code.
    let in_h = lt.locate("h@1").unwrap();
    let depth = lt.state_view(in_h).unwrap().depth;
    let out = lt.nav(in_h, &NavOp::Out).unwrap().unwrap();
    assert_eq!(lt.state_view(out).unwrap().depth, depth - 1);
    assert!(lt.abi_findings(true).iter().all(|f| !f.message.contains(" h ")), "{:?}", lt.abi_findings(true));
}

#[test]
fn a_handler_on_an_alternate_stack_has_a_small_frame_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("alt.c"), format!("#define ALT 1\n{SIGNAL_C}")).unwrap();
    let out = std::process::Command::new("gcc").current_dir(dir.path()).args(FACT_FLAGS).args(["-o", "prog", "alt.c"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let b = Built { exe: dir.path().join("prog"), dir };
    let lt = LoadedTrace::load(&record(&b, &[])).unwrap();
    let in_sq = lt.locate("sq@1").unwrap() + 3;
    let frames = lt.stack_view(in_sq, None, RootFrame::Always).frames;
    let slots: usize = frames.iter().map(|f| f.slots.len()).sum();
    assert!(slots < 1000, "{slots} slots");
    assert_eq!(frames.iter().rev().take(2).map(|f| f.label.as_str()).collect::<Vec<_>>(), ["sq(10)", "h(10)"]);
}

#[test]
fn a_huge_frame_shows_its_ends_and_says_what_it_skips() {
    let src = "static int deep(void) { volatile char big[64 * 1024]; big[0] = 1; big[sizeof big - 1] = 2; return big[0] + big[sizeof big - 1]; }\nint main(void) { return deep() == 3 ? 0 : 1; }\n";
    let (_b, lt) = load_source("big.c", src);
    let s = (0..lt.steps()).find(|&s| lt.regs_at(s).rsp() < lt.stack0() - 60_000).unwrap();
    let v = lt.state_view(s).unwrap();
    let deep = v.stack.frames.last().unwrap();
    assert_eq!(deep.func, "deep");
    assert!(deep.size.unwrap() >= 64 * 1024, "{:?}", deep.size);
    assert!(deep.slots.len() <= 513, "{} slots", deep.slots.len());
    assert!(deep.slots.iter().any(|sl| sl.label.starts_with("… ") && sl.label.ends_with("not shown")), "{:?}", deep.slots.iter().map(|s| &s.label).collect::<Vec<_>>());
    assert!(deep.slots[0].label.starts_with("return address: "));
    assert_eq!(deep.slots.last().unwrap().pointers, ["%rsp"]);
}

#[test]
fn locals_smaller_than_a_slot_are_judged_by_their_own_bytes() {
    // a, b, c and ch share two 8-byte slots; neither slot's first byte need be the one a store hit.
    let src = "static int add(int x, int y) { int s = x + y; return s; }\nint main(void) { int a = 5; int b = 7; int c = add(a, b); char ch = 'x'; return c == 12 && ch ? 0 : 1; }\n";
    let (_b, lt) = load_source("ints.c", src);
    let s = (0..lt.steps()).rev().find(|&s| lt.loc(lt.rip_at(s)).starts_with("main") && lt.asm_at(s).starts_with("cmpb")).unwrap();
    let main = &lt.state_view(s).unwrap().stack.frames[0];
    let text = main.slots.iter().map(|sl| sl.label.as_str()).collect::<Vec<_>>().join(" | ");
    for want in ["a = 5", "b = 7", "c = 12", "ch = 120"] {
        assert!(text.contains(want), "{want} in {text}");
    }
    assert!(!text.contains("not written yet"), "{text}");
}

#[test]
fn memory_reads_at_the_top_of_the_address_space_end_instead_of_wrapping() {
    let (_b, lt) = load();
    for addr in [0xffff_ffff_ffff_f000u64, 0xffff_ffff_ffff_fff0, u64::MAX] {
        let m = lt.mem_view(addr, 4096, 0).unwrap();
        assert!(m.bytes.iter().all(Option::is_none), "{addr:#x}");
    }
    let d = lt.disasm_view(u64::MAX - 8, 0).unwrap();
    assert!(d.lines.is_empty());
}

#[test]
fn disasm_of_a_symbol_with_a_huge_size_lists_only_what_ran() {
    // `.size` claims 4 MiB for a 3-instruction function; decoding that much is neither useful nor cheap.
    let dir = tempfile::tempdir().unwrap();
    let src = "\t.globl _start\n\t.text\n\t.type _start, @function\n_start:\n\tmov $60, %eax\n\txor %edi, %edi\n\tsyscall\n\t.size _start, 0x400000\n";
    std::fs::write(dir.path().join("huge.s"), src).unwrap();
    let out = std::process::Command::new("gcc").current_dir(dir.path()).args(ASM_FLAGS).args(["-o", "prog", "huge.s"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let b = Built { exe: dir.path().join("prog"), dir };
    let lt = LoadedTrace::load(&record(&b, &[])).unwrap();
    let d = lt.disasm_view(lt.regs_at(0).rip, 0).unwrap();
    assert_eq!(d.func, "_start");
    assert_eq!(d.lines.iter().map(|l| l.asm.as_str()).collect::<Vec<_>>(), ["mov $0x3C,%eax", "xor %edi,%edi", "syscall"]);
}

fn raw(addr: &str) -> u64 {
    u64::from_str_radix(addr.trim_start_matches("0x"), 16).unwrap()
}

#[test]
fn plt_stubs_are_named() {
    let (_b, lt) = load();
    let main = lt.symbol_range("main").unwrap().0;
    let d = lt.disasm_view(main, 0).unwrap();
    assert!(d.lines.iter().any(|l| l.asm == "call printf@plt"), "{:?}", d.lines.iter().map(|l| &l.asm).collect::<Vec<_>>());
    let stub = lt.locate("printf@plt").unwrap();
    assert_eq!(lt.loc(lt.rip_at(stub)), "printf@plt");
    assert_eq!(lt.locate("printf@plt@1").unwrap(), stub);
    assert!(lt.api_info().functions.iter().any(|f| f.name == "printf@plt" && f.loc == "printf@plt"));
}

#[test]
fn line_and_end_locators() {
    let (_b, lt) = load();
    let fact3 = lt.locate("fact+0xc@3").unwrap();
    assert_eq!(lt.locate("fact.c:4@3").unwrap(), fact3);
    assert_eq!(lt.locate("fact.c:4").unwrap(), lt.locate("fact+0xc@1").unwrap());
    assert_eq!(lt.locate("step:end").unwrap(), lt.steps());
    assert_eq!(lt.locate("fact.c:4@9").unwrap_err().to_string(), "fact.c:4 occurs 5 times; @9 asked");
    assert_eq!(lt.locate("nope.c:4").unwrap_err().to_string(), "unknown source file 'nope.c'");
    assert_eq!(lt.locate("fact+0xc@9").unwrap_err().to_string(), "fact+0xc occurs 5 times; @9 asked");
    // A library symbol with a colon stays a symbol.
    assert!(lt.locate("libc.so.6:printf").is_ok());
}

#[test]
fn locator_of_round_trips_through_locate() {
    let (_b, lt) = load();
    let n = lt.steps();
    assert_eq!(lt.locator_of(n), "step:end");
    assert_eq!(lt.locator_of(lt.locate("fact+0xc@3").unwrap()), "fact+0xc@3");
    assert_eq!(lt.locator_of(0), "main@1");
    let in_libc = (0..n).find(|&s| lt.loc(lt.rip_at(s)).contains(':')).expect("a step inside libc");
    assert!(lt.locator_of(in_libc).contains(':'), "{}", lt.locator_of(in_libc));
    for s in (0..=n).step_by(7).chain([0, in_libc, n]) {
        let loc = lt.locator_of(s);
        assert_eq!(lt.locate(&loc).unwrap(), s, "{loc}");
    }
}

#[test]
fn write_next_and_line_next_navigate_to_the_next_event() {
    let (_b, lt) = load();
    let s = lt.locate("fact+0xc@3").unwrap();
    let n_slot = raw(&lt.stack_view(s, None, RootFrame::WhenInnermost).frames[3].slots[2].raw_addr);
    let w = lt.nav(s, &NavOp::WritePrev(n_slot)).unwrap().unwrap();
    let n = lt.steps();
    let covers = |i: u32, addr: u64| {
        lt.writes_of_step(i).iter().any(|x| x.kind != systemslab::trace::WriteKind::Snapshot && x.addr <= addr && addr < x.addr + x.data.len() as u64)
    };
    // Strictly after `from`, and the first such step: compare with a scan, from the write itself and elsewhere.
    let mut found = 0;
    for addr in [n_slot, lt.stack0() - 8, lt.stack0() - 0x10, lt.stack0() - 0x58] {
        for from in [0, w, n / 4, n / 2, n - 1, n] {
            let expect = (from + 1..n).find(|&i| covers(i, addr));
            assert_eq!(lt.nav(from, &NavOp::WriteNext(addr)).unwrap(), expect, "from {from} at {addr:#x}");
            found += usize::from(expect.is_some());
        }
    }
    assert!(found > 4, "{found} hits");
    assert_eq!(lt.nav(0, &NavOp::WriteNext(n_slot)).unwrap(), Some(w));
    assert!(matches!(NavOp::parse("write-next", &NavParams { addr: Some("0x10"), ..Default::default() }), Ok(NavOp::WriteNext(0x10))));
    assert!(NavOp::parse("write-next", &NavParams::default()).is_err());
    let params = NavParams { file: Some("fact.c"), line: Some("4"), ..Default::default() };
    assert_eq!(NavOp::parse("line-next", &params).unwrap(), NavOp::LineNext { file: "fact.c".into(), line: 4 });
    assert!(NavOp::parse("line-next", &NavParams { file: Some("fact.c"), ..Default::default() }).is_err());
    assert!(NavOp::parse("line-next", &NavParams { file: Some("fact.c"), line: Some("x"), ..Default::default() }).is_err());

    let line = |file: &str, line: u32| NavOp::LineNext { file: file.to_string(), line };
    // From line 4 of fact(3), line 5 is entered next at fact+0x1a, and line 4 again in fact(2).
    assert_eq!(lt.nav(s, &line("fact.c", 5)).unwrap(), Some(lt.locate("fact.c:5@3").unwrap()));
    assert_eq!(lt.loc(lt.rip_at(lt.nav(s, &line("fact.c", 5)).unwrap().unwrap())), "fact+0x1a");
    assert_eq!(lt.nav(s, &line("fact.c", 4)).unwrap(), Some(lt.locate("fact.c:4@4").unwrap()));
    assert_eq!(lt.nav(lt.locate("fact.c:4@5").unwrap(), &line("fact.c", 4)).unwrap(), None);
    assert!(lt.nav(s, &line("nope.c", 4)).is_err());
}

#[test]
fn var_at_finds_a_variable_in_scope_of_the_innermost_frame() {
    let (_b, lt) = load();
    let s = lt.locate("fact+0xc@3").unwrap();
    let n_slot = &lt.stack_view(s, None, RootFrame::WhenInnermost).frames[3].slots[2];
    let (addr, v) = lt.var_at(s, "n").unwrap();
    assert_eq!(format!("0x{addr:x}"), n_slot.raw_addr);
    assert_eq!(v.name, "n");
    assert!(lt.var_at(s, "nope").is_none());
    // No DWARF scope at the start of main knows `n`.
    assert!(lt.var_at(0, "n").is_none());
}

#[test]
fn frames_know_the_steps_of_their_call_and_ret() {
    let (_b, lt) = load();
    let s = lt.locate("fact+0xc@3").unwrap();
    let frames = lt.stack_view(s, None, RootFrame::WhenInnermost).frames;
    assert_eq!((frames[0].call_step, frames[0].ret_step), (None, None), "{:?}", frames[0]);
    let fact3 = &frames[3];
    assert_eq!(fact3.call_step, Some(lt.locate("fact+0x25@2").unwrap()));
    assert_eq!(fact3.ret_step, Some(lt.locate("fact+0x30@3").unwrap()));
    // The collapsed row stands for the innermost hidden activation.
    let collapsed = &lt.stack_view(s, Some(1), RootFrame::WhenInnermost).frames[0];
    assert!(collapsed.collapsed);
    assert_eq!(collapsed.call_step, Some(lt.locate("fact+0x25@1").unwrap()));
    // A frame still open at the end of the recording does not return.
    let main_ret = lt.stack_view(0, None, RootFrame::Always).frames;
    assert_eq!(main_ret.len(), 1);
}

#[test]
fn disasm_lines_carry_hits_and_source_files() {
    let (_b, lt) = load();
    let d = lt.disasm_view(lt.symbol_range("fact").unwrap().0, 0).unwrap();
    let push = d.lines.iter().find(|l| l.asm == "push %rbp").unwrap();
    assert_eq!((push.hits, push.source_file.as_deref(), push.source_line), (5, Some("fact.c"), Some(3)));
    let cmp = d.lines.iter().find(|l| l.loc == "fact+0xc").unwrap();
    assert_eq!(cmp.hits, 5);
    // libc code is not covered by DWARF, and most of what it contains never ran.
    let in_libc: Vec<u32> = (0..lt.steps()).step_by(25).filter(|&s| lt.loc(lt.rip_at(s)).contains(':')).collect();
    let mut partly_run = false;
    for s in in_libc {
        let lib = lt.disasm_view(lt.rip_at(s), s).unwrap();
        assert!(lib.lines.iter().all(|l| l.source_file.is_none()));
        assert!(lib.lines.iter().any(|l| l.hits > 0), "the executing instruction is listed");
        partly_run |= lib.lines.iter().any(|l| l.hits == 0);
    }
    assert!(partly_run, "some libc function has instructions that never ran");
}

#[test]
fn assembly_labels_name_addresses_without_splitting_the_function() {
    let b = build("labels.s", ASM_FLAGS);
    let lt = LoadedTrace::load(&record(&b, &[])).unwrap();
    let again = lt.symbol_range("again").unwrap().0;
    assert_eq!(lt.loc(again), "again");
    assert_eq!(lt.loc(again + 5), "again+0x5");
    let d = lt.disasm_view(again, 0).unwrap();
    assert_eq!(d.func, "_start");
    assert!(d.lines.iter().any(|l| l.asm == "jne again"), "{:?}", d.lines.iter().map(|l| &l.asm).collect::<Vec<_>>());
    assert!(d.lines.len() >= 7, "the label did not end _start: {} lines", d.lines.len());
    // `helper` is the target of a call, so it is a function, not a label.
    let helper = lt.locate("helper@2").unwrap();
    let frames = lt.stack_view(helper, None, RootFrame::WhenInnermost).frames;
    assert_eq!(frames.iter().map(|f| f.label.as_str()).collect::<Vec<_>>(), ["_start", "helper"]);
    // The caller is `_start`, not the loop label the return address falls under.
    assert!(frames[1].slots[0].label.ends_with(" in _start"), "{:?}", frames[1].slots[0]);
    assert_eq!(lt.api_info().functions.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["_start", "helper"]);
    assert!(lt.locate("again@3").is_ok());
    assert_eq!(lt.locate("again@4").unwrap_err().to_string(), "again occurs 3 times; @4 asked");
    // Recorded from the entry point, the kernel's argc sits at the initial %rsp.
    let root = &lt.state_view(0).unwrap().stack.frames[0];
    assert_eq!(root.slots[0].label, "argc = 1", "{root:?}");
    assert_eq!(root.slots[0].writer, None);
}

// ---- effects ----

#[test]
fn effects_of_push_rbp_show_the_register_and_memory_changes() {
    let (_b, lt) = load();
    let s = lt.locate("fact@1").unwrap();
    assert_eq!(lt.asm_at(s), "push %rbp");
    let e = lt.effects(s).unwrap();
    assert_eq!((e.mnemonic.as_str(), e.kind, e.next_loc.as_str()), ("push", "instr", "fact+0x1"));
    let rsp = e.reg_writes.iter().find(|w| w.name == "rsp").unwrap();
    assert_eq!(raw(&rsp.after), raw(&rsp.before) - 8);
    assert!(rsp.changed && rsp.via.is_none(), "{rsp:?}");
    assert_eq!(rsp.before_rendered, lt.render_value(lt.regs_at(s).rsp(), s));
    assert_eq!(rsp.after_rendered, lt.render_value(lt.regs_at(s + 1).rsp(), s + 1));
    assert_eq!((e.mem_writes.len(), e.mem_writes[0].size, e.mem_writes[0].kind), (1, 8, "instr"), "{:?}", e.mem_writes);
    assert_eq!(e.mem_writes[0].raw_addr, rsp.after, "push writes at the new %rsp");
    let reads: Vec<&str> = e.reg_reads.iter().map(|r| r.name.as_str()).collect();
    assert!(reads.contains(&"rbp") && reads.contains(&"rsp"), "{reads:?}");
    let rbp = e.reg_reads.iter().find(|r| r.name == "rbp").unwrap();
    assert_eq!(e.mem_writes[0].after, rbp.rendered, "the pushed bytes are the old %rbp");
    assert!(e.mem_reads.is_empty() && e.flags_read.is_empty() && e.flags_written.is_empty());
    assert!(e.condition.is_none() && e.control.is_none() && e.event.is_none() && e.delivered.is_none());
    assert!(lt.effects(lt.steps()).is_none());
}

#[test]
fn a_32_bit_write_names_the_register_it_goes_through() {
    let (_b, lt) = load();
    let s = lt.locate("main+0x4").unwrap();
    assert_eq!(lt.asm_at(s), "mov $0x5,%edi");
    let e = lt.effects(s).unwrap();
    let w = e.reg_writes.iter().find(|w| w.name == "rdi").unwrap();
    assert_eq!((w.via.as_deref(), w.after.as_str(), w.after_rendered.as_str(), w.changed), (Some("edi"), "0x5", "5", true), "{w:?}");
    assert!(e.reg_reads.is_empty(), "{:?}", e.reg_reads);
    assert!(e.reg_writes.iter().all(|w| w.name != "rip" && w.name != "rflags"));
}

#[test]
fn a_conditional_jump_reports_its_condition_and_whether_it_was_taken() {
    let (_b, lt) = load();
    let taken = lt.effects(lt.locate("fact+0x11@3").unwrap()).unwrap();
    assert_eq!(taken.mnemonic, "jg");
    let c = taken.condition.unwrap();
    assert_eq!((c.code, c.text, c.meaning, c.holds), ("g", "ZF = 0 and SF = OF", "greater (signed)", true));
    let names: Vec<&str> = taken.flags_read.iter().map(|f| f.name).collect();
    assert!(["ZF", "SF", "OF"].iter().all(|n| names.contains(n)) && names.len() == 3, "{names:?}");
    match taken.control.unwrap() {
        ControlView::Branch { conditional, taken, target, fallthrough } => {
            assert_eq!((conditional, taken, target.as_str(), fallthrough.as_str()), (true, true, "fact+0x1a", "fact+0x13"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(taken.next_loc, "fact+0x1a");
    // fact(1) returns 1 instead of recursing: the jump over the base case is not taken.
    let not_taken = lt.effects(lt.locate("fact+0x11@5").unwrap()).unwrap();
    assert!(!not_taken.condition.unwrap().holds);
    match not_taken.control.unwrap() {
        ControlView::Branch { conditional: true, taken: false, target, fallthrough } => {
            // A branch not taken still names where it would have gone; execution continues at `fallthrough`.
            assert_eq!((target.as_str(), fallthrough.as_str()), ("fact+0x1a", "fact+0x13"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(not_taken.next_loc, "fact+0x13");
}

#[test]
fn call_and_ret_effects_name_the_slot_and_the_matching_call() {
    let (_b, lt) = load();
    let s = lt.locate("fact+0x25@1").unwrap();
    let e = lt.effects(s).unwrap();
    assert_eq!(e.mnemonic, "call");
    match e.control.as_ref().unwrap() {
        ControlView::Call { target, return_address, slot, rsp_before, rsp_after, rsp_mod16 } => {
            assert_eq!((target.as_str(), return_address.as_str(), *rsp_mod16), ("fact", "fact+0x2a", 0));
            assert_eq!(slot, rsp_after);
            assert_eq!(rsp_before, &lt.render_value(lt.regs_at(s).rsp(), s));
            assert_eq!(rsp_after, &lt.render_value(lt.regs_at(s + 1).rsp(), s + 1));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!((e.mem_writes.len(), e.mem_writes[0].size, e.mem_writes[0].after.as_str()), (1, 8, "fact+0x2a"), "{:?}", e.mem_writes);

    let s = lt.locate("fact+0x30@1").unwrap();
    let e = lt.effects(s).unwrap();
    assert_eq!(e.mnemonic, "ret");
    match e.control.as_ref().unwrap() {
        ControlView::Ret { to, slot, rsp_before, rsp_after, call_step } => {
            assert_eq!(to, "fact+0x2a");
            assert_eq!(slot, rsp_before);
            assert_ne!(rsp_before, rsp_after);
            assert_eq!(*call_step, Some(lt.locate("fact+0x25@4").unwrap()));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!((e.mem_reads.len(), e.mem_reads[0].size, e.mem_reads[0].value.as_str()), (1, 8, "fact+0x2a"), "{:?}", e.mem_reads);
}

#[test]
fn a_compare_reads_memory_and_writes_the_arithmetic_flags() {
    let (_b, lt) = load();
    let s = lt.locate("fact+0xc@3").unwrap();
    let e = lt.effects(s).unwrap();
    assert_eq!(e.mnemonic, "cmp");
    assert_eq!((e.mem_reads.len(), e.mem_reads[0].size, e.mem_reads[0].value.as_str()), (1, 8, "3"), "{:?}", e.mem_reads);
    assert_eq!(e.mem_reads[0].raw_addr, format!("0x{:x}", lt.regs_at(s).rbp() - 8));
    let written: Vec<&str> = e.flags_written.iter().map(|f| f.name).collect();
    for flag in ["ZF", "SF", "CF", "OF", "AF", "PF"] {
        assert!(written.contains(&flag), "{flag} in {written:?}");
    }
    assert!(e.flags_written.iter().all(|f| !f.undefined));
    let (before, after) = (lt.regs_at(s), lt.regs_at(s + 1));
    for f in &e.flags_written {
        let bit = systemslab::model::FLAGS.iter().find(|(n, _)| *n == f.name).unwrap().1;
        assert_eq!((f.before as u64, f.after as u64), (before.rflags >> bit & 1, after.rflags >> bit & 1), "{}", f.name);
    }
    assert!(e.flags_read.is_empty());
    assert!(e.reg_writes.iter().all(|w| w.name != "rflags"));
}

#[test]
fn a_syscall_step_carries_its_event_and_the_registers_the_kernel_overwrote() {
    let (_b, lt) = load();
    let step = lt
        .events()
        .into_iter()
        .find_map(|e| match e {
            EventDetail::Syscall { step, name, .. } if name == "write" => Some(step),
            _ => None,
        })
        .unwrap();
    let e = lt.effects(step).unwrap();
    assert_eq!(e.mnemonic, "syscall");
    assert!(matches!(&e.event, Some(EventDetail::Syscall { name, .. }) if name == "write"), "{:?}", e.event);
    for r in ["rax", "rcx", "r11"] {
        assert!(e.reg_writes.iter().any(|w| w.name == r), "{r} in {:?}", e.reg_writes);
    }
    let rax = e.reg_writes.iter().find(|w| w.name == "rax").unwrap();
    assert_eq!((rax.after.as_str(), rax.after_rendered.as_str()), ("0xe", "14"));
    assert!(e.control.is_none());
}

#[test]
fn a_signal_delivery_step_names_the_handler() {
    let (_b, lt) = load_source("sig.c", SIGNAL_C);
    let s = (0..lt.steps()).find(|&s| lt.step_kind(s) == "deliver").expect("a delivery step");
    let e = lt.effects(s).unwrap();
    assert_eq!(e.kind, "deliver");
    let d = e.delivered.as_ref().unwrap();
    assert_eq!((d.signal.as_str(), d.handler.as_deref()), ("SIGUSR1", Some("h")));
    assert_eq!(e.next_loc, "h");
    assert!(e.control.is_none());
    // The kernel built the signal frame: those writes are in the step.
    assert!(e.mem_writes.iter().any(|w| w.kind == "kernel"), "{:?}", e.mem_writes);
    // The interrupted instruction did not run: only what the kernel changed is listed.
    assert!(e.reg_reads.is_empty() && e.mem_reads.is_empty() && e.flags_read.is_empty() && e.condition.is_none(), "{e:?}");
    let written: Vec<&str> = e.reg_writes.iter().map(|w| w.name.as_str()).collect();
    assert!(written.contains(&"rsp") && written.contains(&"rdi"), "{written:?}");
    assert!(e.reg_writes.iter().all(|w| w.changed) && e.flags_written.iter().all(|f| f.before != f.after && !f.undefined), "{e:?}");
    // Ordinary steps deliver nothing.
    assert!(lt.effects(0).unwrap().delivered.is_none());
}

#[test]
fn a_faulting_division_is_a_fault_step_with_its_signal_event() {
    let b = build("sigfpe.c", FACT_FLAGS);
    let lt = LoadedTrace::load(&record(&b, &[])).unwrap();
    let s = (0..lt.steps()).find(|&s| lt.step_kind(s) == "fault").expect("a fault step");
    let e = lt.effects(s).unwrap();
    assert_eq!((e.mnemonic.as_str(), e.kind), ("idiv", "fault"));
    assert!(e.control.is_none());
    match e.event.unwrap() {
        EventDetail::Signal { signal, code_name, cpu_exception, .. } => {
            assert_eq!(signal, "SIGFPE");
            assert_eq!(code_name.as_deref(), Some("FPE_INTDIV"));
            assert_eq!(cpu_exception.as_deref(), Some("#DE (divide error)"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn state_view_carries_the_locator_effects_and_previous_register_values() {
    let (_b, lt) = load();
    let s = lt.locate("fact+0x1@1").unwrap();
    let v = lt.state_view(s).unwrap();
    assert_eq!(v.locator, "fact+0x1@1");
    assert_eq!(v.effects.as_ref().unwrap().mnemonic, "mov");
    let (now, before) = (lt.regs_at(s), lt.regs_at(s - 1));
    let rsp = v.regs.iter().find(|r| r.name == "rsp").unwrap();
    assert!(rsp.changed);
    assert_eq!(rsp.prev, Some(format!("0x{:x}", before.rsp())));
    assert_eq!(rsp.value, format!("0x{:x}", now.rsp()));
    for r in v.regs.iter().filter(|r| !r.changed) {
        assert_eq!(r.prev, None, "{}", r.name);
    }
    let end = lt.state_view(lt.steps()).unwrap();
    assert!(end.effects.is_none());
    assert_eq!(end.locator, "step:end");
    // The JSON drops `writes` and `event` and adds `locator`, `effects` and `prev`.
    let json = serde_json::to_value(&v).unwrap();
    assert!(json.get("writes").is_none() && json.get("event").is_none());
    assert_eq!(json["locator"], "fact+0x1@1");
    assert_eq!(json["effects"]["mnemonic"], "mov");
    assert!(json["regs"][7]["prev"].is_string() && json["regs"][0]["prev"].is_null());
    let jg = serde_json::to_value(lt.effects(lt.locate("fact+0x11@3").unwrap()).unwrap()).unwrap();
    assert_eq!(jg["control"]["kind"], "branch");
    assert_eq!(jg["condition"]["holds"], true);
    assert!(jg["event"].is_null() && jg["delivered"].is_null());
    let call = serde_json::to_value(lt.effects(lt.locate("fact+0x25@1").unwrap()).unwrap()).unwrap();
    assert_eq!(call["control"]["kind"], "call");
    assert_eq!(call["control"]["rsp_mod16"], 0);
}

// ---- expressions, watches, inspection ----

#[test]
fn eval_reads_registers_arithmetic_memory_operands_variables_and_symbols() {
    let (_b, lt) = load();
    let s = lt.locate("fact+0xc@3").unwrap();
    let regs = lt.regs_at(s);
    let hex = |v: u64| format!("0x{v:x}");
    let e = lt.eval(s, "rsp mod 16").unwrap();
    assert_eq!((e.value, e.default_type), (hex(regs.rsp() % 16), WatchType::Value));
    assert_eq!(lt.eval(s, "RSP MOD 16").unwrap().value, hex(regs.rsp() % 16));
    assert_eq!(lt.eval(s, "%rsp").unwrap().value, hex(regs.rsp()));
    assert_eq!(lt.eval(s, "rsp").unwrap().rendered, lt.render_value(regs.rsp(), s));
    // A memory operand denotes the address, not the memory there.
    let m = lt.eval(s, "-0x8(%rbp)").unwrap();
    assert_eq!((m.value.as_str(), m.default_type), (hex(regs.rbp() - 8).as_str(), WatchType::Ptr));
    let sum = lt.eval(s, "rbp-8").unwrap();
    assert_eq!((sum.value.as_str(), sum.default_type), (m.value.as_str(), WatchType::Value));
    assert_eq!(lt.eval(s, "rbp - 0x8").unwrap().value, m.value);
    assert_eq!(lt.eval(s, "(%rbp)").unwrap().value, hex(regs.rbp()));
    assert_eq!(lt.eval(s, "8(%rbp,%rax,4)").unwrap().value, hex(regs.rbp() + 8 + regs.gpr[0] * 4));
    assert_eq!(lt.eval(s, "-8").unwrap().value, hex(-8i64 as u64));
    assert_eq!(lt.eval(s, "0xffffffffffffffff + 2").unwrap().value, "0x1", "arithmetic wraps at 64 bits");
    assert_eq!(lt.eval(s, "edi").unwrap().value, hex(regs.gpr[5] as u32 as u64));
    assert_eq!(lt.eval(s, "dil").unwrap().value, hex(regs.gpr[5] & 0xff));
    assert_eq!(lt.eval(s, "rip").unwrap().value, hex(regs.rip));
    assert_eq!(lt.eval(s, "rflags").unwrap().value, hex(regs.rflags));
    // A C variable evaluates to its address; its type is the declared one.
    let n = lt.eval(s, "n").unwrap();
    assert_eq!((n.value.as_str(), n.default_type), (m.value.as_str(), WatchType::I64));
    assert_eq!(lt.eval(s, "stack0").unwrap().rendered, "stack0");
    assert_eq!(lt.eval(s, "stack0").unwrap().value, hex(lt.stack0()));
    assert_eq!(lt.eval(s, "stack0-0x10").unwrap().value, hex(lt.stack0() - 0x10));
    let plt = lt.eval(s, "printf@plt").unwrap();
    assert_eq!((plt.value, plt.default_type), (hex(lt.symbol_range("printf@plt").unwrap().0), WatchType::Ptr));
    let plus = lt.eval(s, "printf@plt+0x4").unwrap();
    assert_eq!((plus.rendered.as_str(), plus.default_type), ("printf@plt+0x4", WatchType::Value));
    // Rendered forms round-trip: the viewer feeds them back as addresses.
    for text in ["rsp", "rbp", "rip"] {
        let r = lt.eval(s, text).unwrap();
        assert_eq!(lt.eval(s, &r.rendered).unwrap().value, r.value, "{text}");
    }
    assert_eq!(lt.eval(s, "libc.so.6:printf").unwrap().rendered, "libc.so.6:printf");
    // `heap` exists once the program has allocated; before that it does not.
    let end = lt.steps();
    assert_eq!(lt.eval(end, "heap").unwrap().rendered, "heap+0x0");
    assert_eq!(lt.eval(0, "heap").unwrap_err().to_string(), "no heap at this step");
}

#[test]
fn eval_reports_what_is_wrong_with_an_expression() {
    let (_b, lt) = load();
    let s = lt.locate("fact+0xc@3").unwrap();
    let err = |text: &str| lt.eval(s, text).unwrap_err().to_string();
    assert_eq!(err("nosuch"), "unknown name 'nosuch'");
    assert_eq!(err("%nosuch"), "unknown register 'nosuch'");
    assert_eq!(err("libc.so.6:nosuch"), "unknown name 'libc.so.6:nosuch'");
    assert_eq!(err("rsp mod 0"), "mod by zero");
    assert_eq!(err(""), "the expression is empty");
    assert!(err("rsp +").contains("incomplete"), "{}", err("rsp +"));
    assert!(err("rsp mod").contains("number after 'mod'"), "{}", err("rsp mod"));
    assert!(err("(%rbp").contains("expected ')'"), "{}", err("(%rbp"));
    assert!(err("(%rbp,%rax,3)").contains("scale"), "{}", err("(%rbp,%rax,3)"));
    assert!(err("rsp rbp").contains("unexpected input"), "{}", err("rsp rbp"));
    assert!(err("12ab").contains("bad number"), "{}", err("12ab"));
    // `n` is only in scope inside fact.
    assert_eq!(lt.eval(0, "n").unwrap_err().to_string(), "unknown name 'n'");
    assert!(lt.eval(lt.steps() + 1, "rsp").is_err());
}

#[test]
fn watch_reads_the_expression_as_a_type_and_remembers_the_previous_value() {
    let (_b, lt) = load();
    let s = lt.locate("fact.c:4@3").unwrap();
    assert_eq!(s, lt.locate("fact+0xc@3").unwrap());
    let slot = lt.eval(s, "n").unwrap().value;
    let w = &lt.watch(s, &[("n".to_string(), Some(WatchType::I64))])[0];
    assert_eq!((w.value.as_deref(), w.raw.as_deref(), w.ty, w.addr.as_deref()), (Some("3"), Some("0x3"), WatchType::I64, Some(slot.as_str())));
    assert_eq!(w.error, None);
    // The previous instruction stored n, so the slot changed and held something else before.
    assert!(w.changed && w.prev.is_some() && w.prev.as_deref() != Some("3"), "{w:?}");
    let next = &lt.watch(s + 1, &[("n".to_string(), None)])[0];
    assert_eq!((next.ty, next.value.as_deref(), next.changed, next.prev.as_deref()), (WatchType::I64, Some("3"), false, Some("3")));

    let items: Vec<(String, Option<WatchType>)> = [
        ("rsp", None),
        ("rsp", Some(WatchType::Ptr)),
        ("-0x8(%rbp)", None),
        ("-0x8(%rbp)", Some(WatchType::U8)),
        ("-0x8(%rbp)", Some(WatchType::Bytes)),
        ("-0x8(%rbp)", Some(WatchType::F64)),
        ("nosuch", None),
        ("rbp-8", Some(WatchType::Str)),
    ]
    .into_iter()
    .map(|(e, t)| (e.to_string(), t))
    .collect();
    let ws = lt.watch(s, &items);
    assert_eq!(ws.len(), items.len());
    assert_eq!((ws[0].ty, ws[0].addr.as_deref(), ws[0].value.clone()), (WatchType::Value, None, Some(lt.render_value(lt.regs_at(s).rsp(), s))));
    assert_eq!(ws[0].raw, Some(format!("0x{:x}", lt.regs_at(s).rsp())));
    assert_eq!(ws[1].ty, WatchType::Ptr);
    assert_eq!(ws[1].addr.as_deref(), Some(format!("0x{:x}", lt.regs_at(s).rsp()).as_str()));
    assert_eq!((ws[2].ty, ws[2].value.as_deref()), (WatchType::Ptr, Some("3")));
    assert_eq!((ws[3].value.as_deref(), ws[3].raw.as_deref()), (Some("3"), Some("0x3")));
    assert!(ws[4].value.as_deref().is_some_and(|v| v.starts_with("03 00 00 00 00 00 00 00")), "{:?}", ws[4]);
    assert_eq!(ws[4].value.as_deref().unwrap().split(' ').count(), 16);
    assert_eq!(ws[5].raw.as_deref(), Some("0x3"));
    assert_eq!(ws[5].value.as_deref(), Some(f64::from_bits(3).to_string().as_str()));
    assert_eq!(ws[6].error.as_deref(), Some("unknown name 'nosuch'"));
    assert_eq!((ws[6].value.clone(), ws[6].changed), (None, false));
    assert_eq!(ws[7].value.as_deref(), Some("\"\\u{3}\""));
    // Registers change when an instruction writes them.
    let rsp = |at: u32| lt.watch(at, &[("rsp".to_string(), None)]).remove(0);
    // `push %rbp` ran just before fact+0x1; nothing touched %rsp between the stores and the compare.
    assert!(rsp(lt.locate("fact+0x1@1").unwrap()).changed);
    assert!(!rsp(s).changed, "{:?}", rsp(s));
    assert!(!lt.watch(0, &[("rax".to_string(), None)])[0].changed);
}

#[test]
fn read_typed_formats_each_type_and_gives_nothing_for_unmapped_bytes() {
    let (_b, lt) = load();
    let s = lt.locate("fact+0xc@3").unwrap();
    let slot = lt.regs_at(s).rbp() - 8;
    let t = |ty| lt.read_typed(s, slot, ty);
    assert_eq!((t(WatchType::U8).text.as_deref(), t(WatchType::U8).raw.as_deref()), (Some("3"), Some("0x3")));
    assert_eq!(t(WatchType::I8).text.as_deref(), Some("3"));
    assert_eq!(t(WatchType::U16).text.as_deref(), Some("3"));
    assert_eq!(t(WatchType::I32).raw.as_deref(), Some("0x3"));
    assert_eq!(t(WatchType::U64).text.as_deref(), Some("3"));
    assert_eq!(t(WatchType::F32).text.as_deref(), Some(f32::from_bits(3).to_string().as_str()));
    assert_eq!(t(WatchType::Ptr).text.as_deref(), Some("3"));
    // A pointer into the stack renders relative to stack0.
    let saved = lt.read_typed(s, slot + 8, WatchType::Ptr).text.unwrap();
    assert!(saved.starts_with("stack0"), "{saved}");
    // Signed reads sign-extend the stored bits: find a byte with the top bit set below the final %rsp.
    let end = lt.steps();
    let base = lt.stack0() - 0x800;
    let (at, byte) = lt.read_mem(end, base, 0x800).into_iter().enumerate().find_map(|(i, b)| b.filter(|&b| b >= 0x80).map(|b| (base + i as u64, b))).expect("a high byte");
    assert_eq!(lt.read_typed(end, at, WatchType::I8).text, Some((byte as i8).to_string()));
    assert_eq!(lt.read_typed(end, at, WatchType::U8).text, Some(byte.to_string()));
    assert_eq!(lt.read_typed(end, at, WatchType::I8).raw, Some(format!("0x{byte:x}")));
    // Unmapped memory reads as nothing for every type.
    for ty in [WatchType::U8, WatchType::I64, WatchType::Ptr, WatchType::Str, WatchType::Bytes] {
        assert_eq!(lt.read_typed(s, 0x10, ty), Default::default(), "{ty}");
    }
    // A string runs to its NUL and prints escaped: the stdout buffer holds the program's output.
    let write = lt.events().into_iter().find_map(|e| match e {
        EventDetail::Syscall { step, name, .. } if name == "write" => Some(step),
        _ => None,
    });
    let step = write.unwrap();
    let buf = lt.regs_at(step).gpr[4];
    assert_eq!(lt.read_typed(step, buf, WatchType::Str).text.as_deref(), Some("\"fact(5) = 120\\n\""));
    assert_eq!(lt.read_typed(step, buf, WatchType::Bytes).text.as_deref().map(|t| &t[..8]), Some("66 61 63"));
}

#[test]
fn inspect_describes_the_bytes_and_history_of_an_address() {
    let (_b, lt) = load();
    let s = lt.locate("fact+0xc@3").unwrap();
    let slot = &lt.stack_view(s, None, RootFrame::WhenInnermost).frames[3].slots[2];
    let addr = raw(&slot.raw_addr);
    let v = lt.inspect(s, addr).unwrap();
    assert_eq!((v.addr.as_str(), v.rendered.as_str()), (slot.raw_addr.as_str(), slot.addr.as_str()));
    assert!(v.region.starts_with("[stack] (rw"), "{}", v.region);
    assert_eq!(v.bytes.len(), 8);
    assert_eq!(v.bytes[0], Some(3));
    let types: Vec<WatchType> = v.values.iter().map(|x| x.ty).collect();
    use WatchType::*;
    assert_eq!(types, [U8, I8, U16, I16, U32, I32, U64, I64, F32, F64, Ptr]);
    let text = |ty: WatchType| v.values.iter().find(|x| x.ty == ty).unwrap().value.as_deref();
    assert_eq!((text(U8), text(I64), text(U64), text(Ptr)), (Some("3"), Some("3"), Some("3"), Some("3")));
    assert_eq!(v.writer, slot.writer);
    assert_eq!(v.writer_asm.as_deref(), Some("mov %rdi,-0x8(%rbp)"));
    assert_eq!(v.next_writer, lt.nav(s, &NavOp::WriteNext(addr)).unwrap());
    assert!(v.cstring.is_some());
    // Code: a file-backed region named after the executable.
    let rip = lt.regs_at(s).rip;
    let code = lt.inspect(s, rip).unwrap();
    assert!(code.region.ends_with(" (r-xp)") && !code.region.contains('/'), "{}", code.region);
    assert_eq!(code.writer, None);
    assert_eq!(code.writer_asm, None);
    // Nothing is mapped near zero.
    let none = lt.inspect(s, 0x10).unwrap();
    assert_eq!(none.region, "unmapped");
    assert!(none.bytes.iter().all(Option::is_none) && none.values.iter().all(|x| x.value.is_none()));
    assert_eq!((none.cstring, none.writer, none.next_writer), (None, None, None));
    assert!(lt.inspect(lt.steps() + 1, 0).is_err());
}

#[test]
fn timeline_windows_bucket_the_depth_and_keep_only_their_markers() {
    let (_b, lt) = load();
    let n = lt.steps();
    let full = lt.timeline(0, n).unwrap();
    assert_eq!((full.steps, full.from, full.to, full.depth.len()), (n, 0, n, 1000));
    assert!(full.markers.windows(2).all(|w| w[0].step <= w[1].step));
    let (from, to) = (n / 4, n / 2);
    let win = lt.timeline(from, to).unwrap();
    assert_eq!((win.steps, win.from, win.to), (n, from, to));
    assert_eq!(win.depth.len(), 1000.min((to - from + 1) as usize));
    assert!(win.markers.iter().all(|m| (from..=to).contains(&m.step)), "{:?}", win.markers);
    let expect: Vec<(u32, &str)> = full.markers.iter().filter(|m| (from..=to).contains(&m.step)).map(|m| (m.step, m.kind)).collect();
    assert_eq!(win.markers.iter().map(|m| (m.step, m.kind)).collect::<Vec<_>>(), expect);
    assert!(!expect.is_empty());
    // Small windows get one bucket per state; the depth is the state's own.
    let small = lt.timeline(7, 12).unwrap();
    assert_eq!(small.depth, (7..=12).map(|s| lt.state_view(s).unwrap().depth).collect::<Vec<_>>());
    assert_eq!(lt.timeline(9, 9).unwrap().depth.len(), 1);
    // The deepest call shows in a window around it.
    let deepest = (0..=n).max_by_key(|&s| lt.state_view(s).unwrap().depth).unwrap();
    let around = lt.timeline(deepest.saturating_sub(5), (deepest + 5).min(n)).unwrap();
    assert_eq!(around.depth.iter().max(), Some(&lt.state_view(deepest).unwrap().depth));
    assert!(lt.timeline(5, 4).is_err());
    assert!(lt.timeline(0, n + 1).is_err());
    let json = serde_json::to_value(&small).unwrap();
    assert_eq!((json["from"].as_u64(), json["to"].as_u64(), json["steps"].as_u64()), (Some(7), Some(12), Some(n as u64)));
}

#[test]
fn nav_view_reports_where_it_landed() {
    let (_b, lt) = load();
    let s = lt.locate("fact+0xc@3").unwrap();
    let v = lt.nav_view(s, &NavOp::NextLine).unwrap();
    let t = v.step.unwrap();
    assert_eq!((v.loc.as_deref(), v.asm.as_deref()), (Some(lt.loc(lt.rip_at(t)).as_str()), Some(lt.asm_at(t).as_str())));
    let line = NavOp::LineNext { file: "fact.c".into(), line: 4 };
    let by_line = lt.nav_view(s, &line).unwrap();
    assert_eq!(by_line.step, Some(lt.locate("fact.c:4@4").unwrap()));
    let none = lt.nav_view(lt.locate("fact.c:4@5").unwrap(), &line).unwrap();
    assert_eq!((none.step, none.loc.as_deref(), none.asm.as_deref()), (None, None, None));
    let json = serde_json::to_value(&none).unwrap();
    assert!(json["step"].is_null() && json["loc"].is_null() && json["asm"].is_null());
}

// ---- events ----

#[test]
fn the_write_syscall_shows_its_arguments_and_the_text_it_wrote() {
    let (_b, lt) = load();
    let events = lt.events();
    assert_eq!(events.len(), lt.trace.events.len());
    let write = events
        .iter()
        .find_map(|e| match e {
            EventDetail::Syscall { name, args, ret, ret_raw, errno, buffers, .. } if name == "write" => Some((args, ret, ret_raw, errno, buffers)),
            _ => None,
        })
        .expect("a write syscall");
    let (args, ret, ret_raw, errno, buffers) = write;
    assert_eq!(args.iter().map(|a| (a.reg, a.name.as_str())).collect::<Vec<_>>(), [("rdi", "fd"), ("rsi", "buf"), ("rdx", "count")]);
    assert_eq!((args[0].value.as_str(), args[0].note.as_deref()), ("1", Some("stdout")));
    assert_eq!((args[2].value.as_str(), args[2].raw.as_str()), ("14", "0xe"));
    assert_eq!((ret.as_deref(), ret_raw.as_deref(), errno.as_deref()), (Some("14"), Some("0xe"), None));
    assert_eq!(buffers.len(), 1);
    let b = &buffers[0];
    assert_eq!((b.arg.as_str(), b.dir, b.len, b.truncated), ("buf", "in", 14, false));
    assert_eq!(b.text.as_deref(), Some("fact(5) = 120\n"));
    let json = serde_json::to_value(&events).unwrap();
    assert!(json.as_array().unwrap().iter().any(|e| e["kind"] == "syscall" && e["name"] == "write"));
}

#[test]
fn signal_events_name_their_code_and_the_cpu_exception() {
    let b = build("sigfpe.c", FACT_FLAGS);
    let lt = LoadedTrace::load(&record(&b, &[])).unwrap();
    let sig = lt.events().into_iter().find(|e| matches!(e, EventDetail::Signal { .. })).expect("a signal event");
    match sig {
        EventDetail::Signal { step, signal, code, code_name, code_text, cpu_exception, addr } => {
            assert_eq!(lt.step_kind(step), "fault");
            assert_eq!((signal.as_str(), code, code_name.as_deref()), ("SIGFPE", 1, Some("FPE_INTDIV")));
            assert_eq!(code_text.as_deref(), Some("integer divide by zero"));
            assert_eq!(cpu_exception.as_deref(), Some("#DE (divide error)"));
            assert_eq!(addr, lt.render_value(lt.regs_at(step).rip, step), "{addr}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_failed_syscall_names_its_errno_and_the_path_it_was_given() {
    let src = "#include <fcntl.h>\nint main(void) { return open(\"/nonexistent/x\", O_RDONLY) < 0 ? 0 : 1; }\n";
    let (_b, lt) = load_source("open.c", src);
    let (args, ret, ret_raw, errno, errno_text, buffers) = lt
        .events()
        .into_iter()
        .find_map(|e| match e {
            EventDetail::Syscall { name, args, ret, ret_raw, errno, errno_text, buffers, .. } if name == "openat" || name == "open" => {
                Some((args, ret, ret_raw, errno, errno_text, buffers))
            }
            _ => None,
        })
        .expect("an open syscall");
    assert_eq!((ret.as_deref(), ret_raw.as_deref()), (Some("-2"), Some("0xfffffffffffffffe")));
    assert_eq!((errno.as_deref(), errno_text.as_deref()), (Some("ENOENT"), Some("No such file or directory")));
    assert!(args.iter().any(|a| a.name == "pathname"));
    let path = buffers.iter().find(|b| b.arg == "pathname").unwrap();
    assert_eq!((path.dir, path.text.as_deref(), path.len, path.truncated), ("in", Some("/nonexistent/x"), 14, false));
}

#[test]
fn rt_sigreturn_returns_no_value() {
    let (_b, lt) = load_source("sig.c", SIGNAL_C);
    let ev = lt
        .events()
        .into_iter()
        .find_map(|e| match e {
            EventDetail::Syscall { name, ret, ret_raw, .. } if name == "rt_sigreturn" => Some((ret, ret_raw)),
            _ => None,
        })
        .expect("the handler returns through rt_sigreturn");
    assert_eq!(ev, (None, None));
}

#[test]
fn effects_skip_vector_registers_the_recording_does_not_hold() {
    // EVEX instructions name xmm16..31, which the trace never records. Without AVX-512 the first one
    // faults with SIGILL; with it the program runs on.
    let dir = tempfile::tempdir().unwrap();
    let src = "\t.globl _start\n\t.text\n_start:\n\tvmovdqa64 %xmm16, %xmm1\n\tvmovdqa64 %xmm1, %xmm17\n\tmov $60, %eax\n\txor %edi, %edi\n\tsyscall\n";
    std::fs::write(dir.path().join("evex.s"), src).unwrap();
    let out = std::process::Command::new("gcc").current_dir(dir.path()).args(ASM_FLAGS).args(["-o", "prog", "evex.s"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let b = Built { exe: dir.path().join("prog"), dir };
    let lt = LoadedTrace::load(&record(&b, &[])).unwrap();
    assert_eq!(lt.effects(0).unwrap().mnemonic, "vmovdqa64");
    for s in 0..lt.steps() {
        let e = lt.effects(s).unwrap();
        let unrecorded = |name: &str| (16..32).any(|n| name == format!("xmm{n}"));
        assert!(e.reg_reads.iter().all(|r| !unrecorded(&r.name)), "{:?}", e.reg_reads);
        assert!(e.reg_writes.iter().all(|w| !unrecorded(&w.name)), "{:?}", e.reg_writes);
        lt.state_view(s).unwrap();
    }
}

#[test]
fn effects_and_events_cover_every_step_of_every_recordable_fixture() {
    // Whatever a program does (faults, signals, string instructions, vector code, raw syscalls), no
    // step may break the views built on it.
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut names: Vec<String> = std::fs::read_dir(fixtures).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    let mut steps = 0;
    for name in names.iter().filter(|n| !matches!(n.as_str(), "fork_child.c" | "sigstop_self.c")) {
        let b = build(name, if name.ends_with(".s") { ASM_FLAGS } else { FACT_FLAGS });
        let lt = LoadedTrace::load(&record(&b, &[])).unwrap();
        for s in 0..lt.steps() {
            let e = lt.effects(s).unwrap_or_else(|| panic!("{name}: no effects at step {s}"));
            assert_eq!(e.kind, lt.step_kind(s), "{name} step {s}");
            assert_eq!(e.next_loc, lt.loc(lt.rip_at(s + 1)), "{name} step {s}");
            if s % 257 == 0 || e.kind != "instr" {
                lt.state_view(s).unwrap_or_else(|err| panic!("{name}: state {s}: {err}"));
            }
        }
        assert!(lt.effects(lt.steps()).is_none());
        lt.state_view(lt.steps()).unwrap();
        assert_eq!(lt.events().len(), lt.trace.events.len(), "{name}");
        serde_json::to_string(&lt.events()).unwrap();
        steps += lt.steps();
    }
    assert!(steps > 10_000, "{steps} steps checked");
}
