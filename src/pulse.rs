//! The `keel pulse` subcommand implementation.

use crate::ladder::resolve_ladder;
use crate::probe::{ProbeEnv, TierProbe};
use crate::types::{TierHealth, TierStatus};

/// Output format for `keel pulse`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    /// Human-readable table.
    Table,
    /// Machine-readable JSON (one TierHealth per configured tier).
    Json,
}

/// Run the pulse check against the ladder.
///
/// Returns the list of `TierHealth` values (one per *configured* tier,
/// including Skipped tiers which are not probed).
pub fn run_pulse(probe: &dyn TierProbe, env: &dyn ProbeEnv) -> Vec<TierHealth> {
    let skip = env.skip_tiers();
    let ladder = resolve_ladder(env);

    // Build the full default tier list so we can include Skipped entries.
    // The full list is all tiers that would exist before skip filtering.
    // We need to reconstruct skipped tiers from the skip list.
    let all_tiers = crate::ladder::all_default_tiers();

    let mut results: Vec<TierHealth> = Vec::new();

    for tier in &all_tiers {
        if skip.contains(&tier.name) {
            // Skipped — do not probe; emit Skipped health entry
            results.push(TierHealth {
                tier: tier.name.clone(),
                status: TierStatus::Skipped,
                checked_at: 0,
                consecutive_failures: 0,
            });
        } else if ladder.tiers.iter().any(|t| t.name == tier.name) {
            // Active tier — probe it
            let health = probe.probe(tier, env);
            results.push(health);
        } else {
            // Beyond max_tier cut — not in the resolved ladder
            results.push(TierHealth {
                tier: tier.name.clone(),
                status: TierStatus::Unconfigured,
                checked_at: 0,
                consecutive_failures: 0,
            });
        }
    }

    results
}

/// Format and print the pulse results.
///
/// Returns the exit code: 0 if the top *non-skipped, non-unconfigured* tier is
/// `Reachable`, 1 otherwise.
///
/// # Errors
///
/// Returns an error if serialization fails (JSON format only).
pub fn print_pulse(results: &[TierHealth], format: OutputFormat) -> Result<i32, String> {
    match format {
        OutputFormat::Table => {
            println!("{:<15} {:<15} {:<12} {}", "TIER", "STATUS", "CHECKED", "FAILS");
            println!("{}", "-".repeat(55));
            for h in results {
                let status_str = match &h.status {
                    TierStatus::Reachable => "reachable".to_string(),
                    TierStatus::Unreachable { reason } => format!("unreachable({reason})"),
                    TierStatus::Keyless => "keyless".to_string(),
                    TierStatus::Exhausted => "exhausted".to_string(),
                    TierStatus::Unconfigured => "unconfigured".to_string(),
                    TierStatus::Skipped => "skipped".to_string(),
                };
                println!(
                    "{:<15} {:<15} {:<12} {}",
                    h.tier, status_str, h.checked_at, h.consecutive_failures
                );
            }
        }
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(results)
                .map_err(|e| format!("JSON serialization error: {e}"))?;
            println!("{json}");
        }
    }

    Ok(exit_code(results))
}

/// Compute exit code: 0 if top active tier is Reachable, 1 otherwise.
fn exit_code(results: &[TierHealth]) -> i32 {
    // Top tier = last active (non-skipped, non-unconfigured) entry
    let top = results
        .iter()
        .rev()
        .find(|h| !matches!(h.status, TierStatus::Skipped | TierStatus::Unconfigured));

    match top {
        Some(h) if h.status == TierStatus::Reachable => 0,
        _ => 1,
    }
}
