//! Private-intent fixture loader for the demo binary.
//!
//! The demo binary needs private witness material (requester budget,
//! provider reservation) for each node in the topology so the runtime can
//! generate commitments. Those witnesses NEVER appear in the coordination
//! log or on the command line; they live in a separate TOML file.
//!
//! File format:
//!
//! ```toml
//! version = 1
//!
//! [[agents]]
//! node = "1111...64 hex..."
//! role = "requester"
//! required_capability = "GPU"
//! budget_cents = 1000
//!
//! [[agents]]
//! node = "2222...64 hex..."
//! role = "provider"
//! capability_claims = ["GPU", "LLM"]
//! reservation_cents = 500
//! ```
//!
//! The loader fails closed: mismatched capability labels, mismatched roles
//! vs. topology, missing witness fields, and unknown node ids all produce
//! structured errors rather than silent drift. Private field values are
//! parsed but never echoed in error messages.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path};

use serde::Deserialize;
use vertex_veil_core::{
    AgentState, CapabilityTag, NodeId, PrivateProviderIntent, PrivateRequesterIntent, Role,
    Secret, TopologyConfig,
};

#[derive(Debug, thiserror::Error)]
pub enum PrivateIntentError {
    #[error("unsafe private-intent path: {0}")]
    UnsafePath(String),
    #[error("io error reading private-intent file: {0}")]
    Io(String),
    #[error("failed to parse private-intent TOML: {0}")]
    Parse(String),
    #[error("unsupported private-intent version: {0}")]
    UnsupportedVersion(u32),
    #[error("invalid node id in private-intent entry: {0}")]
    InvalidNodeId(String),
    #[error("unknown node id in private-intent entry: {0}")]
    UnknownNodeId(String),
    #[error("duplicate private-intent entry for node id: {0}")]
    DuplicateEntry(String),
    #[error("private-intent role does not match topology role for node id: {0}")]
    RoleMismatch(String),
    #[error("missing capability field for node id: {0}")]
    MissingCapability(String),
    #[error("unknown or malformed capability tag for node id: {0}")]
    BadCapability(String),
    #[error("missing witness field for node id: {0} (role {1})")]
    MissingWitness(String, &'static str),
    #[error("topology has a node with no private-intent entry: {0}")]
    MissingAgent(String),
}

#[derive(Debug, Deserialize)]
struct RawBundle {
    version: u32,
    #[serde(default)]
    agents: Vec<RawAgent>,
}

#[derive(Debug, Deserialize)]
struct RawAgent {
    node: String,
    role: String,
    #[serde(default)]
    required_capability: Option<String>,
    #[serde(default)]
    capability_claims: Option<Vec<String>>,
    #[serde(default)]
    budget_cents: Option<u64>,
    #[serde(default)]
    reservation_cents: Option<u64>,
}

/// Load a private-intent bundle and cross-validate it against the topology.
pub fn load(
    path: impl AsRef<Path>,
    topology: &TopologyConfig,
) -> Result<BTreeMap<NodeId, AgentState>, PrivateIntentError> {
    let path = path.as_ref();
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(PrivateIntentError::UnsafePath(
            "private-intent path must not contain '..' segments".into(),
        ));
    }
    let text = fs::read_to_string(path).map_err(|e| PrivateIntentError::Io(e.to_string()))?;
    from_toml_str(&text, topology)
}

