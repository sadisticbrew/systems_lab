use std::process::ExitCode;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use systemslab::{cli, exit, experiments, record, server};

#[derive(Parser)]
#[command(name = "systemslab", version, about = "Record real x86-64 executions and inspect them")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Record a program's execution one instruction at a time
    Record {
        /// Where to start recording: auto, main, entry, or a symbol of the executable
        #[arg(long, default_value = "auto")]
        from: String,
        /// Stop after this many steps
        #[arg(long, default_value_t = 2_000_000)]
        max_steps: u64,
        /// Exit 4 if any memory change is not accounted for by an instruction or syscall
        #[arg(long)]
        strict: bool,
        /// Do not print the summary line
        #[arg(long)]
        quiet: bool,
        /// Trace file to write
        #[arg(short = 'o', long = "output", default_value = "trace.slt")]
        output: PathBuf,
        /// The program and its arguments, after `--`
        #[arg(last = true, required = true, num_args = 1..)]
        command: Vec<String>,
    },
    /// Summarize a trace
    Info {
        trace: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// List steps with their register changes, memory writes and events
    Steps {
        trace: PathBuf,
        /// First step: `step:N`, `SYMBOL[+OFFSET][@K]` or `0xADDR[@K]` (default: step 0)
        #[arg(long)]
        from: Option<String>,
        /// Step to stop before (exclusive; default: the end)
        #[arg(long)]
        to: Option<String>,
        /// Only steps whose instruction is in this function
        #[arg(long)]
        func: Option<String>,
        #[arg(long, default_value_t = 200)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Show the stack frames and slots at a step
    Stack {
        trace: PathBuf,
        loc: String,
        /// Show only the innermost N activations; the rest collapse into one row
        #[arg(long)]
        frames: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// Generate Graphviz figures
    Dot {
        #[command(subcommand)]
        figure: DotFigure,
    },
    /// Check System V ABI rules over the recorded calls
    CheckAbi {
        trace: PathBuf,
        /// Check every call and activation, not only those of the main executable
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },
    /// Check that every directive of a guide resolves in a trace and show the values it asks about
    CheckGuide {
        trace: PathBuf,
        /// A Markdown guide with `@go` and `@predict` directives
        guide: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Compare two recordings: steps, output, syscalls, calls per function and the first point where they diverge
    Compare {
        /// The recording shown as A
        a: PathBuf,
        /// The recording shown as B
        b: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Open a trace in the browser viewer
    View {
        /// A .slt trace, or a lab directory whose out/ holds one
        path: PathBuf,
        /// Port to listen on (default: any free port)
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// Do not launch a browser
        #[arg(long)]
        no_open: bool,
        /// Open at this moment: `step:N`, `step:end`, `FILE:LINE[@K]`, `SYMBOL[+OFFSET][@K]` or `0xADDR[@K]`
        #[arg(long, value_name = "LOCATOR")]
        at: Option<String>,
        /// Markdown guide to show (default: PATH/guide.md for a lab directory, else TRACE.guide.md next to the trace)
        #[arg(long)]
        guide: Option<PathBuf>,
    },
    /// Compile a C or assembly source, record it and open it in the viewer; saving the source rebuilds it
    Run {
        /// A .c, .s or .S source file
        source: PathBuf,
        /// Optimization level passed to the compiler
        #[arg(short = 'O', value_name = "LEVEL", default_value = "0", value_parser = ["0", "1", "2", "3", "s", "g"])]
        opt: String,
        /// Where to start recording: auto, main, entry, or a symbol of the executable
        #[arg(long, default_value = "auto")]
        from: String,
        /// Stop after this many steps
        #[arg(long, default_value_t = 2_000_000)]
        max_steps: u64,
        /// Trace file to write (default: SOURCE with the extension .slt)
        #[arg(short = 'o', long = "output")]
        output: Option<PathBuf>,
        /// Markdown guide to show (default: SOURCE with the extension .guide.md, if it exists)
        #[arg(long)]
        guide: Option<PathBuf>,
        /// Port to listen on (default: any free port)
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// Do not launch a browser
        #[arg(long)]
        no_open: bool,
        /// Record and exit without starting the viewer
        #[arg(long)]
        no_view: bool,
        /// Do not rebuild when the source changes
        #[arg(long)]
        no_watch: bool,
        /// Arguments of the program, after `--`
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// List the bundled experiments, or run one with its guide
    Try {
        /// Experiment name; omit to list them
        name: Option<String>,
        /// Port to listen on (default: any free port)
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// Do not launch a browser
        #[arg(long)]
        no_open: bool,
        /// Record and exit without starting the viewer
        #[arg(long)]
        no_view: bool,
    },
}

#[derive(Subcommand)]
enum DotFigure {
    /// The stack at a step as a Graphviz figure
    Stack {
        trace: PathBuf,
        loc: String,
        #[arg(long)]
        frames: Option<usize>,
    },
}

fn run(cli: Cli) -> anyhow::Result<u8> {
    match cli.command {
        Command::Record { from, max_steps, strict, quiet, output, mut command } => {
            let program = command.remove(0);
            record::run(&record::Options { from, max_steps, strict, quiet, output, program, args: command })
        }
        Command::Info { trace, json } => cli::info(&trace, json),
        Command::Steps { trace, from, to, func, limit, json } => {
            cli::steps(&cli::StepsOptions { trace, from, to, func, limit, json })
        }
        Command::Stack { trace, loc, frames, json } => cli::stack(&trace, &loc, frames, json),
        Command::Dot { figure: DotFigure::Stack { trace, loc, frames } } => cli::dot_stack(&trace, &loc, frames),
        Command::CheckAbi { trace, all, json } => cli::check_abi(&trace, all, json),
        Command::CheckGuide { trace, guide, json } => cli::check_guide(&trace, &guide, json),
        Command::Compare { a, b, json } => cli::compare(&a, &b, json),
        Command::View { path, port, no_open, at, guide } => {
            let trace_path = server::resolve_trace(&path)?;
            let guide = guide.or_else(|| server::discover_guide(&path, &trace_path));
            server::serve(server::ServeOptions { trace_path, guide, at, port, no_open, mode: server::Mode::View })
        }
        Command::Run { source, opt, from, max_steps, output, guide, port, no_open, no_view, no_watch, args } => {
            systemslab::run::run(systemslab::run::RunOptions {
                source,
                opt,
                from,
                max_steps,
                output,
                guide,
                port,
                no_open,
                no_view,
                no_watch,
                args,
            })
        }
        Command::Try { name, port, no_open, no_view } => {
            experiments::try_experiment(experiments::TryOptions { name, port, no_open, no_view })
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            match err.downcast_ref::<exit::Exit>() {
                Some(e) => eprintln!("{e}"),
                None => eprintln!("error: {err:#}"),
            }
            ExitCode::from(exit::code_of(&err))
        }
    }
}
