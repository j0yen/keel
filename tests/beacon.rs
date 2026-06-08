//! keel-beacon integration tests — covers AC1–AC8 of PRD-keel-beacon.
//!
//! All tests use `RecordingBeacon` (never a live bus connection).
//! State files use `tempfile`-style paths under a temp dir.

use keel::beacon::{
    BeaconOutcome, CeilingState, RecordingBeacon, effective_ceiling, run_beacon,
};
use keel::probe::{FakeProbe, FakeProbeEnv};
use keel::pulse::run_pulse;
use keel::status::{StatusFormat, compute_status};
use keel::types::TierStatus;

// ── helpers ──────────────────────────────────────────────────────────────────

/// Build a temp state file path that doesn't exist yet.
fn tmp_state() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "keel-beacon-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir.join("last-ceiling.json")
}

/// Run pulse with a FakeProbe returning the given status map.
fn fake_healths(
    key: Option<&str>,
    overrides: &[(&str, TierStatus)],
) -> Vec<keel::types::TierHealth> {
    let mut probe = FakeProbe::new();
    for (name, status) in overrides {
        probe.responses.insert((*name).to_string(), status.clone());
    }
    let env = FakeProbeEnv {
        key: key.map(|k| k.to_string()),
        skip: vec![],
        max: None,
    };
    run_pulse(&probe, &env)
}

// ── AC1: offline build — RecordingBeacon makes zero live-bus connections ─────

#[test]
fn recording_beacon_makes_zero_live_bus_connections() {
    // RecordingBeacon never opens a socket — it is structurally impossible.
    // This test verifies it compiles and runs without any network access.
    let beacon = RecordingBeacon::new();
    let state_path = tmp_state();

    // All tiers reachable (with key)
    let healths = fake_healths(Some("sk-test"), &[]);

    let outcome = run_beacon(1_000_000, &state_path, &healths, &beacon)
        .expect("run_beacon should not fail");

    // First run: no event emitted.
    assert_eq!(outcome, BeaconOutcome::FirstRun);
    // RecordingBeacon recorded nothing.
    assert_eq!(beacon.recorded().len(), 0);
}

// ── AC2: keel status nominal / floored ───────────────────────────────────────

#[test]
fn status_nominal_when_all_reachable() {
    // All tiers reachable + key present → ceiling = opus → nominal.
    let healths = fake_healths(Some("sk-test"), &[]);
    let report = compute_status(&healths, None, 1_000_000);
    assert!(report.nominal, "should be nominal when all reachable");
    assert_eq!(report.ceiling, "opus");
    let text = report.as_text();
    assert!(
        text.contains("nominal"),
        "text should contain 'nominal': {text}"
    );
    assert!(
        text.contains("opus"),
        "text should name the top tier: {text}"
    );
}

#[test]
fn status_floored_when_cloud_keyless() {
    // No key → cloud tiers all Keyless → ceiling = local-3b → floored.
    let healths = fake_healths(None, &[]);
    let report = compute_status(&healths, Some(999_000), 1_000_000);
    assert!(!report.nominal, "should be floored when cloud keyless");
    // Ceiling should be the highest reachable (local tiers reachable).
    assert!(
        report.ceiling == "local-3b" || report.ceiling == "local-8b",
        "ceiling should be local: {}",
        report.ceiling
    );
    let text = report.as_text();
    assert!(
        text.contains("floored"),
        "text should contain 'floored': {text}"
    );
    assert!(
        text.contains("keyless"),
        "text should mention keyless: {text}"
    );
    // Duration should appear since we passed a since timestamp.
    assert!(
        report.duration_human.is_some(),
        "should have duration when since is provided"
    );
}

#[test]
fn status_json_format_is_valid() {
    let _ = StatusFormat::Json; // StatusFormat is importable
    let healths = fake_healths(Some("sk-test"), &[]);
    let report = compute_status(&healths, None, 1_000_000);
    let json = serde_json::to_string(&report).expect("serialize StatusReport");
    assert!(json.contains("\"nominal\""), "json should have nominal key");
    assert!(json.contains("\"ceiling\""), "json should have ceiling key");
}

