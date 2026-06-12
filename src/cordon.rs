//! Cordon — skip re-discovering a known-dead tier every turn.
//!
//! When a cloud tier is keyless, exhausted, or unreachable, the brain
//! currently re-attempts the rung on every conversational turn.  This module
//! gives the brain a memory: a [`Cordon`] holds per-tier cooldown state so
//! that a known-dead rung is *skipped* until its cooldown expires (half-open
//! probe), rather than attempted blindly every turn.
//!
//! # Usage
//!
//! ```
//! use keel::cordon::{Cordon, CordonConfig, Decision};
//! use keel::types::TierHealth;
//!
//! let cfg = CordonConfig::default();
//! let healths: Vec<TierHealth> = vec![];
//! let cordon = Cordon::from_health(&healths, &cfg);
//! let decision = cordon.should_attempt("haiku", 0);
//! // Decision::Attempt — no failures recorded yet
//! assert!(matches!(decision, Decision::Attempt));
//! ```

use crate::types::TierStatus;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Cooldown configuration for the [`Cordon`].
///
/// `base_secs` is the base cooldown for the first failure (2^0 * base).
/// `max_secs` caps the exponential growth.
/// `keyless_skip` controls whether a `Keyless` tier is skipped via cooldown
/// or rechecked each call (default: recheck each call since it's free).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CordonConfig {
    /// Base cooldown in seconds (first failure → this many seconds).
    pub base_secs: i64,
    /// Maximum cooldown ceiling in seconds.
    pub max_secs: i64,
    /// If true, `Keyless` tiers skip via cooldown like other failures.
    /// If false (default), `Keyless` is rechecked each call (env-var check is free).
    pub keyless_via_cooldown: bool,
}

impl Default for CordonConfig {
    fn default() -> Self {
        Self {
            base_secs: 60,         // first failure: 60s
            max_secs: 3_600,       // ceiling: 1 hour
            keyless_via_cooldown: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Decision
// ---------------------------------------------------------------------------

/// The result of [`Cordon::should_attempt`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Decision {
    /// Go ahead and attempt this tier.
    Attempt,
    /// Skip this tier; cooldown not yet expired.
    Skip {
        /// Human-readable reason for the skip.
        reason: String,
        /// Unix timestamp (seconds) after which a re-probe is allowed.
        /// `None` means "recheck each call" (e.g., keyless without cooldown).
        retry_after: Option<i64>,
    },
}

// ---------------------------------------------------------------------------
// Per-tier entry
// ---------------------------------------------------------------------------

/// Per-tier cordon state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TierCordonState {
    /// Tier name.
    pub tier: String,
    /// Number of consecutive failures recorded since the last success.
    pub consecutive_failures: u32,
    /// Unix timestamp (seconds) before which this tier is cordoned.
    /// `None` means no active cooldown.
    pub cooldown_until: Option<i64>,
    /// The underlying tier status that triggered the cordon (if any).
    pub status: TierStatus,
}

impl TierCordonState {
    /// Create a fresh (no failures) entry for a tier.
    const fn fresh(tier: String, status: TierStatus) -> Self {
        Self {
            tier,
            consecutive_failures: 0,
            cooldown_until: None,
            status,
        }
    }
}

// ---------------------------------------------------------------------------
// Cordon
// ---------------------------------------------------------------------------

/// Holds per-tier cordon state and evaluates skip/attempt decisions.
///
/// Constructed from keel-pulse [`TierHealth`] snapshots.  The consumer
/// (eventually `LadderClient`) calls [`Cordon::should_attempt`] before each
/// dispatch, and [`Cordon::record_failure`] / [`Cordon::record_success`]
/// after dispatch completes.
///
/// `now` is always injected as a parameter so this type is fully deterministic
/// in tests.
///
/// [`TierHealth`]: crate::types::TierHealth
#[derive(Debug, Clone)]
pub struct Cordon {
    /// Per-tier state indexed by tier name.
    tiers: HashMap<String, TierCordonState>,
    /// Cooldown configuration.
    cfg: CordonConfig,
}

impl Cordon {
    /// Create a `Cordon` from a slice of [`crate::types::TierHealth`] snapshots and a config.
    ///
    /// For each health entry:
    /// - `Reachable` → fresh entry, no cooldown.
    /// - `Keyless` → cordon state with `Keyless` status (cooldown depends on config).
    /// - `Exhausted` / `Unreachable` → cordon state with appropriate status and
    ///   1st-failure cooldown (`base_secs` from `now=0`; caller should call
    ///   [`record_failure`][Self::record_failure] with real `now` to set a proper cooldown).
    /// - `Skipped` / `Unconfigured` → included in state but always returns `Skip`.
    #[must_use]
    pub fn from_health(healths: &[crate::types::TierHealth], cfg: &CordonConfig) -> Self {
        let mut tiers = HashMap::new();
        for h in healths {
            let entry = TierCordonState::fresh(h.tier.clone(), h.status.clone());
            tiers.insert(h.tier.clone(), entry);
        }
        Self { tiers, cfg: cfg.clone() }
    }

