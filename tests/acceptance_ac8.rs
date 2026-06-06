//! AC8: README documents type surface and TierProbe/ProbeEnv traits.

use std::fs;

#[test]
fn readme_exists_and_documents_key_types() {
    let readme = fs::read_to_string("README.md")
        .expect("README.md must exist at the crate root");

    let required_terms = [
        "TierProbe",
        "ProbeEnv",
        "TierHealth",
        "LedgerEntry",
        "Ladder",
    ];

    for term in &required_terms {
        assert!(
            readme.contains(term),
            "README.md must contain '{term}' (contract for sibling crates)"
        );
    }
}
