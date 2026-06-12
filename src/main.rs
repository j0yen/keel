//! `keel` — brain tier-ladder health probe.
//!
//! Subcommands:
//! - `keel pulse`   — health table for the full ladder.
//! - `keel status`  — one-liner: what tier is the brain on, for how long?
//! - `keel beacon`  — emit `wm.keel.*` events when the ceiling changes.
//! - `keel record`  — append a token-usage entry to the spend ledger.
//! - `keel spend`   — aggregate the spend ledger, optional warn threshold.
//! - `keel mark`    — stamp a tier's health status as exhausted or keyless.
//! - `keel cordon`  — show which tiers the brain would skip this turn.

use clap::{Parser, Subcommand};
use keel::beacon::{AgorabusBeacon, run_beacon};
use keel::cordon::{Cordon, CordonConfig, Decision, format_decision};
use keel::ledger::DefaultStore;
use keel::mark::MarkStamp;
use keel::probe::{HttpProbe, SystemEnv};
use keel::pulse::{OutputFormat, print_pulse, run_pulse};
use keel::spend::{SpendFormat, run_spend};
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
    /// Append a token-usage entry to the spend ledger.
    Record {
        /// Tier name (e.g. haiku, sonnet, opus, local-3b).
        #[arg(value_name = "TIER")]
        tier: String,
        /// Input tokens consumed.
        #[arg(value_name = "TOKENS_IN")]
        tokens_in: u64,
        /// Output tokens generated.
        #[arg(value_name = "TOKENS_OUT")]
        tokens_out: u64,
    },
    /// Aggregate the spend ledger and optionally warn on threshold breach.
    Spend {
        /// Include only entries from the past <dur> (e.g. 7d, 24h, 30m).
        #[arg(long)]
        since: Option<String>,
        /// Output format.
        #[arg(long, default_value = "table")]
        format: SpendFormat,
        /// Exit non-zero and print a warning when windowed spend >= this USD threshold.
        #[arg(long)]
        warn_at: Option<f64>,
    },
    /// Stamp a tier's health status as exhausted or keyless.
    Mark {
        /// Tier name (e.g. sonnet, haiku, opus).
        #[arg(value_name = "TIER")]
        tier: String,
        /// The health stamp to apply.
        #[arg(value_name = "STAMP")]
        stamp: MarkStamp,
    },
    /// Show which tiers the brain would skip vs. attempt this turn.
    ///
    /// Consumes live pulse health to build a Cordon and prints each tier's
    /// decision.  Use this to answer "why is the brain not trying cloud?".
    Cordon {
        /// Output format.
        #[arg(long, default_value = "table")]
        format: CordonOutputFormat,
    },
}

/// Output format for `keel cordon`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum CordonOutputFormat {
    /// Human-readable table.
    Table,
    /// Machine-readable JSON.
    Json,
}

#[allow(clippy::print_stderr)]
#[allow(clippy::print_stdout)]
fn main() -> std::process::ExitCode {
    // SIGPIPE safety: self_sigpipe_panic_toolkit — `keel spend | head` must not panic.
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
                    println!("{}", report.as_text());
                }
                StatusFormat::Json => {
                    match serde_json::to_string_pretty(&report) {
                        Ok(json) => {
                            println!("{json}");
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

        Commands::Record {
            tier,
            tokens_in,
            tokens_out,
        } => {
            let store = DefaultStore;
            let now = unix_now();
            match keel::ledger::record(&store, &tier, tokens_in, tokens_out, now) {
                Ok(()) => std::process::ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("keel record: error: {e}");
                    std::process::ExitCode::FAILURE
                }
            }
        }

        Commands::Spend {
            since,
            format,
            warn_at,
        } => {
            let store = DefaultStore;
            let now = unix_now();
            match run_spend(&store, since.as_deref(), format, warn_at, now) {
                Ok(code) => {
                    if code == 0 {
                        std::process::ExitCode::SUCCESS
                    } else {
                        std::process::ExitCode::FAILURE
                    }
                }
                Err(e) => {
                    eprintln!("keel spend: error: {e}");
                    std::process::ExitCode::FAILURE
                }
            }
        }

        Commands::Mark { tier, stamp } => {
            let store = DefaultStore;
            let now = unix_now();
            match keel::mark::mark_tier(&store, &tier, stamp, now) {
                Ok(()) => {
                    println!("keel mark: {tier} → {stamp}");
                    std::process::ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("keel mark: error: {e}");
                    std::process::ExitCode::FAILURE
                }
            }
        }

        Commands::Cordon { format } => {
            let probe = HttpProbe;
            let env = SystemEnv;
            let healths = run_pulse(&probe, &env);
            let cfg = CordonConfig::default();
            let cordon = Cordon::from_health(&healths, &cfg);

            // Use current time for the decision.
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
                .unwrap_or(0);

            match format {
                CordonOutputFormat::Table => {
                    println!("{:<15} {:<10} DETAIL", "TIER", "DECISION");
                    println!("{}", "-".repeat(60));
                    for state in cordon.states() {
                        let decision = cordon.should_attempt(&state.tier, now);
                        println!(
                            "{:<15} {:<10} {}",
                            state.tier,
                            match &decision {
                                Decision::Attempt => "attempt",
                                Decision::Skip { .. } => "skip",
                            },
                            format_decision(&decision),
                        );
                    }
                    std::process::ExitCode::SUCCESS
                }
                CordonOutputFormat::Json => {
                    let entries: Vec<serde_json::Value> = cordon
                        .states()
                        .iter()
                        .map(|state| {
                            let decision = cordon.should_attempt(&state.tier, now);
                            serde_json::json!({
                                "tier": state.tier,
                                "decision": decision,
                                "consecutive_failures": state.consecutive_failures,
                                "cooldown_until": state.cooldown_until,
                            })
                        })
                        .collect();
                    match serde_json::to_string_pretty(&entries) {
                        Ok(json) => {
                            println!("{json}");
                            std::process::ExitCode::SUCCESS
                        }
                        Err(e) => {
                            eprintln!("keel: error: {e}");
                            std::process::ExitCode::FAILURE
                        }
                    }
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
