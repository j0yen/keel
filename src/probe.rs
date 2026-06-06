//! The `TierProbe` and `ProbeEnv` traits and their default implementations.

use crate::types::{TierConfig, TierHealth, TierKind, TierStatus};

/// Reads the probe's configuration from the environment or a fixture.
///
/// All real environment reads go through this trait so tests can inject
/// a `FakeProbeEnv` that never touches `std::env`.
pub trait ProbeEnv: Send + Sync {
    /// Returns the Anthropic API key, or `None` if unset or empty.
    fn anthropic_key(&self) -> Option<String>;

    /// Returns the set of tier names in `WM_BRAIN_SKIP_TIERS` (comma-separated).
    fn skip_tiers(&self) -> Vec<String>;

    /// Returns the max tier name from `WM_BRAIN_MAX_TIER`, or `None`.
    fn max_tier(&self) -> Option<String>;
}

/// Probes one tier and returns its health.
///
/// A local tier probes its OpenAI-compatible endpoint with a non-generating
/// GET (`/v1/models` or `/health`) — never `/v1/chat/completions`.
/// A cloud tier checks key presence first; with a key it calls a minimal
/// authed endpoint (models list) that returns 401/200 without a completion.
///
/// Tests inject a `FakeProbe` that records calls without opening sockets.
pub trait TierProbe: Send + Sync {
    /// Probe `tier` and return its current health.
    fn probe(&self, tier: &TierConfig, env: &dyn ProbeEnv) -> TierHealth;
}

// ---------------------------------------------------------------------------
// Real (system-env) ProbeEnv
// ---------------------------------------------------------------------------

/// Reads probe configuration from the real process environment.
#[derive(Debug, Default)]
pub struct SystemEnv;

impl ProbeEnv for SystemEnv {
    fn anthropic_key(&self) -> Option<String> {
        let v = std::env::var("WM_ANTHROPIC_KEY").unwrap_or_default();
        if v.is_empty() { None } else { Some(v) }
    }

