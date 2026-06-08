//! `keel beacon` — ceiling-change detection and agorabus event emission.
//!
//! This module provides:
//! - [`BeaconEvent`]: the events emitted when the effective ceiling changes.
//! - [`Beacon`] trait: a `publish` method with a `RecordingBeacon` test double
//!   and an [`AgorabusBeacon`] real implementation.
//! - [`CeilingState`]: the persisted last-emitted ceiling (state file).
//! - [`effective_ceiling`]: computes the highest `Attempt`-able tier from
//!   a slice of [`TierHealth`] values.
//! - [`run_beacon`]: diff current vs stored ceiling and emit the right event.

use crate::types::{TierHealth, TierStatus};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

// ── Event types ─────────────────────────────────────────────────────────────

/// A ceiling-change event ready to publish on `wm.keel.*`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeaconEvent {
    /// `"wm.keel.degraded"` or `"wm.keel.refloat"`.
    pub topic: String,
    /// The previous effective ceiling tier name.
    pub from: String,
    /// The new effective ceiling tier name.
    pub to: String,
    /// Unix timestamp (seconds) of when the degradation started (for
    /// `degraded`) or when the refloat was detected (for `refloat`).
    pub since: i64,
}

// ── Beacon trait ─────────────────────────────────────────────────────────────

/// Publishes a [`BeaconEvent`] on the bus.
///
/// Tests inject a [`RecordingBeacon`] that never touches the real bus.
/// Production code uses [`AgorabusBeacon`].
pub trait Beacon {
    /// Publish `event`. May silently discard if the bus is unavailable.
    ///
    /// # Errors
    ///
    /// Returns `Err` only on hard internal failures (serialization, etc.).
    /// A missing or unreachable bus should be treated as a soft failure and
    /// logged but not propagated (fail-open).
    fn publish(&self, event: &BeaconEvent) -> Result<(), String>;
}

// ── RecordingBeacon (test double) ────────────────────────────────────────────

/// A [`Beacon`] that records published events instead of touching the bus.
///
/// Used in every test to assert zero live-bus connections.
#[derive(Debug, Default)]
pub struct RecordingBeacon {
    events: Mutex<Vec<BeaconEvent>>,
}

impl RecordingBeacon {
    /// Create a new empty recording beacon.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Return all recorded events.
    ///
    /// # Panics
    ///
    /// If the internal mutex is poisoned (only if a test thread panicked while
    /// holding the lock — benign in single-threaded test contexts).
    #[must_use]
    pub fn recorded(&self) -> Vec<BeaconEvent> {
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl Beacon for RecordingBeacon {
    fn publish(&self, event: &BeaconEvent) -> Result<(), String> {
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(event.clone());
        Ok(())
    }
}

// ── AgorabusBeacon (real) ────────────────────────────────────────────────────

/// A [`Beacon`] that publishes to the live agorabus over its UDS socket.
///
/// Fail-open: if the socket is absent or refuses, silently succeeds.
/// The bus is not critical to `keel beacon`'s state-file updates.
#[derive(Debug)]
pub struct AgorabusBeacon {
    socket_path: std::path::PathBuf,
}

impl AgorabusBeacon {
    /// Create a beacon targeting the default agorabus socket.
    #[must_use]
    pub fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let path = std::path::PathBuf::from(&home).join(".cache/agorabus/sock");
        Self { socket_path: path }
    }

    /// Create a beacon targeting a custom socket path (for testing).
    #[must_use]
    pub fn with_socket(socket_path: std::path::PathBuf) -> Self {
        Self { socket_path }
    }
}

impl Default for AgorabusBeacon {
    fn default() -> Self {
        Self::new()
    }
}

impl Beacon for AgorabusBeacon {
    #[allow(clippy::print_stderr)]
    fn publish(&self, event: &BeaconEvent) -> Result<(), String> {
        use std::io::Write as _;
        use std::os::unix::net::UnixStream;

        // Fail-open: if the socket doesn't exist, do nothing.
        let mut stream = match UnixStream::connect(&self.socket_path) {
            Ok(s) => s,
            Err(_) => return Ok(()),
        };

        let pid = std::process::id();
        let announce = serde_json::json!({
            "op": "announce",
            "session_id": format!("keel-beacon-{pid}"),
            "pid": pid,
            "cwd": std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("/"))
                .display()
                .to_string(),
            "intent": "keel beacon publish"
        });

        let payload = serde_json::json!({
            "from": event.from,
            "to": event.to,
            "since": event.since,
        });

        let publish = serde_json::json!({
            "op": "publish",
            "topic": event.topic,
            "data": payload,
        });

        let mut announce_line =
            serde_json::to_vec(&announce).map_err(|e| e.to_string())?;
        announce_line.push(b'\n');
        let mut publish_line =
            serde_json::to_vec(&publish).map_err(|e| e.to_string())?;
        publish_line.push(b'\n');

        // Best-effort writes; ignore errors (fail-open).
        let _ = stream.write_all(&announce_line);
        let _ = stream.write_all(&publish_line);
        let _ = stream.flush();

        Ok(())
    }
}

// ── CeilingState — persisted last-ceiling ───────────────────────────────────

