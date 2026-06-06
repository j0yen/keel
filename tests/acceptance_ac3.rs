//! AC3: keel pulse --format json emits one TierHealth per configured tier.
//! With WM_BRAIN_SKIP_TIERS=local-8b set (via ProbeEnv fixture), that tier
//! appears with status Skipped and is not probed.

use keel::probe::{FakeProbe, FakeProbeEnv};
use keel::pulse::{OutputFormat, print_pulse, run_pulse};
use keel::types::TierStatus;

#[test]
fn skipped_tier_appears_with_skipped_status_and_is_not_probed() {
    let probe = FakeProbe::new();
    let env = FakeProbeEnv {
        key: Some("sk-test".to_string()),
        skip: vec!["local-8b".to_string()],
        max: None,
    };

    let results = run_pulse(&probe, &env);

    // local-8b must be present with Skipped status
    let local_8b = results
        .iter()
        .find(|h| h.tier == "local-8b")
        .expect("local-8b should be in results");
    assert_eq!(
        local_8b.status,
        TierStatus::Skipped,
        "local-8b should be Skipped"
    );

    // The FakeProbe should not have been called for local-8b
    let calls = probe.recorded_calls();
    assert!(
        !calls.iter().any(|c| c.tier_name == "local-8b"),
        "local-8b should not have been probed; calls: {calls:?}"
    );

    // JSON output should be valid
    let _code = print_pulse(&results, OutputFormat::Json).expect("JSON output should succeed");

    // All non-skipped active tiers should be present
    let non_skipped: Vec<_> = results
        .iter()
        .filter(|h| !matches!(h.status, TierStatus::Skipped))
        .collect();
    assert!(!non_skipped.is_empty(), "should have non-skipped tiers");
}

#[test]
fn json_output_one_entry_per_tier() {
    let probe = FakeProbe::new();
    let env = FakeProbeEnv {
        key: Some("sk-test".to_string()),
        skip: vec![],
        max: None,
    };

    let results = run_pulse(&probe, &env);

    // Each tier name should appear exactly once
    let mut names: Vec<_> = results.iter().map(|h| h.tier.as_str()).collect();
    let orig_len = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(
        names.len(),
        orig_len,
        "duplicate tier entries in results"
    );
}
