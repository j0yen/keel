//! `keel spend` subcommand — aggregate ledger entries and warn on threshold breach.
//!
//! Usage:
//!   keel spend [--since <dur>] [--format table|json] [--warn-at <usd>]
//!
//! Duration format: `<N>d`, `<N>h`, `<N>m`, `<N>s`.  Defaults to all time.

use crate::ledger::{LedgerRecord, LedgerStore, read_records};
use serde::Serialize;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Duration parsing
// ---------------------------------------------------------------------------

/// Parse a human duration string like `"7d"`, `"24h"`, `"30m"`, `"60s"`
/// into seconds.
///
/// Returns an error string for unrecognised formats.
///
/// # Errors
///
/// Returns an error if the string is not a recognised duration.
pub fn parse_duration_secs(s: &str) -> Result<i64, String> {
    let s = s.trim();
    let (num_str, unit) = if let Some(n) = s.strip_suffix('d') {
        (n, 86_400_i64)
    } else if let Some(n) = s.strip_suffix('h') {
        (n, 3_600_i64)
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 60_i64)
    } else if let Some(n) = s.strip_suffix('s') {
        (n, 1_i64)
    } else {
        return Err(format!(
            "unrecognised duration '{s}'; expected <N>d|h|m|s (e.g. '7d', '24h')"
        ));
    };
    let n: i64 = num_str
        .parse()
        .map_err(|_| format!("invalid number in duration '{s}'"))?;
    if n < 0 {
        return Err(format!("duration must be positive, got '{s}'"));
    }
    #[allow(clippy::arithmetic_side_effects)]
    Ok(n * unit)
}

// ---------------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------------

/// Per-tier spend summary.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TierSummary {
    /// Tier name.
    pub tier: String,
    /// Total input tokens.
    pub tokens_in: u64,
    /// Total output tokens.
    pub tokens_out: u64,
    /// Total estimated cost; `None` if any entry had `null` cost for this tier.
    pub est_cost_usd: Option<f64>,
    /// Number of ledger entries.
    pub calls: u64,
}

/// Full spend report.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpendReport {
    /// Per-tier breakdown.
    pub tiers: Vec<TierSummary>,
    /// Total input tokens across all tiers.
    pub total_tokens_in: u64,
    /// Total output tokens across all tiers.
    pub total_tokens_out: u64,
    /// Total estimated cost; `None` if any entry had `null` cost.
    pub total_est_cost_usd: Option<f64>,
    /// Total call count.
    pub total_calls: u64,
    /// Earliest timestamp included.
    pub window_start: Option<i64>,
    /// Latest timestamp included.
    pub window_end: Option<i64>,
}

/// Aggregate ledger records into a [`SpendReport`].
///
/// - `records`: the raw records to aggregate.
/// - `since_secs`: if `Some(s)`, only include records where `ts >= now - s`.
/// - `now`: the current Unix timestamp, injected for determinism.
#[must_use]
pub fn aggregate(records: &[LedgerRecord], since_secs: Option<i64>, now: i64) -> SpendReport {
    let cutoff = since_secs.map(|s| now - s);

    let filtered: Vec<&LedgerRecord> = records
        .iter()
        .filter(|r| cutoff.is_none_or(|c| r.ts >= c))
        .collect();

    // Per-tier accumulation
    let mut tier_map: HashMap<String, (u64, u64, Option<f64>, u64)> = HashMap::new();
    let mut window_start: Option<i64> = None;
    let mut window_end: Option<i64> = None;

    for r in &filtered {
        let entry = tier_map
            .entry(r.tier.clone())
            .or_insert((0u64, 0u64, Some(0.0_f64), 0u64));
        entry.0 = entry.0.saturating_add(r.tokens_in);
        entry.1 = entry.1.saturating_add(r.tokens_out);
        // If any entry for this tier has null cost, the tier total becomes null.
        entry.2 = match (entry.2, r.est_cost_usd) {
            (None, _) | (_, None) => None,
            #[allow(clippy::float_arithmetic)]
            (Some(a), Some(b)) => Some(a + b),
        };
        entry.3 = entry.3.saturating_add(1);

        window_start = Some(window_start.map_or(r.ts, |s| s.min(r.ts)));
        window_end = Some(window_end.map_or(r.ts, |e| e.max(r.ts)));
    }

    // Build sorted tier summaries
    let mut tiers: Vec<TierSummary> = tier_map
        .into_iter()
        .map(|(tier, (ti, to, cost, calls))| TierSummary {
            tier,
            tokens_in: ti,
            tokens_out: to,
            est_cost_usd: cost,
            calls,
        })
        .collect();
    tiers.sort_by(|a, b| a.tier.cmp(&b.tier));

    // Grand totals
    let mut total_tokens_in: u64 = 0;
    let mut total_tokens_out: u64 = 0;
    let mut total_cost: Option<f64> = Some(0.0);
    let mut total_calls: u64 = 0;

    for t in &tiers {
        total_tokens_in = total_tokens_in.saturating_add(t.tokens_in);
        total_tokens_out = total_tokens_out.saturating_add(t.tokens_out);
        total_cost = match (total_cost, t.est_cost_usd) {
            (None, _) | (_, None) => None,
            #[allow(clippy::float_arithmetic)]
            (Some(a), Some(b)) => Some(a + b),
        };
        total_calls = total_calls.saturating_add(t.calls);
    }

    SpendReport {
        tiers,
        total_tokens_in,
        total_tokens_out,
        total_est_cost_usd: total_cost,
        total_calls,
        window_start,
        window_end,
    }
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

/// Output format for `keel spend`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SpendFormat {
    /// Human-readable table.
    Table,
    /// Machine-readable JSON.
    Json,
}

