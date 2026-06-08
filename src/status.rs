//! `keel status` — one-line human-readable summary of the current tier ceiling.
//!
//! Answers: "what tier is the brain standing on, and for how long?"
//! - Degraded: `floored: local-3b for 4d 6h (cloud keyless since 2026-05-30)`
//! - Nominal:  `nominal: opus reachable`

use crate::types::{TierHealth, TierStatus};
use serde::Serialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Output format for `keel status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum StatusFormat {
    /// One-line human-readable text.
    Text,
    /// Machine-readable JSON.
    Json,
}

/// The computed status for `keel status` output.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatusReport {
    /// Whether the ladder is at full capacity.
    pub nominal: bool,
    /// The effective ceiling tier name (highest reachable).
    pub ceiling: String,
    /// Why the ceiling is where it is (e.g. `"cloud keyless"`).
    pub reason: Option<String>,
    /// Unix timestamp (seconds) since when the current ceiling has been in effect.
    /// `None` if the first run or ceiling state is not persisted.
    pub since: Option<i64>,
    /// Human-readable "for N days M hours" duration string.
    pub duration_human: Option<String>,
}

impl StatusReport {
    /// Format as a single human-readable line.
    #[must_use]
    pub fn as_text(&self) -> String {
        if self.nominal {
            return format!("nominal: {} reachable", self.ceiling);
        }

        let mut parts = vec![format!("floored: {}", self.ceiling)];

        if let Some(ref dur) = self.duration_human {
            parts.push(format!("for {dur}"));
        }

        if let Some(ref why) = self.reason {
            parts.push(format!("({why})"));
        }

        parts.join(" ")
    }
}

/// Compute a [`StatusReport`] from tier health values and an optional `since`
/// timestamp from the persisted ceiling state.
///
/// `now` is injected for determinism.
#[must_use]
pub fn compute_status(healths: &[TierHealth], since: Option<i64>, now: i64) -> StatusReport {
    // Find the highest reachable tier.
    let top_reachable = healths
        .iter()
        .rev()
        .find(|h| h.status == TierStatus::Reachable);

    // Find the absolute highest active tier (non-unconfigured, non-skipped).
    let top_active = healths
        .iter()
        .rev()
        .find(|h| !matches!(h.status, TierStatus::Unconfigured | TierStatus::Skipped));

    let nominal = match (top_reachable, top_active) {
        (Some(r), Some(a)) => r.tier == a.tier,
        (Some(_), None) => true,
        _ => false,
    };

    let ceiling = top_reachable
        .map(|h| h.tier.clone())
        .or_else(|| healths.iter().find(|h| !matches!(h.status, TierStatus::Unconfigured)).map(|h| h.tier.clone()))
        .unwrap_or_else(|| "unknown".to_string());

    // Compute degradation reason.
    let reason = if nominal {
        None
    } else {
        // Look at the highest active tiers to find the cause.
        let cloud_status = healths.iter().rev().find(|h| {
            matches!(h.tier.as_str(), "haiku" | "sonnet" | "opus")
                && !matches!(h.status, TierStatus::Unconfigured | TierStatus::Skipped)
        });
        cloud_status.map(|h| match &h.status {
            TierStatus::Keyless => "cloud keyless".to_string(),
            TierStatus::Unreachable { reason } => format!("cloud unreachable: {reason}"),
            TierStatus::Exhausted => "cloud exhausted".to_string(),
            _ => "cloud degraded".to_string(),
        })
    };

    let duration_human = since.map(|s| human_duration(now - s));

    StatusReport {
        nominal,
        ceiling,
        reason,
        since,
        duration_human,
    }
}

/// Format a duration in seconds as a human-readable string.
///
/// Examples: `"4d 6h"`, `"2h 15m"`, `"45s"`.
#[must_use]
pub fn human_duration(secs: i64) -> String {
    let secs = secs.max(0) as u64;
    let dur = Duration::from_secs(secs);
    let total_secs = dur.as_secs();

    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let mins = (total_secs % 3600) / 60;
    let s = total_secs % 60;

    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {mins}m")
    } else if mins > 0 {
        format!("{mins}m {s}s")
    } else {
        format!("{s}s")
    }
}

/// Get the current unix timestamp in seconds (fallback to 0).
#[must_use]
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
