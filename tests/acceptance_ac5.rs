//! AC5: A local TierConfig whose endpoint refuses connection → Unreachable{reason}.
//! A reachable one → Reachable.  The probe uses a non-generating endpoint
//! (FakeProbe sees /v1/models-class path, never /v1/chat/completions).

use keel::probe::{FakeProbe, FakeProbeEnv, TierProbe};
use keel::types::{TierConfig, TierKind, TierStatus};

#[test]
fn reachable_local_tier_returns_reachable() {
    let probe = FakeProbe::new(); // default: all → Reachable
    let env = FakeProbeEnv::default();
    let tier = TierConfig {
        name: "local-3b".to_string(),
        kind: TierKind::Local,
        endpoint: "http://localhost:11434".to_string(),
    };

    let health = probe.probe(&tier, &env);
    assert_eq!(health.status, TierStatus::Reachable);
}

#[test]
fn unreachable_local_tier_returns_unreachable() {
    let mut probe = FakeProbe::new();
    probe.responses.insert(
        "local-3b".to_string(),
        TierStatus::Unreachable {
            reason: "connection refused".to_string(),
        },
    );
    let env = FakeProbeEnv::default();
    let tier = TierConfig {
        name: "local-3b".to_string(),
        kind: TierKind::Local,
        endpoint: "http://localhost:11434".to_string(),
    };

    let health = probe.probe(&tier, &env);
    assert!(
        matches!(health.status, TierStatus::Unreachable { .. }),
        "expected Unreachable, got {:?}",
        health.status
    );
}

#[test]
fn probe_path_is_models_not_completions() {
    let probe = FakeProbe::new();
    let env = FakeProbeEnv {
        key: Some("sk-test".to_string()),
        ..Default::default()
    };
    let tier = TierConfig {
        name: "local-3b".to_string(),
        kind: TierKind::Local,
        endpoint: "http://localhost:11434".to_string(),
    };

    probe.probe(&tier, &env);

    let calls = probe.recorded_calls();
    assert_eq!(calls.len(), 1, "expected exactly one probe call");
    let call = &calls[0];
    assert!(
        call.path.contains("models") || call.path.contains("health"),
        "probe path should be /v1/models or /health, not completions; got {}",
        call.path
    );
    assert!(
        !call.path.contains("completions"),
        "probe must never use /v1/chat/completions; got {}",
        call.path
    );
}
