# SystemsLab

SystemsLab records a real x86-64 Linux program one machine instruction at a time and
lets you inspect the recording afterwards. It single-steps the program with `ptrace` and
saves every register change and memory write. You can then:

- print the stack frames at any instruction,
- list what each instruction changed,
- check the recorded calls against System V ABI rules,
- generate Graphviz figures of the stack,
- step forwards *and backwards* through the run in a browser viewer, and jump to the
  instruction that last wrote a register or memory byte.

Everything comes from the real execution, not from an emulator.

## Requirements

- Linux on x86-64. Tracing your own child process must be allowed: Yama
  `kernel.yama.ptrace_scope` must be 0 or 1, which is the default on most distributions.
- Rust, stable (developed with 1.99).
- [bun](https://bun.sh), to build the browser viewer. `build.rs` looks for it on `PATH` and
  in `~/.bun/bin`.
- gcc for compiling the programs you study. Graphviz (`dot`) to turn figures into images.
  The test suite needs both.

## Install

```sh
cargo install --git https://github.com/sadisticbrew/systems_lab --locked
```

From a checkout, the following installs to `~/.local/bin`:

```sh
cargo install --path . --root ~/.local --locked
```

For a command-line-only build without bun, set `SYSTEMSLAB_SKIP_WEB=1`. The viewer then
serves a placeholder page.

## Quick start

```c
// fact.c
#include <stdio.h>

long fact(long n) {
    if (n <= 1) return 1;
    return n * fact(n - 1);
}

int main(void) {
    printf("fact(5) = %ld\n", fact(5));
    return 0;
}
```

```console
$ gcc -O0 -g -fcf-protection=none -o fact fact.c
$ systemslab record -o fact.slt -- ./fact
fact(5) = 120
recorded 3413 steps from main; ended: exit 0; unattributed bytes: 0; wrote fact.slt

$ systemslab stack fact.slt 'fact+0xc@3'
step 28 at fact+0xc (cmpq $0x1,-0x8(%rbp))
main's frame
fact(5), 32 bytes
     8(%rbp)  return address: movq in main   main+0xe
     0(%rbp)  saved %rbp: main's             stack0-0x8
    -8(%rbp)  n = 5                          5
   -16(%rbp)  unused                         -
fact(4), 32 bytes
     8(%rbp)  return address: imulq in fact  fact+0x2a
     0(%rbp)  saved %rbp: fact(5)'s          stack0-0x18
    -8(%rbp)  n = 4                          4
   -16(%rbp)  unused                         -
fact(3), 32 bytes
     8(%rbp)  return address: imulq in fact  fact+0x2a
     0(%rbp)  saved %rbp: fact(4)'s          stack0-0x38  ← %rbp
    -8(%rbp)  n = 3                          3
   -16(%rbp)  unused                         -            ← %rsp

$ systemslab view fact.slt     # prints the viewer's URL and opens it in your browser
```

`systemslab run` does all three steps at once: it compiles the source with the flags below,
records it, and opens the viewer, which rebuilds and re-records whenever you save the file:

```console
$ systemslab run fact.c
```

- `-O0 -g` gives ordinary frame-pointer frames and the debug info that names `n`.
- `-fcf-protection=none` drops the `endbr64` that would otherwise start every function.

Text output names addresses instead of printing them. Stack addresses are relative to
`stack0`, the value of `%rsp` when recording started. Code and data addresses are
`symbol+offset`, and heap addresses are `heap+offset`. Stack, heap and code addresses
therefore print the same in every recording of a program, even with ASLR on. Pointers into
other anonymous memory (such as the thread-local area) and values that are random by
nature (such as stack canaries) print as hex and can differ.

## Naming a moment: locators

`stack`, `dot stack` and `steps --from/--to` take a locator:

| Locator | Meaning |
|---|---|
| `step:28` | step 28 |
| `step:end` | the final state, after the last instruction |
| `fact` | the first time the first instruction of `fact` is about to run |
| `fact+0xc@3` | the third time the instruction at `fact+0xc` is about to run |
| `fact.c:4@3` | the third time execution enters line 4 of `fact.c` |
| `printf@plt` | the PLT stub through which the program calls `printf` |
| `libc.so.6:printf` | a symbol in a shared library |
| `0x401136@2` | the second time the instruction at that address is about to run |

Source-line locators survive recompiling with other flags; `SYMBOL+0xOFF` locators do not.

To find offsets, list the steps of a function:

```console
$ systemslab steps fact.slt --func fact --limit 6
#4 fact  push %rbp rsp=stack0-0x18 stack0-0x18←stack0-0x8/8
#5 fact+0x1  mov %rsp,%rbp rbp=stack0-0x18
#6 fact+0x4  sub $0x10,%rsp rsp=stack0-0x28 rflags=[PF IF]
#7 fact+0x8  mov %rdi,-0x8(%rbp) stack0-0x20←5/8
#8 fact+0xc  cmpq $0x1,-0x8(%rbp) rflags=[IF]
#9 fact+0x11  jg fact+0x1a
… 56 more steps
```

Each line shows the step number, its location and instruction, the registers it changed,
and its memory writes as `address←value/size`.

## Commands

| Command | What it does |
|---|---|
| `record [--from auto\|main\|entry\|SYMBOL] [--max-steps N] [--strict] [--quiet] [-o FILE] -- PROGRAM [ARGS...]` | Run the program under the recorder and write a `.slt` trace. Recording starts at `main` by default. The default step limit is 2,000,000. |
| `info TRACE [--json]` | Summarize a trace: program, step count, how it ended, syscalls, signals. |
| `steps TRACE [--from LOC] [--to LOC] [--func NAME] [--limit N] [--json]` | List the steps with their register changes, memory writes, syscalls and signals. |
| `stack TRACE LOC [--frames N] [--json]` | Show the stack frames and slots at a step. |
| `dot stack TRACE LOC [--frames N]` | Write the same stack as a Graphviz figure, without colors or fonts so your own theme applies. |
| `check-abi TRACE [--all] [--json]` | Check the recorded calls for System V ABI violations. |
| `check-guide TRACE GUIDE [--json]` | Check that every `@go` and `@predict` directive of a guide resolves in a trace, and print the step and recorded value each one points at. Exits 1 when a directive fails. |
| `compare A.slt B.slt [--json]` | Compare two recordings, for example of one program built with `-O0` and `-O2`: steps, how they ended, whether the output and syscall sequences are the same, calls and steps per function, and the first call, syscall or signal where they diverge. |
| `view PATH [--port P] [--no-open] [--at LOC] [--guide FILE]` | Serve the browser viewer for a trace, or for a lab directory whose `out/` holds one trace. `--at` opens it at a locator. The viewer reloads when the trace file is replaced. |
| `run SOURCE [-O LEVEL] [--from WHERE] [--max-steps N] [-o TRACE] [--guide FILE] [--port P] [--no-open] [--no-view] [--no-watch] [-- ARGS...]` | Compile a `.c`, `.s` or `.S` file with `gcc -O0 -g -fcf-protection=none` (`$CC` overrides gcc), record it next to the source (`fact.c` → `fact` and `fact.slt`), and open the viewer. Saving the source rebuilds and re-records it; a build error is shown in the viewer while the previous recording stays loaded. Assembly that defines `_start` is linked with `-nostdlib -static`. |
| `try [NAME] [--port P] [--no-open] [--no-view]` | List the bundled experiments, or copy one to `~/.local/share/systemslab/experiments/NAME/` (only files you do not have yet, so your edits stay) and `run` it with its guide. |

`check-abi` checks four things for calls in the main executable (`--all` checks every call):

- `%rsp` is 16-byte aligned at each `call`;
- `%rbx`, `%rbp`, `%r12`–`%r15` and `%rsp` are the same after the callee returns;
- the direction flag is clear at calls and returns;
- every `ret` goes back to the address its `call` pushed.

```console
$ systemslab check-abi abi_bad.slt
call-alignment step 3 _start+0x9: %rsp mod 16 = 8 before call to misaligned (must be 0)
callee-saved step 7 _start+0x19: clobber changed %rbx: 7 at the call, 99 after its ret (step 9)
2 findings
```

Exit codes:

| Code | Meaning |
|---|---|
| 0 | ok |
| 1 | `check-abi` found violations, or `check-guide` found a directive that does not resolve |
| 2 | usage, file or locator error |
| 3 | the program did something unsupported (threads, `fork`, `exec`, stopping itself) |
| 4 | `--strict` found memory changes it could not attribute |

## The viewer

`systemslab view` (and `run`, `try`) opens a page with the source, the disassembly, the
registers, the stack, a memory dump, and a timeline of the whole run. Step N is the machine
state just before instruction N runs.

- **Effects** explains the instruction about to run, from the recording: the registers and
  memory it reads and writes (`%rax (via %eax): 0xffffffffffffffff → 0x11223344`), the flags
  it sets, whether a conditional jump's condition holds, what a `call` pushes and whether
  `%rsp` is 16-byte aligned, and the arguments, buffers and result of a syscall. Press `p` to
  predict each instruction's effect before it is revealed.
- **Click to inspect.** Click a register, memory byte or stack slot to see it in every
  width and signedness, where it last changed and where it changes next. `[` and `]` jump
  there; Backspace returns to where you were before the jump.
- **Timeline.** Call depth over the whole run, with marks for calls, returns, syscalls,
  signals, ABI findings and your bookmarks. Toggle the marks in its legend, hover for
  labels, scroll to zoom, drag to scrub.
- **Bottom tabs.** Memory, the syscall and signal events, the program's output up to the
  current step, ABI findings with explanations, your notes and bookmarks, and the guide.
- **Your place is kept.** Notes, bookmarks, watches and the last position are saved per
  trace under `~/.local/share/systemslab/notes/`, panel sizes in
  `~/.config/systemslab/viewer.json`. The URL carries `#step=N`, so a moment can be bookmarked
  in the browser. When the trace is re-recorded (`run` after a save, or a new `record -o`
  over a viewed trace), the viewer reloads and stays at the same instruction.

| Key | Action |
|---|---|
| `→` / `l`, `←` / `h` | next or previous instruction |
| `n` / `N` | next or previous source line |
| `s` / `S` | step over a call, forwards or backwards |
| `o` / `O` | step out of the function, forwards or backwards |
| `[` / `]` | previous or next change of the selected register, byte or slot |
| `Backspace` / `Shift+Backspace` | back or forward in the jump history |
| `g` | type a locator |
| `m` | bookmark this step |
| `p` | predict mode |
| `Esc` | clear the selection |
| `?` | help |
| `Home` / `End` | first or last step |

## Guides and expressions

A guide is a Markdown file the viewer shows in its Guide tab (`view --guide FILE`, `run --guide FILE`, or `NAME.guide.md` next to the trace). Two directives, each alone on a line starting in column 0, make it interactive:

- `@go LOCATOR` is a button that jumps to that moment.
- `@predict EXPR [as TYPE] at LOCATOR [after]` asks the learner for the value of `EXPR` before showing the recorded one. With `after` it is the value after the instruction at `LOCATOR` ran. The lines starting with `> ` that follow explain the answer and stay hidden until it is checked.

`systemslab check-guide` resolves every directive against a recording, so a guide that stops matching its program is caught before a learner sees it.

Expressions are used by guides, the viewer's watch list and its memory address box. They are made of registers of any width (`rax`, `%eax`, `al`, `rip`, `rflags`), C variables in scope (`n`, which evaluates to its address), symbols (`printf@plt`, `libc.so.6:printf`), `stack0` and `heap`, numbers, `+`, `-`, `mod`, and AT&T memory operands such as `-0x8(%rbp)` or `(%rax,%rcx,8)`. A memory operand is an address, so `-0x8(%rbp) as i64` reads the 8 bytes there. `TYPE` is one of `value` (the expression's own value), `u8`, `i8`, `u16`, `i16`, `u32`, `i32`, `u64`, `i64`, `f32`, `f64`, `ptr`, `str` or `bytes`. Without it, a C variable is read as its declared type, a memory operand or symbol as a pointer, and anything else as a value.