    fn skip_tiers(&self) -> Vec<String> {
        std::env::var("WM_BRAIN_SKIP_TIERS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect()
    }

    fn max_tier(&self) -> Option<String> {
        let v = std::env::var("WM_BRAIN_MAX_TIER").unwrap_or_default();
        if v.is_empty() { None } else { Some(v) }
    }
}

// ---------------------------------------------------------------------------
// Real probe (HTTP, no completions)
// ---------------------------------------------------------------------------

/// HTTP-based probe using ureq.
///
/// Local tiers: GET `{endpoint}/v1/models`.
/// Cloud tiers with key: GET `{endpoint}/v1/models` with Authorization header.
/// Cloud tiers without key: short-circuit to `Keyless` without touching the socket.
#[derive(Debug, Default)]
pub struct HttpProbe;

impl TierProbe for HttpProbe {
    fn probe(&self, tier: &TierConfig, env: &dyn ProbeEnv) -> TierHealth {
        let now = unix_now();

        match tier.kind {
            TierKind::Cloud => {
                let key = match env.anthropic_key() {
                    None => {
                        return TierHealth {
                            tier: tier.name.clone(),
                            status: TierStatus::Keyless,
                            checked_at: now,
                            consecutive_failures: 0,
                        };
                    }
                    Some(k) => k,
                };

                let url = format!("{}/v1/models", tier.endpoint.trim_end_matches('/'));
                let result = ureq::get(&url)
                    .set("x-api-key", &key)
                    .set("anthropic-version", "2023-06-01")
                    .call();

                let status = match result {
                    Ok(_) => TierStatus::Reachable,
                    Err(ureq::Error::Status(401 | 403, _)) => TierStatus::Unreachable {
                        reason: "auth rejected (401/403)".to_string(),
                    },
                    Err(ureq::Error::Status(429 | 402, _)) => TierStatus::Exhausted,
                    Err(ureq::Error::Status(code, _)) => TierStatus::Unreachable {
                        reason: format!("HTTP {code}"),
                    },
                    Err(ureq::Error::Transport(e)) => TierStatus::Unreachable {
                        reason: e.to_string(),
                    },
                };

                TierHealth {
                    tier: tier.name.clone(),
                    status,
                    checked_at: now,
                    consecutive_failures: 0,
                }
            }

            TierKind::Local => {
                let url = format!("{}/v1/models", tier.endpoint.trim_end_matches('/'));
                let result = ureq::get(&url).call();

                let status = match result {
                    Ok(_) => TierStatus::Reachable,
                    Err(ureq::Error::Status(429, _)) => TierStatus::Exhausted,
                    Err(ureq::Error::Status(code, _)) => TierStatus::Unreachable {
                        reason: format!("HTTP {code}"),
                    },
                    Err(ureq::Error::Transport(e)) => TierStatus::Unreachable {
                        reason: e.to_string(),
                    },
                };

                TierHealth {
                    tier: tier.name.clone(),
                    status,
                    checked_at: now,
                    consecutive_failures: 0,
                }
            }
        }
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| {
            i64::try_from(d.as_secs()).unwrap_or(i64::MAX)
        })
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Fake (test) doubles
// ---------------------------------------------------------------------------

/// A recorded probe call.
#[derive(Debug, Clone)]
pub struct ProbeCall {
    /// The tier name that was probed.
    pub tier_name: String,
    /// The URL path that would have been requested (e.g. "/v1/models").
    pub path: String,
}

/// A probe that never opens a socket; returns pre-configured `TierHealth` values.
///
/// Used in tests to verify call patterns without network access.
pub struct FakeProbe {
    /// Responses keyed by tier name. Tiers not in this map return `Reachable`.
    pub responses: std::collections::HashMap<String, TierStatus>,
    /// Records every call made (mutable via interior mutability).
    pub calls: std::sync::Mutex<Vec<ProbeCall>>,
}

impl FakeProbe {
    /// Create a new `FakeProbe` with no pre-configured responses (all → `Reachable`).
    #[must_use]
    pub fn new() -> Self {
        Self {
            responses: std::collections::HashMap::new(),
            calls: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Returns the recorded calls.
    ///
    /// # Panics
    ///
    /// If the internal mutex is poisoned (can only happen if a test thread panicked
    /// while holding the lock — benign in single-threaded test contexts).
    #[must_use]
    pub fn recorded_calls(&self) -> Vec<ProbeCall> {
        self.calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
}

impl Default for FakeProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl TierProbe for FakeProbe {
    fn probe(&self, tier: &TierConfig, env: &dyn ProbeEnv) -> TierHealth {
        // Short-circuit keyless before recording a call (mirrors HttpProbe behaviour)
        if tier.kind == TierKind::Cloud && env.anthropic_key().is_none() {
            return TierHealth {
                tier: tier.name.clone(),
                status: TierStatus::Keyless,
                checked_at: 0,
                consecutive_failures: 0,
            };
        }

        // Record the call with a /v1/models path (never /v1/chat/completions)
        let call = ProbeCall {
            tier_name: tier.name.clone(),
            path: "/v1/models".to_string(),
        };
        // Only panics if another test thread panicked while holding the lock
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(call);

        let status = self
            .responses
            .get(&tier.name)
            .cloned()
            .unwrap_or(TierStatus::Reachable);

        TierHealth {
            tier: tier.name.clone(),
            status,
            checked_at: 0,
            consecutive_failures: 0,
        }
    }
}

/// A `ProbeEnv` that returns fixture values — never reads `std::env`.
#[derive(Debug, Default)]
pub struct FakeProbeEnv {
    /// Returned by `anthropic_key()`. `None` simulates an unset/empty key.
    pub key: Option<String>,
    /// Returned by `skip_tiers()`.
    pub skip: Vec<String>,
    /// Returned by `max_tier()`.
    pub max: Option<String>,
}

impl ProbeEnv for FakeProbeEnv {
    fn anthropic_key(&self) -> Option<String> {
        self.key.clone()
    }

    fn skip_tiers(&self) -> Vec<String> {
        self.skip.clone()
    }

    fn max_tier(&self) -> Option<String> {
        self.max.clone()
    }
}
