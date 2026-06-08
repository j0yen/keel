//! `keel` — brain tier-ladder health probe.
//!
//! Subcommands:
//! - `keel pulse`  — health table for the full ladder.
//! - `keel status` — one-liner: what tier is the brain on, for how long?
//! - `keel beacon` — emit `wm.keel.*` events when the ceiling changes.

use clap::{Parser, Subcommand};
use keel::beacon::{AgorabusBeacon, run_beacon};
use keel::probe::{HttpProbe, SystemEnv};
use keel::pulse::{OutputFormat, print_pulse, run_pulse};
use keel::status::{StatusFormat, compute_status, unix_now};

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
    /// Print a one-line summary of the current effective tier ceiling.
    Status {
        /// Output format.
        #[arg(long, default_value = "text")]
        format: StatusFormat,
        /// Path to the last-ceiling state file.
        #[arg(long, default_value = "")]
        state_file: String,
    },
    /// Detect ceiling changes and emit wm.keel.* events on agorabus.
    Beacon {
        /// Path to the last-ceiling state file.
        #[arg(long, default_value = "")]
        state_file: String,
    },
}

#[allow(clippy::print_stderr)]
fn main() -> std::process::ExitCode {
    // SIGPIPE safety: self_sigpipe_panic_toolkit — `keel status | head` must not panic.
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

        Commands::Status { format, state_file } => {
            let probe = HttpProbe;
            let env = SystemEnv;
            let healths = run_pulse(&probe, &env);
            let now = unix_now();

            // Load the since timestamp from the state file if available.
            let since = resolve_state_path(&state_file)
                .and_then(|p| {
                    keel::beacon::CeilingState::load(&p)
                        .ok()
                        .flatten()
                        .map(|s| s.since)
                });

            let report = compute_status(&healths, since, now);

            match format {
                StatusFormat::Text => {
                    #[allow(clippy::print_stdout)]
                    {
                        println!("{}", report.as_text());
                    }
                }
                StatusFormat::Json => {
                    match serde_json::to_string_pretty(&report) {
                        Ok(json) => {
                            #[allow(clippy::print_stdout)]
                            {
                                println!("{json}");
                            }
                        }
                        Err(e) => {
                            eprintln!("keel: json error: {e}");
                            return std::process::ExitCode::FAILURE;
                        }
                    }
                }
            }

            if report.nominal {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::FAILURE
            }
        }

        Commands::Beacon { state_file } => {
            let probe = HttpProbe;
            let env = SystemEnv;
            let healths = run_pulse(&probe, &env);
            let now = unix_now();

            let state_path = match resolve_state_path(&state_file) {
                Some(p) => p,
                None => {
                    eprintln!("keel: beacon: --state-file is required or WM_KEEL_STATE_FILE must be set");
                    return std::process::ExitCode::FAILURE;
                }
            };

            let beacon = AgorabusBeacon::new();
            match run_beacon(now, &state_path, &healths, &beacon) {
                Ok(_outcome) => std::process::ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("keel: beacon error: {e}");
                    std::process::ExitCode::FAILURE
                }
            }
        }
    }
}

/// Resolve the state file path: prefer explicit arg, then env var, then None.
fn resolve_state_path(arg: &str) -> Option<std::path::PathBuf> {
    if !arg.is_empty() {
        return Some(std::path::PathBuf::from(arg));
    }
    let env_val = std::env::var("WM_KEEL_STATE_FILE").unwrap_or_default();
    if !env_val.is_empty() {
        return Some(std::path::PathBuf::from(env_val));
    }
    // Default: ~/.local/share/keel/last-ceiling.json
    std::env::var("HOME").ok().map(|home| {
        std::path::PathBuf::from(home)
            .join(".local/share/keel/last-ceiling.json")
    })
}
