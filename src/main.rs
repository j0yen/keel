//! `keel` — brain tier-ladder health probe.
//!
//! Run `keel pulse` for a one-glance ladder health table.

use clap::{Parser, Subcommand};
use keel::probe::{HttpProbe, SystemEnv};
use keel::pulse::{OutputFormat, print_pulse, run_pulse};

/// Brain tier-ladder health probe and shared types for the keel fleet.
#[derive(Debug, Parser)]
#[command(name = "keel", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Available subcommands.
#[derive(Debug, Subcommand)]
enum Commands {
    /// Print a health table for the configured tier ladder.
    Pulse {
        /// Output format.
        #[arg(long, default_value = "table")]
        format: OutputFormat,
    },
}

#[allow(clippy::print_stderr)]
fn main() -> std::process::ExitCode {
    // SIGPIPE safety: self_sigpipe_panic_toolkit — `keel pulse | head` must not panic.
    sigpipe::reset();

    let cli = Cli::parse();

    match cli.command {
        Commands::Pulse { format } => {
            let probe = HttpProbe;
            let env = SystemEnv;
            let results = run_pulse(&probe, &env);
            match print_pulse(&results, format) {
                Ok(code) => {
                    if code == 0 {
                        std::process::ExitCode::SUCCESS
                    } else {
                        std::process::ExitCode::FAILURE
                    }
                }
                Err(e) => {
                    eprintln!("keel: error: {e}");
                    std::process::ExitCode::FAILURE
                }
            }
        }
    }
}
