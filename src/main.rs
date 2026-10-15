use std::process::ExitCode;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use systemslab::{exit, record};

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
}

fn run(cli: Cli) -> anyhow::Result<u8> {
    match cli.command {
        Command::Record { from, max_steps, strict, quiet, output, mut command } => {
            let program = command.remove(0);
            record::run(&record::Options { from, max_steps, strict, quiet, output, program, args: command })
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
