//! `keel mark` subcommand — stamp a tier's health status as Exhausted or Keyless.
//!
//! The brain calls `mark_tier()` (or the subcommand CLI) when it sees a 402/401
//! response, so the event becomes durable state that keel-cordon and keel-beacon
//! can read on the next tick.
//!
//! The override is written as a single `TierHealth` JSON object in
//! `~/.local/state/keel/status.json`.  It replaces any previous entry for the
//! same tier name; other tiers are preserved.

use crate::ledger::LedgerStore;
use crate::types::{TierHealth, TierStatus};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// On-disk format
// ---------------------------------------------------------------------------

/// The full on-disk status file: a map from tier name to `TierHealth`.
///
/// Written as pretty JSON so it is human-readable; read by other keel
/// subcommands.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatusFile {
    /// Per-tier health overrides.
    pub tiers: HashMap<String, TierHealth>,
}

// ---------------------------------------------------------------------------
// mark_tier() — library entry point
// ---------------------------------------------------------------------------

/// Stamp a tier's status as `Exhausted` or `Keyless` with the given timestamp.
///
/// Reads the current `status.json`, upserts the entry, and atomically writes
/// it back (write-to-tmp + rename, but since we need wide compatibility we
/// just overwrite — the file is small and the update is idempotent).
///
/// # Errors
///
/// Returns a human-readable error string on I/O or serialization failure.
pub fn mark_tier(
    store: &dyn LedgerStore,
    tier: &str,
    stamp: MarkStamp,
    now: i64,
) -> Result<(), String> {
    let mut sf = load_status(store)?;

    let status = match stamp {
        MarkStamp::Exhausted => TierStatus::Exhausted,
        MarkStamp::Keyless => TierStatus::Keyless,
    };

    sf.tiers.insert(
        tier.to_owned(),
        TierHealth {
            tier: tier.to_owned(),
            status,
            checked_at: now,
            consecutive_failures: 0,
        },
    );

    save_status(store, &sf)
}

/// Read the current status override for a tier, if any.
///
/// Returns `None` if the tier has no entry in `status.json` or if the file
/// does not exist.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be read or parsed.
pub fn tier_status(store: &dyn LedgerStore, tier: &str) -> Result<Option<TierHealth>, String> {
    let sf = load_status(store)?;
    Ok(sf.tiers.get(tier).cloned())
}

// ---------------------------------------------------------------------------
// What kind of stamp to apply
// ---------------------------------------------------------------------------

/// The kind of health stamp to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum MarkStamp {
    /// Tier reported quota exhaustion (HTTP 402/429).
    Exhausted,
    /// Tier has no API key configured.
    Keyless,
}

impl std::fmt::Display for MarkStamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exhausted => write!(f, "exhausted"),
            Self::Keyless => write!(f, "keyless"),
        }
    }
}

// ---------------------------------------------------------------------------
// I/O helpers
// ---------------------------------------------------------------------------

fn load_status(store: &dyn LedgerStore) -> Result<StatusFile, String> {
    let path = store.status_path();
    if !path.exists() {
        return Ok(StatusFile::default());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("read status file {}: {e}", path.display()))?;
    serde_json::from_str(&raw)
        .map_err(|e| format!("parse status file {}: {e}", path.display()))
}

fn save_status(store: &dyn LedgerStore, sf: &StatusFile) -> Result<(), String> {
    let path = store.status_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create status dir {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(sf)
        .map_err(|e| format!("serialize status file: {e}"))?;
    std::fs::write(&path, json)
        .map_err(|e| format!("write status file {}: {e}", path.display()))
}
