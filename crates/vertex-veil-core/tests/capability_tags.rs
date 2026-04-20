//! Integration tests for runtime-configurable capability tags.
//!
//! Plan coverage (Edge Cases):
//!
//! - Runtime-configurable capability tags work with a custom label set
//!   beyond the illustrative `GPU` / `CPU` / `LLM` / `ZK_DEV` defaults.

use std::path::PathBuf;

use vertex_veil_core::{CapabilityTagSet, ConfigError, TopologyConfig};

fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root from manifest dir")
        .to_path_buf()
}

#[test]
fn capability_tags_custom_labels_load_from_fixture() {
    let path = repo_root().join("fixtures/topology-custom-tags.toml");
    let cfg = TopologyConfig::load(&path).expect("custom-tag topology loads");
    assert_eq!(cfg.capability_tags.len(), 3);
    let req = cfg.requester();
    assert_eq!(
        req.required_capability.as_ref().unwrap().as_str(),
        "SPECIAL_ZONE"
    );
}

#[test]
fn capability_tags_custom_set_normalizes_correctly() {
    let set = CapabilityTagSet::new(["LINEAR_ALGEBRA", "SPECIAL_ZONE"]).unwrap();
    assert!(set.normalize("SPECIAL_ZONE").is_ok());
    assert!(matches!(
        set.normalize("GPU"),
        Err(ConfigError::UnknownCapabilityTag(_))
    ));
}

#[test]
fn capability_tags_custom_and_illustrative_are_independent() {
    let custom = CapabilityTagSet::new(["SPECIAL_ZONE"]).unwrap();
    let defaults = CapabilityTagSet::illustrative_defaults();
    // No cross-contamination: defaults don't contain the custom tag.
    assert!(defaults.normalize("SPECIAL_ZONE").is_err());
    // And the custom set doesn't contain the defaults.
    assert!(custom.normalize("GPU").is_err());
}
