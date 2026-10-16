mod common;

use common::*;
use serde_json::Value;

#[test]
fn fact_records_strict_and_info_says_exit_0_from_main() {
    let b = build("fact.c", FACT_FLAGS);
    let t = record(&b, &[]);
    let info: Value = serde_json::from_str(&ok(&["info", "--json", p(&t)])).unwrap();
    assert_eq!(info["end"]["reason"], "exit");
    assert_eq!(info["end"]["code"], 0);
    assert_eq!(info["unattributed_bytes"], 0);
    assert_eq!(info["from"], "main");
    let text = ok(&["info", p(&t)]);
    assert!(text.contains(&format!("{:<20}{}", "end", "exit 0")), "{text}");
    assert!(text.contains(&format!("{:<20}{}", "unattributed bytes", "0")), "{text}");
    assert!(text.contains(&format!("{:<20}{}", "signals", "none")), "{text}");
}

#[test]
fn uname_syscall_writes_the_kernel_buffer_on_the_stack() {
    let b = build("uname.s", ASM_FLAGS);
    let t = record(&b, &[]);
    let lines = json_lines(&ok(&["steps", p(&t), "--json"]));
    let step = lines
        .iter()
        .find(|l| l["event"]["kind"] == "syscall" && l["event"]["name"] == "uname")
        .expect("uname step");
    assert_eq!(step["event"]["ret"], "0");
    let kernel_stack_writes = step["writes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|w| w["kind"] == "kernel" && w["addr"].as_str().unwrap().starts_with("stack0"))
        .count();
    assert!(kernel_stack_writes >= 1, "{step}");
}

#[test]
fn rep_stos_is_fully_attributed() {
    let b = build("repstos.s", ASM_FLAGS);
    let o = run(&["record", "--strict", "--quiet", "-o", p(&b.dir.path().join("t.slt")), "--", p(&b.exe)]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
}

#[test]
fn sigfpe_ends_with_the_signal_on_an_idiv() {
    let b = build("sigfpe.c", &["-O0", "-g"]);
    let t = record(&b, &[]);
    let info: Value = serde_json::from_str(&ok(&["info", "--json", p(&t)])).unwrap();
    assert_eq!(info["end"]["reason"], "signal");
    assert_eq!(info["end"]["signal"], "SIGFPE");
    let text = ok(&["info", p(&t)]);
    assert!(text.contains(&format!("{:<20}{}", "end", "signal SIGFPE")), "{text}");
    let lines = json_lines(&ok(&["steps", p(&t), "--json", "--limit", "100000"]));
    let step = lines.iter().find(|l| l["event"]["kind"] == "signal").expect("signal step");
    assert!(step["loc"].as_str().unwrap().starts_with("main+"), "{step}");
    assert!(step["asm"].as_str().unwrap().contains("idiv"), "{step}");
    assert_eq!(step["event"]["signal"], "SIGFPE");
    let plain = ok(&["steps", p(&t), "--limit", "100000"]);
    assert!(plain.lines().any(|l| l.contains("signal SIGFPE code") && l.contains(" at main+")), "{plain}");
}

#[test]
fn from_symbol_starts_at_that_symbol() {
    let b = build("fact.c", FACT_FLAGS);
    let t = record(&b, &["--from", "fact"]);
    let lines = json_lines(&ok(&["steps", p(&t), "--json", "--limit", "1"]));
    assert_eq!(lines[0]["loc"], "fact");
    assert_eq!(lines[0]["step"], 0);
}

#[test]
fn recording_twice_gives_identical_stack_text() {
    let b = build("fact.c", FACT_FLAGS);
    let t1 = record_named(&b, "a.slt", &[]);
    let t2 = record_named(&b, "b.slt", &[]);
    let a = ok(&["stack", p(&t1), "fact+0xc@3"]);
    let c = ok(&["stack", p(&t2), "fact+0xc@3"]);
    assert_eq!(a, c);
}

#[test]
fn from_entry_is_deterministic_for_check_abi_and_steps() {
    let b = build("fact.c", FACT_FLAGS);
    let t1 = record_named(&b, "a.slt", &["--from", "entry"]);
    let t2 = record_named(&b, "b.slt", &["--from", "entry"]);
    assert_eq!(ok(&["check-abi", p(&t1)]).trim(), "no findings");
    // The total step count of a from-entry run varies by about a hundred steps between runs
    // (the dynamic loader branches on random bytes), so only the listed steps are compared.
    let first = |t| ok(&["steps", p(t), "--limit", "5"]).lines().filter(|l| l.starts_with('#')).map(str::to_string).collect::<Vec<_>>();
    assert_eq!(first(&t1).len(), 5);
    assert_eq!(first(&t1), first(&t2));
    assert_eq!(ok(&["check-abi", p(&t1)]), ok(&["check-abi", p(&t2)]));
}

fn steps_json(t: &std::path::Path) -> Vec<Value> {
    json_lines(&ok(&["steps", p(t), "--json", "--limit", "10000000"]))
}

/// The step index following the first signal event, and its JSON.
fn deliver_step(lines: &[Value]) -> (usize, &Value) {
    let i = lines.iter().position(|l| l["event"]["kind"] == "signal").expect("signal event");
    (i + 1, &lines[i + 1])
}

#[test]
fn genuine_sigtrap_reaches_the_program() {
    let b = build("sigtrap.c", FACT_FLAGS);
    let native = std::process::Command::new(&b.exe).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&native.stdout), "handler ran 2 times\n");
    let out = b.dir.path().join("t.slt");
    let o = run(&["record", "--strict", "-o", p(&out), "--", p(&b.exe)]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(stdout(&o), "handler ran 2 times\n", "recorded run must behave like the native run");
    let lines = steps_json(&out);
    let traps = lines.iter().filter(|l| l["event"]["signal"] == "SIGTRAP").count();
    assert!(traps >= 1, "int3 must be a signal event");
    let int3 = lines.iter().find(|l| l["asm"] == "int3").expect("int3 step");
    assert_eq!(int3["event"]["kind"], "signal");
}

#[test]
fn sigtrap_before_the_start_symbol_is_forwarded() {
    // raise() runs inside main; starting from `main` must not eat a later trap,
    // and starting from entry must not lose it either.
    let b = build("sigtrap.c", FACT_FLAGS);
    let out = b.dir.path().join("e.slt");
    let o = run(&["record", "--from", "entry", "--strict", "-o", p(&out), "--", p(&b.exe)]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(stdout(&o), "handler ran 2 times\n");
}

#[test]
fn handler_entry_is_kernel_work_not_the_interrupted_instruction() {
    let b = build("sigsegv_push.c", FACT_FLAGS);
    let t = record(&b, &[]);
    let info: Value = serde_json::from_str(&ok(&["info", "--json", p(&t)])).unwrap();
    assert_eq!(info["end"]["reason"], "exit");
    assert_eq!(info["end"]["code"], 3);
    let lines = steps_json(&t);
    let (_, step) = deliver_step(&lines);
    let writes = step["writes"].as_array().unwrap();
    assert!(!writes.is_empty(), "the signal frame must be recorded: {step}");
    assert!(writes.iter().all(|w| w["kind"] == "kernel"), "frame writes are kernel writes: {step}");
    assert!(step["event"].is_null(), "no syscall event at the handler-entry step: {step}");
}

#[test]
fn signal_frame_that_grows_the_stack_is_recorded() {
    let b = build("sigfpe_edge.c", FACT_FLAGS);
    let t = record(&b, &[]);
    let info: Value = serde_json::from_str(&ok(&["info", "--json", p(&t)])).unwrap();
    assert_eq!(info["end"]["code"], 3);
    let lines = steps_json(&t);
    let (_, step) = deliver_step(&lines);
    let writes = step["writes"].as_array().unwrap();
    assert!(!writes.is_empty(), "pages grown by the signal frame must be captured at the deliver step: {step}");
    assert!(writes.iter().all(|w| w["addr"].as_str().unwrap().starts_with("stack0-")), "{step}");
}

#[test]
fn map_fixed_replacement_of_a_file_mapping_is_snapshotted() {
    let b = build("map_replace.c", FACT_FLAGS);
    let t = record(&b, &[]);
    let trace = systemslab::trace::load(&t).unwrap();
    let snap = |needle: &[u8]| {
        trace.writes.iter().any(|w| {
            w.kind == systemslab::trace::WriteKind::Snapshot && w.addr == 0x1000_0000 && w.data.starts_with(needle)
        })
    };
    assert!(snap(b"AAAA"), "first mapping");
    assert!(snap(b"BBBB"), "replacement mapping at the same address");
}

#[test]
fn writes_into_a_grown_stack_page_keep_their_step() {
    use systemslab::model::LoadedTrace;
    use systemslab::trace::WriteKind;
    let b = build("stack_grow.c", FACT_FLAGS);
    let t = record(&b, &[]);
    let lt = LoadedTrace::load(&t).unwrap();
    let stack_lo = lt.maps_at(0).regions.iter().find(|r| r.path == "[stack]").expect("[stack] region").start;
    let mut checked = 0;
    for i in 0..lt.steps() {
        for w in lt.writes_of_step(i).iter().filter(|w| w.kind == WriteKind::Instr && w.addr < stack_lo) {
            let at = format!("write of step {i} at {:#x} below the initial stack", w.addr);
            assert_eq!(lt.last_writer(i + 1, w.addr), Some(i), "{at}");
            checked += 1;
        }
    }
    assert!(checked > 0, "the fixture must write below the initial [stack] mapping");
}

#[test]
fn a_program_that_stops_itself_is_unsupported_not_an_internal_error() {
    let b = build("sigstop_self.c", FACT_FLAGS);
    for (args, when) in [(&[][..], "inside main"), (&["early"][..], "before main")] {
        let out = b.dir.path().join("stop.slt");
        let mut cmd = vec!["record", "--quiet", "-o", p(&out), "--", p(&b.exe)];
        cmd.extend_from_slice(args);
        let o = run(&cmd);
        assert_eq!(o.status.code(), Some(3), "{when}: {}", stderr(&o));
        let err = stderr(&o);
        assert!(err.contains("SIGSTOP") && err.contains("stopped"), "{when}: {err}");
        assert!(!out.exists(), "{when}: no trace is written for an unsupported program");
    }
}

#[test]
fn string_instructions_record_one_write_per_iteration_in_either_direction() {
    use systemslab::model::LoadedTrace;
    use systemslab::trace::WriteKind;
    let b = build("repstring.s", ASM_FLAGS);
    let t = record(&b, &[]);
    let lt = LoadedTrace::load(&t).unwrap();
    let mut iterations = 0;
    for i in 0..lt.steps() {
        let bytes = &lt.trace.insns[lt.trace.steps[i as usize].insn as usize].bytes;
        let prefixes = bytes.iter().take_while(|&&x| matches!(x, 0x66 | 0xf2 | 0xf3 | 0x40..=0x4f)).count();
        let Some(&op) = bytes.get(prefixes).filter(|&&x| matches!(x, 0xa4 | 0xa5 | 0xaa | 0xab)) else { continue };
        let (rex_w, opsize) = (bytes[..prefixes].iter().any(|&x| x & 0xf8 == 0x48), bytes[..prefixes].contains(&0x66));
        let elem: u64 = match (op, rex_w, opsize) {
            (0xa4 | 0xaa, ..) => 1,
            (_, true, _) => 8,
            (_, _, true) => 2,
            _ => 4,
        };
        let (before, after) = (lt.regs_at(i), lt.regs_at(i + 1));
        if before.gpr[5] == after.gpr[5] {
            continue; // no iteration ran (rcx was 0)
        }
        let writes = lt.writes_of_step(i);
        let at = format!("step {i} {bytes:02x?} rdi {:#x} -> {:#x}", before.gpr[5], after.gpr[5]);
        assert_eq!(writes.len(), 1, "{at}: {writes:?}");
        let w = &writes[0];
        assert_eq!((w.kind, w.addr, w.data.len() as u64), (WriteKind::Instr, before.gpr[5], elem), "{at}");
        iterations += 1;
    }
    assert_eq!(iterations, 128 + 100 + 10 + 1 + 16, "every iteration of every string instruction was checked");
}

#[test]
fn legacy_int_0x80_syscall_memory_writes_are_recorded() {
    use systemslab::model::LoadedTrace;
    use systemslab::trace::WriteKind;
    let b = build("int80_uname.s", ASM_FLAGS);
    let t = record(&b, &[]);
    let lt = LoadedTrace::load(&t).unwrap();
    let bytes_of = |i: u32| &lt.trace.insns[lt.trace.steps[i as usize].insn as usize].bytes;
    let int80 = (0..lt.steps()).find(|&i| bytes_of(i)[..] == [0xcd, 0x80]).expect("int $0x80 step");
    let buf = lt.regs_at(int80).gpr[1]; // %rbx: the buffer
    let writes = lt.writes_of_step(int80);
    let covers = |w: &&systemslab::trace::MemWrite| {
        w.kind == WriteKind::Kernel && (w.addr..w.addr + w.data.len() as u64).contains(&buf)
    };
    let written = writes.iter().find(covers);
    let written = written.unwrap_or_else(|| panic!("int $0x80 at step {int80} wrote nothing: {writes:?}"));
    assert!(written.data[(buf - written.addr) as usize..].starts_with(b"Linux"), "{:?}", written.data);
    assert_eq!(lt.last_writer(int80 + 1, buf), Some(int80), "the kernel write is attributed to the int $0x80 step");
}

#[test]
fn single_stepping_does_not_leak_the_trap_flag_into_the_program() {
    let b = build("trap_flag_leak.s", ASM_FLAGS);
    let native = std::process::Command::new(&b.exe).status().unwrap();
    assert_eq!(native.code(), Some(0), "natively TF is clear in the pushed flags and in %r11");
    let t = record(&b, &[]);
    let info: Value = serde_json::from_str(&ok(&["info", "--json", p(&t)])).unwrap();
    assert_eq!(info["end"]["code"], 0, "pushfq pushed TF (bit 0) and/or syscall left TF in %r11 (bit 1)");
}

#[test]
fn a_hand_written_main_label_is_where_recording_starts() {
    let b = build("asm_main.s", &[]);
    for extra in [&[][..], &["--from", "main"][..]] {
        let t = record(&b, extra);
        let info: Value = serde_json::from_str(&ok(&["info", "--json", p(&t)])).unwrap();
        assert_eq!(info["from"], "main", "{extra:?}");
        assert_eq!(info["end"]["code"], 42);
        assert!(info["steps"].as_u64().unwrap() < 5000, "{extra:?}: recorded the dynamic loader too: {info}");
    }
}

#[test]
fn an_unsupported_fork_leaves_no_traced_child_behind() {
    let b = build("fork_child.c", FACT_FLAGS);
    let opts = systemslab::record::Options {
        from: "auto".into(),
        max_steps: 2_000_000,
        strict: false,
        quiet: true,
        output: b.dir.path().join("fork.slt"),
        program: p(&b.exe).into(),
        args: Vec::new(),
    };
    let err = systemslab::record::record(&opts).err().expect("fork is unsupported");
    assert_eq!(systemslab::exit::code_of(&err), systemslab::exit::UNSUPPORTED, "{err:#}");
    // ptrace reports the tracer as a thread id: this test's thread did the tracing.
    let me = unsafe { libc::gettid() };
    let still_traced: Vec<String> = std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| {
            let status = std::fs::read_to_string(e.path().join("status")).ok()?;
            let tracer = status.lines().find_map(|l| l.strip_prefix("TracerPid:"))?.trim().to_string();
            (tracer == me.to_string()).then(|| e.file_name().to_string_lossy().into_owned())
        })
        .collect();
    assert!(still_traced.is_empty(), "still traced by this thread after the error: {still_traced:?}");
}