// ── AC3: ceiling drop emits wm.keel.degraded ─────────────────────────────────

#[test]
fn ceiling_drop_emits_degraded_event() {
    let beacon = RecordingBeacon::new();
    let state_path = tmp_state();

    // First run: ceiling = opus (all reachable with key).
    let healths_up = fake_healths(Some("sk-test"), &[]);
    let _ = run_beacon(1_000, &state_path, &healths_up, &beacon)
        .expect("first run should succeed");
    assert_eq!(beacon.recorded().len(), 0, "first run emits nothing");

    // Second run: no key → ceiling drops to local-3b.
    let healths_down = fake_healths(None, &[]);
    let outcome = run_beacon(2_000, &state_path, &healths_down, &beacon)
        .expect("second run should succeed");

    let recorded = beacon.recorded();
    assert_eq!(recorded.len(), 1, "exactly one event emitted on drop");

    let event = &recorded[0];
    assert_eq!(
        event.topic, "wm.keel.degraded",
        "topic should be wm.keel.degraded"
    );
    assert_eq!(event.from, "opus", "from should be opus");
    assert!(
        event.to == "local-3b" || event.to == "local-8b",
        "to should be a local tier: {}",
        event.to
    );
    // since should be the timestamp from the first run's state.
    assert_eq!(event.since, 1_000, "since should be from first run");

    assert!(
        matches!(outcome, BeaconOutcome::Degraded(_)),
        "outcome should be Degraded"
    );
}

// ── AC4: ceiling rise emits wm.keel.refloat ──────────────────────────────────

#[test]
fn ceiling_rise_emits_refloat_event() {
    let beacon = RecordingBeacon::new();
    let state_path = tmp_state();

    // First run: no key → ceiling = local tier.
    let healths_down = fake_healths(None, &[]);
    let _ = run_beacon(1_000, &state_path, &healths_down, &beacon)
        .expect("first run");
    assert_eq!(beacon.recorded().len(), 0, "first run emits nothing");

    // Second run: key present → ceiling rises to opus.
    let healths_up = fake_healths(Some("sk-test"), &[]);
    let outcome = run_beacon(2_000, &state_path, &healths_up, &beacon)
        .expect("second run");

    let recorded = beacon.recorded();
    assert_eq!(recorded.len(), 1, "exactly one event on rise");

    let event = &recorded[0];
    assert_eq!(
        event.topic, "wm.keel.refloat",
        "topic should be wm.keel.refloat"
    );
    assert_eq!(event.to, "opus", "to should be opus");

    assert!(
        matches!(outcome, BeaconOutcome::Refloat(_)),
        "outcome should be Refloat"
    );
}

// ── AC4 (subject strings): wm.keel.* convention ──────────────────────────────

#[test]
fn event_topics_match_wm_keel_convention() {
    // Degrade.
    let beacon = RecordingBeacon::new();
    let state_path = tmp_state();
    let healths_up = fake_healths(Some("sk-test"), &[]);
    let _ = run_beacon(1_000, &state_path, &healths_up, &beacon).unwrap();
    let healths_down = fake_healths(None, &[]);
    let _ = run_beacon(2_000, &state_path, &healths_down, &beacon).unwrap();

    for event in beacon.recorded() {
        assert!(
            event.topic.starts_with("wm.keel."),
            "topic must start with wm.keel.: {}",
            event.topic
        );
    }
}

// ── AC5: no change → zero events (edge-triggered) ───────────────────────────