    /// Create an empty `Cordon` with the given config (no tier state loaded).
    #[must_use]
    pub fn empty(cfg: CordonConfig) -> Self {
        Self {
            tiers: HashMap::new(),
            cfg,
        }
    }

    /// Returns the cordon config.
    #[must_use]
    pub const fn config(&self) -> &CordonConfig {
        &self.cfg
    }

    /// Returns the current state for all tiers (for CLI display).
    #[must_use]
    pub fn states(&self) -> Vec<&TierCordonState> {
        let mut v: Vec<_> = self.tiers.values().collect();
        v.sort_by(|a, b| a.tier.cmp(&b.tier));
        v
    }

    /// Evaluate whether `tier` should be attempted at time `now` (Unix seconds).
    ///
    /// - If the tier is unknown → `Attempt` (no recorded failures).
    /// - If the tier's status is `Reachable` → `Attempt`.
    /// - If the tier's status is `Keyless` and `keyless_via_cooldown` is false →
    ///   `Skip { reason: "keyless", retry_after: None }` (recheck each call).
    /// - If the tier is in active cooldown (`now < cooldown_until`) → `Skip`.
    /// - If the tier was in cooldown but `now >= cooldown_until` → `Attempt` (half-open probe).
    #[must_use]
    pub fn should_attempt(&self, tier: &str, now: i64) -> Decision {
        let Some(state) = self.tiers.get(tier) else {
            // Unknown tier — no recorded failures
            return Decision::Attempt;
        };

        match &state.status {
            TierStatus::Reachable => Decision::Attempt,

            TierStatus::Skipped => Decision::Skip {
                reason: "tier is statically skipped".to_string(),
                retry_after: None,
            },
            TierStatus::Unconfigured => Decision::Skip {
                reason: "tier is not configured".to_string(),
                retry_after: None,
            },

            TierStatus::Keyless => {
                if self.cfg.keyless_via_cooldown {
                    Self::cooldown_decision(state, now, "keyless — no API key")
                } else {
                    // Free env-var recheck; skip the socket, recheck cheaply each call
                    Decision::Skip {
                        reason: "keyless — no API key".to_string(),
                        retry_after: None,
                    }
                }
            }

            TierStatus::Exhausted => {
                Self::cooldown_decision(state, now, "quota exhausted")
            }

            TierStatus::Unreachable { reason } => {
                let msg = format!("unreachable: {reason}");
                Self::cooldown_decision(state, now, &msg)
            }
        }
    }

    /// Record a failure for `tier` at time `now`, advancing the exponential cooldown.
    ///
    /// If the tier is unknown, a new entry is created with `Unreachable` status.
    pub fn record_failure(&mut self, tier: &str, now: i64, status: TierStatus) {
        let entry = self.tiers.entry(tier.to_string()).or_insert_with(|| {
            TierCordonState::fresh(tier.to_string(), status.clone())
        });
        entry.status = status;
        entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
        let n = entry.consecutive_failures;
        // cooldown = base * 2^(n-1), capped at max
        let exp: u32 = n.saturating_sub(1).min(62); // prevent overflow: 2^62 is safe for i64
        let raw = self.cfg.base_secs.saturating_mul(1_i64 << exp);
        let cooldown = raw.min(self.cfg.max_secs);
        entry.cooldown_until = Some(now.saturating_add(cooldown));
    }

    /// Record a success for `tier`, clearing all failure state and cooldown.
    pub fn record_success(&mut self, tier: &str) {
        if let Some(entry) = self.tiers.get_mut(tier) {
            entry.consecutive_failures = 0;
            entry.cooldown_until = None;
            entry.status = TierStatus::Reachable;
        }
        // If the tier was unknown, no-op (already implicitly fresh)
    }

    // --- helpers ---

    fn cooldown_decision(state: &TierCordonState, now: i64, reason: &str) -> Decision {
        match state.cooldown_until {
            None => {
                // Status is bad but no cooldown recorded yet — treat as first contact;
                // allow one attempt (half-open) so the caller can call record_failure.
                Decision::Attempt
            }
            Some(until) if now >= until => {
                // Cooldown expired — half-open: allow one probe attempt.
                Decision::Attempt
            }
            Some(until) => Decision::Skip {
                reason: reason.to_string(),
                retry_after: Some(until),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Display helpers for the CLI
// ---------------------------------------------------------------------------

/// Format a [`Decision`] for the cordon table output.
#[must_use]
pub fn format_decision(d: &Decision) -> String {
    match d {
        Decision::Attempt => "attempt".to_string(),
        Decision::Skip { reason, retry_after } => {
            retry_after.as_ref().map_or_else(
                || format!("skip ({reason})"),
                |t| format!("skip ({reason}) retry_after={t}"),
            )
        }
    }
}
