use std::process::ExitCode;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use systemslab::{cli, exit, record};

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