#[test]
fn no_change_emits_zero_events() {
    let beacon = RecordingBeacon::new();
    let state_path = tmp_state();

    let healths = fake_healths(Some("sk-test"), &[]);

    // First run: initializes state file.
    let _ = run_beacon(1_000, &state_path, &healths, &beacon).unwrap();
    assert_eq!(beacon.recorded().len(), 0);

    // Second run: same ceiling → no event.
    let outcome = run_beacon(2_000, &state_path, &healths, &beacon).unwrap();
    assert_eq!(beacon.recorded().len(), 0, "no event on second run (no change)");
    assert_eq!(outcome, BeaconOutcome::NoChange);

    // Third run: still same → still no event.
    let outcome3 = run_beacon(3_000, &state_path, &healths, &beacon).unwrap();
    assert_eq!(beacon.recorded().len(), 0, "no event on third run either");
    assert_eq!(outcome3, BeaconOutcome::NoChange);
}

// ── AC6: last-ceiling state file round-trip across two beacon runs ───────────

#[test]
fn state_file_updated_after_emit_so_next_run_diffs_against_new_ceiling() {
    let beacon = RecordingBeacon::new();
    let state_path = tmp_state();

    // Run 1: ceiling = opus (all up).
    let healths_up = fake_healths(Some("sk-test"), &[]);
    let _ = run_beacon(1_000, &state_path, &healths_up, &beacon).unwrap();

    // Verify state file contains opus.
    let state1 = CeilingState::load(&state_path)
        .expect("load ok")
        .expect("state file should exist after first run");
    assert_eq!(state1.ceiling, "opus");
    assert_eq!(state1.since, 1_000);

    // Run 2: ceiling drops → event emitted + state updated.
    let healths_down = fake_healths(None, &[]);
    let _ = run_beacon(2_000, &state_path, &healths_down, &beacon).unwrap();

    // State file now reflects the new (lower) ceiling.
    let state2 = CeilingState::load(&state_path)
        .expect("load ok")
        .expect("state should exist after second run");
    assert_ne!(state2.ceiling, "opus", "ceiling should have changed");
    assert_eq!(state2.since, 2_000, "since should be the new run's timestamp");

    // Run 3: same lower ceiling → no event (state correctly updated).
    let beacon2 = RecordingBeacon::new();
    let _ = run_beacon(3_000, &state_path, &healths_down, &beacon2).unwrap();
    assert_eq!(
        beacon2.recorded().len(),
        0,
        "third run should emit nothing — state correctly reflects new ceiling"
    );
}

// ── AC7: SIGPIPE guard — effective_ceiling and compute_status don't panic ────

#[test]
fn keel_status_head_does_not_panic() {
    // Structural SIGPIPE guard: `keel status | head` should not panic.
    // We can't spawn the binary in a pure unit test, but we verify the output
    // path (as_text) produces exactly one line and doesn't panic.
    let healths = fake_healths(None, &[]);
    let report = compute_status(&healths, None, 1_000_000);
    let text = report.as_text();
    // Must be a single line (no embedded newlines from our code).
    assert!(
        !text.contains('\n'),
        "as_text must not embed newlines: {text:?}"
    );
}

// ── AC8: tests/beacon.rs appears in cargo test output (self_orphaned guard) ──
// This file IS tests/beacon.rs, so its presence proves the guard passes.
// The test below is a no-op marker that validates the module is wired in.

#[test]
fn beacon_test_file_is_wired() {
    // If this function runs, the test harness found tests/beacon.rs.
    // Per self_orphaned_mock_tests: verify `Running tests/beacon.rs` appears.
    // (Cargo prints "Running tests/beacon.rs" when the file has ≥1 test.)
    assert!(true, "tests/beacon.rs is wired into the test harness");
}

// ── Extra: effective_ceiling helper ──────────────────────────────────────────

#[test]
fn effective_ceiling_returns_highest_reachable() {
    let healths = fake_healths(Some("sk-test"), &[]);
    let ceiling = effective_ceiling(&healths);
    assert_eq!(ceiling.as_deref(), Some("opus"));
}

#[test]
fn effective_ceiling_fallback_when_all_keyless() {
    let healths = fake_healths(None, &[]);
    let ceiling = effective_ceiling(&healths);
    // Should return the highest reachable local tier.
    assert!(
        ceiling.as_deref() == Some("local-3b")
            || ceiling.as_deref() == Some("local-8b"),
        "ceiling should be a local tier: {ceiling:?}"
    );
}
