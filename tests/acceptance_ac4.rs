//! AC4: A cloud TierConfig with no key yields Keyless without opening a socket.
//! FakeProbe records no connect attempt for that tier.

use keel::probe::{FakeProbe, FakeProbeEnv};
use keel::pulse::run_pulse;
use keel::types::TierStatus;

#[test]
fn cloud_tier_with_no_key_yields_keyless_no_socket() {
    let probe = FakeProbe::new();
    let env = FakeProbeEnv {
        key: None, // no API key
        skip: vec![],
        max: None,
    };

    let results = run_pulse(&probe, &env);

    // All cloud tiers (haiku, sonnet, opus) should be Keyless
    for tier_name in &["haiku", "sonnet", "opus"] {
        let health = results
            .iter()
            .find(|h| h.tier.as_str() == *tier_name)
            .unwrap_or_else(|| panic!("{tier_name} should be in results"));
        assert_eq!(
            health.status,
            TierStatus::Keyless,
            "{tier_name} should be Keyless when no key is set"
        );
    }

    // FakeProbe should record NO connect attempts for any cloud tier
    let calls = probe.recorded_calls();
    let cloud_calls: Vec<_> = calls
        .iter()
        .filter(|c| ["haiku", "sonnet", "opus"].contains(&c.tier_name.as_str()))
        .collect();
    assert!(
        cloud_calls.is_empty(),
        "cloud tiers should not be probed when keyless; got calls: {cloud_calls:?}"
    );
}
