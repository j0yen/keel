//! Resolves the configured tier ladder from the environment.
//!
//! Reads `WM_BRAIN_SKIP_TIERS` and `WM_BRAIN_MAX_TIER` via a `ProbeEnv`
//! and returns the subset of tiers actually active in this runtime.

use crate::probe::ProbeEnv;
use crate::types::{Ladder, TierConfig, TierKind};

/// Returns all default tiers in ladder order (lowest → highest priority).
///
/// This is the full list before skip/max filtering is applied.
/// Used by `pulse.rs` to reconstruct the Skipped/Unconfigured entries
/// that `resolve_ladder` would have removed.
#[must_use]
pub fn all_default_tiers() -> Vec<TierConfig> {
    default_tiers()
}

/// Default tier order (lowest → highest priority).
///
/// This mirrors the wintermute-brain ladder documented in `project_brain_local_first_ladder`.
fn default_tiers() -> Vec<TierConfig> {
    vec![
        TierConfig {
            name: "local-3b".to_string(),
            kind: TierKind::Local,
            endpoint: "http://localhost:11434".to_string(),
        },
        TierConfig {
            name: "local-8b".to_string(),
            kind: TierKind::Local,
            endpoint: "http://localhost:11434".to_string(),
        },
        TierConfig {
            name: "haiku".to_string(),
            kind: TierKind::Cloud,
            endpoint: "https://api.anthropic.com".to_string(),
        },
        TierConfig {
            name: "sonnet".to_string(),
            kind: TierKind::Cloud,
            endpoint: "https://api.anthropic.com".to_string(),
        },
        TierConfig {
            name: "opus".to_string(),
            kind: TierKind::Cloud,
            endpoint: "https://api.anthropic.com".to_string(),
        },
    ]
}

/// Resolves the active tier ladder given the probe environment.
///
/// Applies `WM_BRAIN_SKIP_TIERS` (removes named tiers) and
/// `WM_BRAIN_MAX_TIER` (removes everything above the named tier).
#[must_use]
pub fn resolve_ladder(env: &dyn ProbeEnv) -> Ladder {
    let skip = env.skip_tiers();
    let max = env.max_tier();

    let mut tiers: Vec<TierConfig> = default_tiers()
        .into_iter()
        .filter(|t| !skip.contains(&t.name))
        .collect();

    if let Some(ref max_name) = max {
        if let Some(pos) = tiers.iter().position(|t| &t.name == max_name) {
            tiers.truncate(pos + 1);
        }
    }

    Ladder { tiers }
}