/// The persisted last-emitted ceiling.
///
/// Written to a small JSON file so the next `keel beacon` run can diff
/// against it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CeilingState {
    /// The effective ceiling tier name at last emit.
    pub ceiling: String,
    /// Unix timestamp (seconds) when this ceiling first became effective.
    pub since: i64,
}

impl CeilingState {
    /// Load the state from `path`. Returns `None` if the file is absent.
    ///
    /// # Errors
    ///
    /// Returns `Err` on I/O or parse failures (other than file-not-found).
    pub fn load(path: &Path) -> Result<Option<Self>, String> {
        match std::fs::read_to_string(path) {
            Ok(s) => serde_json::from_str(&s).map(Some).map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Persist to `path` (creates parent dirs if needed).
    ///
    /// # Errors
    ///
    /// Returns `Err` on I/O or serialization failures.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }
}

// ── Ceiling computation ──────────────────────────────────────────────────────

/// Compute the effective ceiling from a list of tier health values.
///
/// The effective ceiling is the name of the highest tier whose status is
/// [`TierStatus::Reachable`]. If none are reachable, returns the name of
/// the lowest configured tier (the fallback floor).
///
/// Tiers are assumed to be ordered lowest → highest (as returned by
/// `run_pulse`).
#[must_use]
pub fn effective_ceiling(healths: &[TierHealth]) -> Option<String> {
    // Walk in reverse (highest → lowest) to find the first Reachable tier.
    let top_reachable = healths
        .iter()
        .rev()
        .find(|h| h.status == TierStatus::Reachable);

    if let Some(h) = top_reachable {
        return Some(h.tier.clone());
    }

    // No reachable tier — return the lowest configured (non-unconfigured) tier
    // as the floor label.
    healths
        .iter()
        .find(|h| !matches!(h.status, TierStatus::Unconfigured))
        .map(|h| h.tier.clone())
}

// ── run_beacon ───────────────────────────────────────────────────────────────

/// Output of a single `keel beacon` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BeaconOutcome {
    /// No change; nothing emitted.
    NoChange,
    /// Ceiling dropped; `wm.keel.degraded` emitted.
    Degraded(BeaconEvent),
    /// Ceiling rose; `wm.keel.refloat` emitted.
    Refloat(BeaconEvent),
    /// First run (no prior state); state file written but no event emitted.
    FirstRun,
}

/// Run the beacon: diff current vs stored ceiling, emit the right event.
///
/// - `now`: injectable unix timestamp (seconds) for determinism.
/// - `state_path`: path to the `last-ceiling` state file.
/// - `healths`: current tier health values (from `run_pulse`).
/// - `beacon`: event publisher.
///
/// # Errors
///
/// Returns `Err` on state file I/O failures or publish errors.
pub fn run_beacon(
    now: i64,
    state_path: &Path,
    healths: &[TierHealth],
    beacon: &dyn Beacon,
) -> Result<BeaconOutcome, String> {
    let current = match effective_ceiling(healths) {
        Some(c) => c,
        None => return Ok(BeaconOutcome::NoChange),
    };

    let prior = CeilingState::load(state_path)?;

    let outcome = match prior {
        None => {
            // First run: write the state file and emit nothing.
            let state = CeilingState {
                ceiling: current.clone(),
                since: now,
            };
            state.save(state_path)?;
            BeaconOutcome::FirstRun
        }
        Some(ref prior_state) if prior_state.ceiling == current => {
            // No change.
            BeaconOutcome::NoChange
        }
        Some(prior_state) => {
            // Ceiling changed — determine direction.
            let from = prior_state.ceiling.clone();
            let to = current.clone();

            // Build the ordered tier names to compare positions.
            let tier_order = tier_rank(&from, &to, healths);
            let topic = if tier_order == TierChangeDirection::Degraded {
                "wm.keel.degraded".to_string()
            } else {
                "wm.keel.refloat".to_string()
            };

            let event = BeaconEvent {
                topic,
                from: from.clone(),
                to: to.clone(),
                since: prior_state.since,
            };

            beacon.publish(&event)?;

            // Update state file with new ceiling.
            let new_state = CeilingState {
                ceiling: to,
                since: now,
            };
            new_state.save(state_path)?;

            if matches!(event.topic.as_str(), "wm.keel.degraded") {
                BeaconOutcome::Degraded(event)
            } else {
                BeaconOutcome::Refloat(event)
            }
        }
    };

    Ok(outcome)
}

// ── Tier direction helper ─────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
enum TierChangeDirection {
    Degraded,
    Refloat,
}

fn tier_rank(from: &str, to: &str, healths: &[TierHealth]) -> TierChangeDirection {
    // Build position map from the health slice (lowest index = lowest tier).
    let pos_from = healths.iter().position(|h| h.tier == from);
    let pos_to = healths.iter().position(|h| h.tier == to);

    match (pos_from, pos_to) {
        (Some(f), Some(t)) => {
            if t < f {
                TierChangeDirection::Degraded
            } else {
                TierChangeDirection::Refloat
            }
        }
        // If we can't find either in the health list, default based on
        // a known name ordering (local < cloud).
        _ => {
            if is_cloud_tier(from) && !is_cloud_tier(to) {
                TierChangeDirection::Degraded
            } else {
                TierChangeDirection::Refloat
            }
        }
    }
}

fn is_cloud_tier(name: &str) -> bool {
    matches!(name, "haiku" | "sonnet" | "opus")
}