/// Parse a private-intent bundle from a TOML string.
pub fn from_toml_str(
    text: &str,
    topology: &TopologyConfig,
) -> Result<BTreeMap<NodeId, AgentState>, PrivateIntentError> {
    let bundle: RawBundle =
        toml::from_str(text).map_err(|e| redact_parse_error(&e.to_string()))?;
    if bundle.version != 1 {
        return Err(PrivateIntentError::UnsupportedVersion(bundle.version));
    }

    let mut out: BTreeMap<NodeId, AgentState> = BTreeMap::new();

    for raw in bundle.agents {
        let node: NodeId = raw
            .node
            .parse()
            .map_err(|_| PrivateIntentError::InvalidNodeId(redact_tail(&raw.node)))?;
        let topology_node = topology
            .nodes
            .iter()
            .find(|n| n.id == node)
            .ok_or_else(|| PrivateIntentError::UnknownNodeId(node.to_hex()))?;

        if out.contains_key(&node) {
            return Err(PrivateIntentError::DuplicateEntry(node.to_hex()));
        }

        let declared_role = parse_role(&raw.role)
            .ok_or_else(|| PrivateIntentError::RoleMismatch(node.to_hex()))?;
        if declared_role != topology_node.role {
            return Err(PrivateIntentError::RoleMismatch(node.to_hex()));
        }

        let agent = match topology_node.role {
            Role::Requester => {
                let cap = raw
                    .required_capability
                    .as_ref()
                    .ok_or_else(|| PrivateIntentError::MissingCapability(node.to_hex()))?;
                let cap = CapabilityTag::parse_shape(cap)
                    .map_err(|_| PrivateIntentError::BadCapability(node.to_hex()))?;
                let topo_cap = topology_node
                    .required_capability
                    .as_ref()
                    .expect("topology-required-capability");
                if topo_cap != &cap {
                    return Err(PrivateIntentError::BadCapability(node.to_hex()));
                }
                let budget = raw
                    .budget_cents
                    .ok_or_else(|| PrivateIntentError::MissingWitness(node.to_hex(), "requester"))?;
                AgentState::requester(PrivateRequesterIntent {
                    node_id: node,
                    required_capability: cap,
                    budget_cents: Secret::new(budget),
                })
            }
            Role::Provider => {
                let raws = raw
                    .capability_claims
                    .as_ref()
                    .ok_or_else(|| PrivateIntentError::MissingCapability(node.to_hex()))?;
                let mut claims: Vec<CapabilityTag> = Vec::with_capacity(raws.len());
                for s in raws {
                    let c = CapabilityTag::parse_shape(s)
                        .map_err(|_| PrivateIntentError::BadCapability(node.to_hex()))?;
                    claims.push(c);
                }
                if topology_node.capability_claims != claims {
                    return Err(PrivateIntentError::BadCapability(node.to_hex()));
                }
                let reservation = raw.reservation_cents.ok_or_else(|| {
                    PrivateIntentError::MissingWitness(node.to_hex(), "provider")
                })?;
                AgentState::provider(PrivateProviderIntent {
                    node_id: node,
                    capability_claims: claims,
                    reservation_cents: Secret::new(reservation),
                })
            }
        };

        out.insert(node, agent);
    }

    for tnode in &topology.nodes {
        if !out.contains_key(&tnode.id) {
            return Err(PrivateIntentError::MissingAgent(tnode.id.to_hex()));
        }
    }

    Ok(out)
}

fn parse_role(s: &str) -> Option<Role> {
    match s {
        "requester" => Some(Role::Requester),
        "provider" => Some(Role::Provider),
        _ => None,
    }
}

fn redact_parse_error(raw: &str) -> PrivateIntentError {
    // Strip TOML's source-echo section (pipe-prefixed lines) so the error
    // does not surface private field values from the malformed file.
    let cleaned: String = raw
        .lines()
        .filter(|l| !l.contains('|'))
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    PrivateIntentError::Parse(cleaned)
}

fn redact_tail(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.len() <= 12 {
        trimmed.to_string()
    } else {
        format!("{}…", &trimmed[..12])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex64(b: u8) -> String {
        let mut s = String::new();
        for _ in 0..32 {
            s.push_str(&format!("{:02x}", b));
        }
        s
    }

    fn topology_text() -> String {
        format!(
            r#"
version = 1
capability_tags = ["GPU", "CPU"]

[[nodes]]
id = "{}"
role = "requester"
required_capability = "GPU"

[[nodes]]
id = "{}"
role = "provider"
capability_claims = ["GPU"]
"#,
            hex64(0x11),
            hex64(0x22),
        )
    }

    fn private_text() -> String {
        format!(
            r#"
version = 1

[[agents]]
node = "{}"
role = "requester"
required_capability = "GPU"
budget_cents = 1000

[[agents]]
node = "{}"
role = "provider"
capability_claims = ["GPU"]
reservation_cents = 500
"#,
            hex64(0x11),
            hex64(0x22),
        )
    }

    #[test]
    fn round_trips() {
        let t = TopologyConfig::from_toml_str(&topology_text()).unwrap();
        let agents = from_toml_str(&private_text(), &t).unwrap();
        assert_eq!(agents.len(), 2);
    }

    #[test]
    fn missing_witness_errors() {
        let t = TopologyConfig::from_toml_str(&topology_text()).unwrap();
        let bad = format!(
            r#"
version = 1

[[agents]]
node = "{}"
role = "requester"
required_capability = "GPU"

[[agents]]
node = "{}"
role = "provider"
capability_claims = ["GPU"]
reservation_cents = 500
"#,
            hex64(0x11),
            hex64(0x22),
        );
        let err = from_toml_str(&bad, &t).unwrap_err();
        assert!(matches!(err, PrivateIntentError::MissingWitness(_, _)));
    }

    #[test]
    fn role_mismatch_rejected() {
        let t = TopologyConfig::from_toml_str(&topology_text()).unwrap();
        let bad = format!(
            r#"
version = 1

[[agents]]
node = "{}"
role = "provider"
capability_claims = ["GPU"]
reservation_cents = 100

[[agents]]
node = "{}"
role = "provider"
capability_claims = ["GPU"]
reservation_cents = 500
"#,
            hex64(0x11),
            hex64(0x22),
        );
        let err = from_toml_str(&bad, &t).unwrap_err();
        assert!(matches!(err, PrivateIntentError::RoleMismatch(_)));
    }

    #[test]
    fn parse_errors_redact_pipes() {
        let t = TopologyConfig::from_toml_str(&topology_text()).unwrap();
        let err = from_toml_str("not toml at all", &t).unwrap_err();
        let msg = format!("{err}");
        assert!(!msg.contains('|'));
    }
}
