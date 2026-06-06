//! Core shared types for the keel fleet.
//!
//! These types form the contract that keel-ledger, keel-cordon, and
//! keel-beacon extend. They are declared here in the foundational `keel`
//! crate and re-exported from `lib.rs`.

use serde::{Deserialize, Serialize};

/// The health status of a single tier in the brain's ladder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TierStatus {
    /// Tier endpoint is reachable and (for cloud tiers) authed.
    Reachable,
    /// Tier endpoint could not be contacted.
    Unreachable {
        /// Human-readable reason (e.g. "connection refused", "timeout").
        reason: String,
    },
    /// A cloud tier that has no API key configured.
    Keyless,
    /// Tier reported quota exhaustion (HTTP 429 / 402).
    Exhausted,
    /// Tier is not present in the resolved ladder (missing from config).
    Unconfigured,
    /// Tier is explicitly excluded via `WM_BRAIN_SKIP_TIERS`.
    Skipped,
}

/// A point-in-time health snapshot for one ladder tier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TierHealth {
    /// Name of the tier (e.g. `"local-3b"`, `"haiku"`, `"opus"`).
    pub tier: String,
    /// The health status of this tier.
    pub status: TierStatus,
    /// Unix timestamp (seconds) when this check was performed.
    pub checked_at: i64,
    /// How many consecutive probe failures this tier has accumulated.
    pub consecutive_failures: u32,
}

/// A token-usage ledger entry (schema owner; written/read by keel-ledger).
///
/// Declared here so the shared type exists for all keel/* crates.
/// keel-pulse does not write or read ledger entries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerEntry {
    /// The tier that generated this usage.
    pub tier: String,
    /// Input tokens consumed.
    pub tokens_in: u64,
    /// Output tokens generated.
    pub tokens_out: u64,
    /// Estimated cost in USD (may be 0.0 for local tiers).
    pub est_cost_usd: f64,
    /// Unix timestamp (seconds) of the turn.
    pub ts: i64,
}

/// The kind of a tier — determines what the probe checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TierKind {
    /// A local model served via an OpenAI-compatible HTTP API.
    Local,
    /// A cloud provider (Anthropic).
    Cloud,
}

/// Configuration for a single rung in the brain's tier ladder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TierConfig {
    /// Tier name matching `WM_BRAIN_SKIP_TIERS` / `WM_BRAIN_MAX_TIER` values.
    pub name: String,
    /// Kind of tier.
    pub kind: TierKind,
    /// For local tiers: the base URL of the OpenAI-compatible endpoint.
    /// For cloud tiers: the Anthropic API base URL.
    pub endpoint: String,
}

/// The resolved list of configured tiers after applying skip/max config.
///
/// Every keel subcommand uses this type to agree on what the rungs are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ladder {
    /// Ordered tiers from lowest (local-3b) to highest (opus).
    pub tiers: Vec<TierConfig>,
}

impl Ladder {
    /// Returns the top (highest-priority) tier, if any.
    #[must_use]
    pub fn top(&self) -> Option<&TierConfig> {
        self.tiers.last()
    }
}