## How it works

- **Recording.** The recorder forks the program with ASLR disabled and runs it to the start
  point, then single-steps it. Before each instruction runs, [iced-x86](https://github.com/icedland/iced)
  decodes it and works out which bytes it will write. After the step, those bytes are read
  back from the process.
- **Kernel writes.** The recorder keeps a copy of all writable memory. After syscalls,
  faults and signal deliveries it compares that copy with the process to catch what the
  kernel wrote. It also compares periodically and before exit.
- **`--strict`.** Any change that neither an instruction nor the kernel accounts for is
  counted as "unattributed bytes". With `--strict` the recording then exits with code 4.
- **Trace files.** A `.slt` trace is self-contained. It holds the executable's bytes, the
  symbols of the loaded libraries, and the program's source files, so you can still view
  it after the binary is deleted. Traces are zstd-compressed; the fact example above is
  about 1.4 MB.
- **Reconstructing a state.** The state at any step is built from register checkpoints
  (one every 1,024 steps) plus deltas, and from the memory writes recorded up to that step.
- **Stack frames.** Frames come from the recorded `call`/`ret` pairs and from the kernel
  entering signal handlers, not from unwind tables. A frame that is left without a `ret`
  (`longjmp`) ends when `%rsp` moves above its return address. Slot labels come from the
  recorded writes (a `push %rbp` is a saved `%rbp`) and from the DWARF locations of
  parameters and local variables. A frame of more than 512 slots (a large array on the
  stack) lists its first and last 256.

## Limitations

- x86-64 Linux only.
- Single-threaded programs only. `clone`, `fork`, `vfork` and `execve` stop the recording
  with exit code 3. So does a program that stops itself with `SIGSTOP` or `SIGTSTP`,
  because only another process could resume it.
- 32-bit `int $0x80` system calls are recorded with their memory effects but without a
  syscall name.
- Single-stepping is slow: about 12 µs per instruction in a release build. Keep the
  programs you study small, or use `--from SYMBOL` to skip startup.
- Variable names and values in stack labels need simple DWARF locations (`DW_OP_fbreg`),
  which in practice means `-O0`. Optimized code still gets return addresses, saved
  registers and raw values.
- A program that handles `SIGTRAP` must install its handler with `SA_NODEFER`. The kernel
  resets a blocked `SIGTRAP` handler while the program is being single-stepped; gdb has
  the same limitation.

## Development

```sh
cargo test                            # needs gcc, dot and bun on PATH
SYSTEMSLAB_SKIP_WEB=1 cargo test      # skip the viewer build
```

To work on the viewer with hot reload, start the server on port 7777 and run Vite, which
proxies `/api` to it:

```sh
systemslab view trace.slt --no-open --port 7777
cd web && bun install && bun run dev
```

`bun run check` type-checks the frontend.

Layout:

| Path | Contents |
|---|---|
| `src/record/` | ptrace recorder |
| `src/trace/format.rs` | the `.slt` file format |
| `src/model/` | state reconstruction, symbols, DWARF, activations, stack view, ABI rules, navigation |
| `src/run.rs`, `src/experiments.rs` | the `run` and `try` commands |
| `experiments/` | the programs and guides `try` bundles |
| `src/cli/` | text and JSON output of the commands |
| `src/server/` | HTTP API, file watcher and embedded viewer assets |
| `web/` | Svelte 5 viewer |
| `tests/` | integration tests that compile and record the programs in `tests/fixtures/` |
