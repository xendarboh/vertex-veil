//! Topology configuration for Vertex Veil runs.
//!
//! The 4-node baseline used by the validated `v1` demo is expressed as a TOML
//! file shaped like:
//!
//! ```toml
//! version = 1
//! capability_tags = ["GPU", "CPU", "LLM", "ZK_DEV"]
//!
//! [[nodes]]
//! id = "aaaaaaaa...64 hex..."
//! role = "requester"
//! required_capability = "GPU"
//!
//! [[nodes]]
//! id = "bbbbbbbb...64 hex..."
//! role = "provider"
//! capability_claims = ["GPU", "LLM"]
//! ```
//!
//! Load validation enforces:
//!
//! - exactly one requester
//! - at least one provider
//! - unique node identifiers
//! - capability tags drawn from the configured set
//! - requester declares `required_capability`, providers declare
//!   `capability_claims`
//! - node ids parse as 64 lowercase hex characters

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::capability::{CapabilityTag, CapabilityTagSet};
use crate::error::ConfigError;
use crate::keys::NodeId;
use crate::signing::SigningPublicKey;

/// Node role in a Vertex Veil run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Requester,
    Provider,
}

/// Raw per-node config as deserialized from TOML. Internal to the loader.
#[derive(Deserialize)]
struct NodeConfigRaw {
    id: String,
    role: Role,
    #[serde(default)]
    required_capability: Option<String>,
    #[serde(default)]
    capability_claims: Option<Vec<String>>,
    /// Hex-encoded ed25519 verifying key. Optional for back-compat with
    /// Phase 3 fixtures; required in Phase 4 fixtures.
    #[serde(default)]
    signing_public_key: Option<String>,
}

/// Fully validated node configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeConfig {
    pub id: NodeId,
    pub role: Role,
    /// Present for requesters only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_capability: Option<CapabilityTag>,
    /// Present for providers only.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub capability_claims: Vec<CapabilityTag>,
    /// Ed25519 verifying key used to check the completion receipt signature.
    /// Stored as a 32-byte curve point; serialized as hex in TOML/JSON.
    /// Optional so Phase 3 fixtures keep working against the legacy blake2s
    /// path; Phase 4 fixtures populate this.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_pubkey",
        deserialize_with = "deserialize_pubkey"
    )]
    pub signing_public_key: Option<SigningPublicKey>,
}

fn serialize_pubkey<S: serde::Serializer>(
    v: &Option<SigningPublicKey>,
    s: S,
) -> Result<S::Ok, S::Error> {
    match v {
        Some(pk) => s.serialize_str(&pk.to_hex()),
        None => s.serialize_none(),
    }
}

