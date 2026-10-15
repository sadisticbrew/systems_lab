use std::process::ExitCode;

use clap::{Parser, Subcommand};
use systemslab::exit;

#[derive(Parser)]
#[command(name = "systemslab", version, about = "Record real x86-64 executions and inspect them")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {}

fn run(cli: Cli) -> anyhow::Result<u8> {
    match cli.command {}
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
