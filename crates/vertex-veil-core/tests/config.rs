//! Integration tests for `TopologyConfig` loading and validation.
//!
//! These tests cover the Phase 0 plan rows for: happy-path baseline load,
//! invalid topology rejection, unknown / malformed capability tags, missing
//! fields, invalid stable key parsing, empty provider list, single-tag set,
//! duplicate node ids, and unsafe path traversal. The test names start with
//! `config_` so the `cargo test config` filter from the E2E gate selects
//! them.

use std::path::PathBuf;

use vertex_veil_core::{ConfigError, Role, TopologyConfig};

fn hex_of(b: u8) -> String {
    let mut s = String::with_capacity(64);
    for _ in 0..32 {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn baseline_text() -> String {
    format!(
        r#"
version = 1
capability_tags = ["GPU", "CPU", "LLM", "ZK_DEV"]

[[nodes]]
id = "{r}"
role = "requester"
required_capability = "GPU"

[[nodes]]
id = "{p1}"
role = "provider"
capability_claims = ["GPU", "LLM"]

[[nodes]]
id = "{p2}"
role = "provider"
capability_claims = ["GPU"]

[[nodes]]
id = "{p3}"
role = "provider"
capability_claims = ["CPU"]
"#,
        r = hex_of(0x11),
        p1 = hex_of(0x22),
        p2 = hex_of(0x33),
        p3 = hex_of(0x44),
    )
}

#[test]
fn config_loads_4node_baseline() {
    let cfg = TopologyConfig::from_toml_str(&baseline_text()).unwrap();
    assert_eq!(cfg.nodes.len(), 4);
    assert_eq!(cfg.requester().role, Role::Requester);
    assert_eq!(cfg.providers_stable_order().len(), 3);
}

#[test]
fn config_loads_baseline_fixture_file() {
    let path: PathBuf = repo_root().join("fixtures/topology-4node.toml");
    let cfg = TopologyConfig::load(&path).expect("baseline fixture loads");
    assert_eq!(cfg.nodes.len(), 4);
}

#[test]
fn config_rejects_invalid_topology_no_requester() {
    let text = format!(
        r#"
version = 1
capability_tags = ["GPU"]

[[nodes]]
id = "{p1}"
role = "provider"
capability_claims = ["GPU"]
"#,
        p1 = hex_of(0x22),
    );
    let err = TopologyConfig::from_toml_str(&text).unwrap_err();
    assert!(matches!(err, ConfigError::InvalidTopology(_)));
}

#[test]
fn config_rejects_unknown_capability_tag() {
    let text = format!(
        r#"
version = 1
capability_tags = ["GPU"]

[[nodes]]
id = "{r}"
role = "requester"
required_capability = "QUANTUM"

[[nodes]]
id = "{p1}"
role = "provider"
capability_claims = ["GPU"]
"#,
        r = hex_of(0x11),
        p1 = hex_of(0x22),
    );
    let err = TopologyConfig::from_toml_str(&text).unwrap_err();
    assert!(matches!(err, ConfigError::UnknownCapabilityTag(_)));
}

#[test]
fn config_rejects_malformed_capability_tag() {
    let text = format!(
        r#"
version = 1
capability_tags = ["gpu"]

[[nodes]]
id = "{r}"
role = "requester"
required_capability = "gpu"

[[nodes]]
id = "{p1}"
role = "provider"
capability_claims = ["gpu"]
"#,
        r = hex_of(0x11),
        p1 = hex_of(0x22),
    );
    let err = TopologyConfig::from_toml_str(&text).unwrap_err();
    assert!(matches!(err, ConfigError::MalformedCapabilityTag(_)));
}

#[test]
fn config_rejects_missing_required_capability_for_requester() {
    let text = format!(
        r#"
version = 1
capability_tags = ["GPU"]

[[nodes]]
id = "{r}"
role = "requester"

[[nodes]]
id = "{p1}"
role = "provider"
capability_claims = ["GPU"]
"#,
        r = hex_of(0x11),
        p1 = hex_of(0x22),
    );
    let err = TopologyConfig::from_toml_str(&text).unwrap_err();
    assert_eq!(err, ConfigError::MissingField("required_capability"));
}

#[test]
fn config_rejects_invalid_stable_key_input() {
    let text = format!(
        r#"
version = 1
capability_tags = ["GPU"]

[[nodes]]
id = "not-hex"
role = "requester"
required_capability = "GPU"

[[nodes]]
id = "{p1}"
role = "provider"
capability_claims = ["GPU"]
"#,
        p1 = hex_of(0x22),
    );
    let err = TopologyConfig::from_toml_str(&text).unwrap_err();
    assert!(matches!(err, ConfigError::InvalidStableKey(_)));
}

#[test]
fn config_rejects_empty_provider_list() {
    let path: PathBuf = repo_root().join("fixtures/topology-empty-providers.toml");
    let err = TopologyConfig::load(&path).unwrap_err();
    assert_eq!(err, ConfigError::EmptyProviderList);
}

#[test]
fn config_rejects_duplicate_node_ids() {
    let path: PathBuf = repo_root().join("fixtures/topology-duplicate-ids.toml");
    let err = TopologyConfig::load(&path).unwrap_err();
    assert!(matches!(err, ConfigError::DuplicateNodeId(_)));
}

#[test]
fn config_loads_single_tag_topology() {
    let path: PathBuf = repo_root().join("fixtures/topology-single-tag.toml");
    let cfg = TopologyConfig::load(&path).unwrap();
    assert_eq!(cfg.capability_tags.len(), 1);
    assert!(!cfg.providers_stable_order().is_empty());
}

#[test]
fn config_rejects_path_traversal() {
    let err = TopologyConfig::load(PathBuf::from("../etc/passwd")).unwrap_err();
    assert!(matches!(err, ConfigError::UnsafePath(_)));
}

#[test]
fn config_error_does_not_print_private_fixture_values() {
    // Construct a malformed TOML that mentions a value we treat as private
    // (a fake budget). The parser must not echo the value back in the error.
    let text = r#"
version = 1
capability_tags = ["GPU"]
this_is_not_a_field_at_all = 8675309_secret_budget
"#;
    let err = TopologyConfig::from_toml_str(text).unwrap_err();
    let msg = format!("{err}");
    assert!(
        !msg.contains("8675309_secret_budget"),
        "config error must not echo private-looking values; got: {msg}"
    );
}

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points at crates/vertex-veil-core; go up two levels.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root from manifest dir")
        .to_path_buf()
}
