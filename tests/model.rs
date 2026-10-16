//! The public model API the HTTP server routes to.

mod common;

use common::*;
use systemslab::model::{LoadedTrace, NavOp, RootFrame};

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
    assert!(rax_next >= s);
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
    // The instruction about to run (`mov %rsp,%rbp`) writes no memory, so `writes` differs.
    assert!(v.writes.is_empty());
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