/// Print the spend report and return the exit code.
///
/// Exits non-zero when `warn_at` is `Some(threshold)` and
/// `total_est_cost_usd >= threshold`.
///
/// # Errors
///
/// Returns an error if JSON serialization fails.
#[allow(clippy::print_stdout, clippy::print_stderr)]
pub fn print_spend(
    report: &SpendReport,
    format: SpendFormat,
    warn_at: Option<f64>,
) -> Result<i32, String> {
    match format {
        SpendFormat::Table => print_table(report),
        SpendFormat::Json => {
            let json = serde_json::to_string_pretty(report)
                .map_err(|e| format!("JSON serialization error: {e}"))?;
            println!("{json}");
        }
    }

    // Threshold check
    let breached = warn_at.is_some_and(|threshold| {
        report
            .total_est_cost_usd
            .is_some_and(|cost| cost >= threshold)
    });

    if breached {
        let cost = report.total_est_cost_usd.unwrap_or(0.0);
        let threshold = warn_at.unwrap_or(0.0);
        eprintln!(
            "keel spend: WARNING — windowed spend ${cost:.4} exceeds threshold ${threshold:.2}"
        );
        return Ok(1);
    }

    Ok(0)
}

#[allow(clippy::print_stdout)]
fn print_table(report: &SpendReport) {
    println!(
        "{:<15} {:>10} {:>11} {:>12} {:>8}",
        "TIER", "TOKENS_IN", "TOKENS_OUT", "COST_USD", "CALLS"
    );
    println!("{}", "-".repeat(60));
    for t in &report.tiers {
        let cost_str = t
            .est_cost_usd
            .map_or_else(|| "null".to_string(), |c| format!("{c:.6}"));
        println!(
            "{:<15} {:>10} {:>11} {:>12} {:>8}",
            t.tier, t.tokens_in, t.tokens_out, cost_str, t.calls
        );
    }
    println!("{}", "-".repeat(60));
    let total_cost_str = report
        .total_est_cost_usd
        .map_or_else(|| "null".to_string(), |c| format!("{c:.6}"));
    println!(
        "{:<15} {:>10} {:>11} {:>12} {:>8}",
        "TOTAL",
        report.total_tokens_in,
        report.total_tokens_out,
        total_cost_str,
        report.total_calls
    );
}

// ---------------------------------------------------------------------------
// run_spend() — load + aggregate + print
// ---------------------------------------------------------------------------

/// Load records from `store`, aggregate with `since_secs` window, print, and
/// return the exit code.
///
/// `now` is injected for determinism (no `SystemTime::now()` calls here).
///
/// # Errors
///
/// Returns an error string if the ledger cannot be read or JSON output fails.
pub fn run_spend(
    store: &dyn LedgerStore,
    since: Option<&str>,
    format: SpendFormat,
    warn_at: Option<f64>,
    now: i64,
) -> Result<i32, String> {
    let since_secs = since.map(parse_duration_secs).transpose()?;
    let records = read_records(store)?;
    let report = aggregate(&records, since_secs, now);
    print_spend(&report, format, warn_at)
}