fn deserialize_pubkey<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<SigningPublicKey>, D::Error> {
    let opt: Option<String> = Option::deserialize(d)?;
    match opt {
        None => Ok(None),
        Some(s) => SigningPublicKey::from_hex(&s)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

/// Raw topology shape used for deserialization.
#[derive(Deserialize)]
struct TopologyConfigRaw {
    #[serde(default = "default_version")]
    version: u32,
    capability_tags: Vec<String>,
    nodes: Vec<NodeConfigRaw>,
}

fn default_version() -> u32 {
    1
}

/// Fully validated topology configuration.
///
/// After [`TopologyConfig::from_toml_str`] or [`TopologyConfig::load`]
/// returns `Ok`, every capability tag has been normalized against the
/// configured [`CapabilityTagSet`], every node identifier is unique, and the
/// requester / provider counts satisfy the `v1` baseline shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TopologyConfig {
    pub version: u32,
    pub capability_tags: CapabilityTagSet,
    pub nodes: Vec<NodeConfig>,
}

impl TopologyConfig {
    /// Parse and validate a topology configuration from an in-memory TOML
    /// string.
    pub fn from_toml_str(text: &str) -> Result<Self, ConfigError> {
        let raw: TopologyConfigRaw = toml::from_str(text).map_err(ConfigError::parse)?;
        Self::from_raw(raw)
    }

    /// Load and validate a topology configuration from a file path.
    ///
    /// Paths containing parent-directory traversal components (`..`) are
    /// rejected to avoid unsafe relative traversal when fixture paths are
    /// passed through untrusted config.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        validate_path_safe(path)?;
        let text = fs::read_to_string(path).map_err(ConfigError::io)?;
        Self::from_toml_str(&text)
    }

    fn from_raw(raw: TopologyConfigRaw) -> Result<Self, ConfigError> {
        if raw.version != 1 {
            return Err(ConfigError::InvalidTopology(format!(
                "unsupported topology version {}",
                raw.version
            )));
        }
        let capability_tags = CapabilityTagSet::new(raw.capability_tags)?;

        if raw.nodes.is_empty() {
            return Err(ConfigError::InvalidTopology(
                "nodes list must not be empty".into(),
            ));
        }

        let mut seen: BTreeSet<NodeId> = BTreeSet::new();
        let mut nodes = Vec::with_capacity(raw.nodes.len());
        let mut requester_count = 0usize;
        let mut provider_count = 0usize;

        for node_raw in raw.nodes {
            let id: NodeId = node_raw.id.parse()?;
            if !seen.insert(id) {
                return Err(ConfigError::DuplicateNodeId(id.to_hex()));
            }

            let required_capability = match (node_raw.role, node_raw.required_capability) {
                (Role::Requester, Some(raw_tag)) => {
                    Some(capability_tags.normalize(&raw_tag)?)
                }
                (Role::Requester, None) => {
                    return Err(ConfigError::MissingField("required_capability"));
                }
                (Role::Provider, Some(_)) => {
                    return Err(ConfigError::InvalidTopology(
                        "provider must not declare required_capability".into(),
                    ));
                }
                (Role::Provider, None) => None,
            };

            let capability_claims = match (node_raw.role, node_raw.capability_claims) {
                (Role::Provider, Some(raws)) => {
                    if raws.is_empty() {
                        return Err(ConfigError::InvalidTopology(
                            "provider capability_claims must not be empty".into(),
                        ));
                    }
                    let mut out = Vec::with_capacity(raws.len());
                    for raw_tag in raws {
                        out.push(capability_tags.normalize(&raw_tag)?);
                    }
                    out
                }
                (Role::Provider, None) => {
                    return Err(ConfigError::MissingField("capability_claims"));
                }
                (Role::Requester, Some(_)) => {
                    return Err(ConfigError::InvalidTopology(
                        "requester must not declare capability_claims".into(),
                    ));
                }
                (Role::Requester, None) => Vec::new(),
            };

            match node_raw.role {
                Role::Requester => requester_count += 1,
                Role::Provider => provider_count += 1,
            }

            let signing_public_key = match node_raw.signing_public_key {
                None => None,
                Some(hex_s) => Some(SigningPublicKey::from_hex(&hex_s).map_err(|_| {
                    ConfigError::InvalidTopology(format!(
                        "invalid signing_public_key for node {}",
                        id.to_hex()
                    ))
                })?),
            };

            nodes.push(NodeConfig {
                id,
                role: node_raw.role,
                required_capability,
                capability_claims,
                signing_public_key,
            });
        }

        if requester_count != 1 {
            return Err(ConfigError::InvalidTopology(format!(
                "exactly one requester required, found {requester_count}"
            )));
        }
        if provider_count == 0 {
            return Err(ConfigError::EmptyProviderList);
        }

        Ok(TopologyConfig {
            version: raw.version,
            capability_tags,
            nodes,
        })
    }

    /// Canonical stable-order iterator over nodes by byte-lex NodeId.
    pub fn nodes_stable_order(&self) -> Vec<&NodeConfig> {
        let mut v: Vec<&NodeConfig> = self.nodes.iter().collect();
        v.sort_by_key(|n| n.id);
        v
    }

    /// Return the sole requester.
    pub fn requester(&self) -> &NodeConfig {
        self.nodes
            .iter()
            .find(|n| n.role == Role::Requester)
            .expect("from_raw enforces exactly one requester")
    }

    /// Return providers in stable key order.
    pub fn providers_stable_order(&self) -> Vec<&NodeConfig> {
        let mut v: Vec<&NodeConfig> = self
            .nodes
            .iter()
            .filter(|n| n.role == Role::Provider)
            .collect();
        v.sort_by_key(|n| n.id);
        v
    }
}

fn validate_path_safe(path: &Path) -> Result<(), ConfigError> {
    // Reject traversal components regardless of whether the path is
    // absolute or relative. Absolute paths themselves are allowed; only
    // parent-directory climbs are rejected.
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(ConfigError::UnsafePath(
            "path must not contain '..' segments".into(),
        ));
    }
    // Normalize to PathBuf once to check OsStr validity.
    let _ = PathBuf::from(path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex_of(b: u8) -> String {
        let mut s = String::with_capacity(64);
        for _ in 0..32 {
            s.push_str(&format!("{:02x}", b));
        }
        s
    }

    fn baseline_toml() -> String {
        format!(
            r#"
version = 1
capability_tags = ["GPU", "CPU", "LLM", "ZK_DEV"]

[[nodes]]
id = "{req}"
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
            req = hex_of(0x11),
            p1 = hex_of(0x22),
            p2 = hex_of(0x33),
            p3 = hex_of(0x44),
        )
    }

    #[test]
    fn loads_baseline() {
        let cfg = TopologyConfig::from_toml_str(&baseline_toml()).unwrap();
        assert_eq!(cfg.nodes.len(), 4);
        assert_eq!(cfg.providers_stable_order().len(), 3);
    }

    #[test]
    fn rejects_duplicate_ids() {
        let dup = hex_of(0x55);
        let text = format!(
            r#"
version = 1
capability_tags = ["GPU"]

[[nodes]]
id = "{dup}"
role = "requester"
required_capability = "GPU"

[[nodes]]
id = "{dup}"
role = "provider"
capability_claims = ["GPU"]
"#
        );
        let err = TopologyConfig::from_toml_str(&text).unwrap_err();
        assert!(matches!(err, ConfigError::DuplicateNodeId(_)));
    }

    #[test]
    fn path_traversal_rejected() {
        let p = PathBuf::from("../etc/passwd");
        assert!(matches!(
            TopologyConfig::load(&p),
            Err(ConfigError::UnsafePath(_))
        ));
    }
}
