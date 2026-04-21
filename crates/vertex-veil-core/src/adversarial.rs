//! Adversarial scenarios for the Phase 3 coordination runtime.
//!
//! A [`Scenario`] is a short, runtime-configurable list of adversarial events
//! the scenario injector should attempt during a coordination run. The
//! runtime is responsible for applying the events and either rejecting them
//! with a logged [`crate::RejectionRecord`] or advancing to a fallback round,
//! whichever the protocol requires.
//!
//! Scenario events are public by construction — they reference stable public
//! keys and round numbers only, never private witness values. The scenario
//! fixture file is therefore safe to ship as part of a demo artifact bundle.
//!
//! # File Format
//!
//! Scenario files are TOML with shape:
//!
//! ```toml
//! version = 1
//!
//! [[events]]
//! kind = "double_commit"
//! node = "2222...64 hex..."
//! round = 0
//!
//! [[events]]
//! kind = "replay_prior_commitment"
//! node = "3333...64 hex..."
//! round = 1
//! from_round = 0
//!
//! [[events]]
//! kind = "drop_node"
//! node = "4444...64 hex..."
//! after_round = 0
//!
//! [[events]]
//! kind = "inject_invalid_proof"
//! node = "2222...64 hex..."
//! round = 0
//! ```
//!
//! Paths are validated the same way as topology fixtures: parent-directory
//! traversal is rejected.

use std::fs;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;
use crate::keys::NodeId;

/// Public-only adversarial event.
///
/// Tagged by `kind` for readable TOML. The `node` field always parses as a
/// [`NodeId`] after load; round numbers are `u64` to align with [`crate::RoundId`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScenarioEvent {
    /// The named node attempts to commit twice in the given round.
    DoubleCommit { node: NodeId, round: u64 },
    /// The named node republishes a commitment that was already accepted in
    /// an earlier `from_round`, labelling it with the later `round`.
    ReplayPriorCommitment {
        node: NodeId,
        round: u64,
        from_round: u64,
    },
    /// The named node stops participating starting the round strictly after
    /// `after_round`. Commitments from the node are not broadcast; the
    /// runtime's fallback handling must still reach a valid or verifiably
    /// aborted state.
    DropNode { node: NodeId, after_round: u64 },
    /// The named node publishes a proof whose public inputs intentionally
    /// mismatch the logged commitment for `(node, round)`. The runtime must
    /// reject the proof visibly.
    InjectInvalidProof { node: NodeId, round: u64 },
}

impl ScenarioEvent {
    /// Return the node this event targets.
    pub fn node(&self) -> NodeId {
        match self {
            ScenarioEvent::DoubleCommit { node, .. }
            | ScenarioEvent::ReplayPriorCommitment { node, .. }
            | ScenarioEvent::DropNode { node, .. }
            | ScenarioEvent::InjectInvalidProof { node, .. } => *node,
        }
    }

    /// Short label used by verifier reports and rejection records.
    pub fn kind_label(&self) -> &'static str {
        match self {
            ScenarioEvent::DoubleCommit { .. } => "double_commit",
            ScenarioEvent::ReplayPriorCommitment { .. } => "replay_prior_commitment",
            ScenarioEvent::DropNode { .. } => "drop_node",
            ScenarioEvent::InjectInvalidProof { .. } => "inject_invalid_proof",
        }
    }
}

/// Ordered list of adversarial events to apply during a run.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scenario {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub events: Vec<ScenarioEvent>,
}

fn default_version() -> u32 {
    1
}

impl Scenario {
    /// An empty scenario. The runtime runs a clean baseline when given this.
    pub fn empty() -> Self {
        Scenario {
            version: 1,
            events: Vec::new(),
        }
    }

    /// Parse a scenario from a TOML string.
    pub fn from_toml_str(text: &str) -> Result<Self, ConfigError> {
        let scenario: Scenario = toml::from_str(text).map_err(ConfigError::parse)?;
        if scenario.version != 1 {
            return Err(ConfigError::InvalidTopology(format!(
                "unsupported scenario version {}",
                scenario.version
            )));
        }
        Ok(scenario)
    }

    /// Load a scenario from a TOML file. Rejects paths containing `..`.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        if path.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(ConfigError::UnsafePath(
                "scenario path must not contain '..' segments".into(),
            ));
        }
        let text = fs::read_to_string(path).map_err(ConfigError::io)?;
        Self::from_toml_str(&text)
    }

    /// Events that target this node, by round (for [`ScenarioEvent::DoubleCommit`],
    /// [`ScenarioEvent::ReplayPriorCommitment`], [`ScenarioEvent::InjectInvalidProof`]).
    pub fn events_for_round(&self, node: NodeId, round: u64) -> Vec<&ScenarioEvent> {
        self.events
            .iter()
            .filter(|e| match e {
                ScenarioEvent::DoubleCommit { node: n, round: r }
                | ScenarioEvent::ReplayPriorCommitment {
                    node: n, round: r, ..
                }
                | ScenarioEvent::InjectInvalidProof { node: n, round: r } => *n == node && *r == round,
                ScenarioEvent::DropNode { .. } => false,
            })
            .collect()
    }

    /// Returns `true` when the node has dropped by the given round.
    pub fn has_dropped(&self, node: NodeId, round: u64) -> bool {
        self.events.iter().any(|e| match e {
            ScenarioEvent::DropNode {
                node: n,
                after_round,
            } => *n == node && round > *after_round,
            _ => false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(b: u8) -> NodeId {
        NodeId::from_bytes([b; 32])
    }

    fn sample_toml() -> String {
        format!(
            r#"
version = 1

[[events]]
kind = "double_commit"
node = "{}"
round = 0

[[events]]
kind = "replay_prior_commitment"
node = "{}"
round = 1
from_round = 0

[[events]]
kind = "drop_node"
node = "{}"
after_round = 0

[[events]]
kind = "inject_invalid_proof"
node = "{}"
round = 0
"#,
            node(0x22),
            node(0x33),
            node(0x44),
            node(0x22),
        )
    }

    #[test]
    fn parses_sample() {
        let s = Scenario::from_toml_str(&sample_toml()).unwrap();
        assert_eq!(s.events.len(), 4);
    }

    #[test]
    fn empty_scenario_accepted() {
        let s = Scenario::from_toml_str("version = 1\n").unwrap();
        assert!(s.events.is_empty());
    }

    #[test]
    fn rejects_unsupported_version() {
        let err = Scenario::from_toml_str("version = 2\n").unwrap_err();
        assert!(matches!(err, ConfigError::InvalidTopology(_)));
    }

    #[test]
    fn events_for_round_matches_per_node() {
        let s = Scenario::from_toml_str(&sample_toml()).unwrap();
        let e0 = s.events_for_round(node(0x22), 0);
        assert_eq!(e0.len(), 2); // double_commit + inject_invalid_proof
        let e1 = s.events_for_round(node(0x33), 1);
        assert_eq!(e1.len(), 1);
    }

    #[test]
    fn has_dropped_respects_after_round() {
        let s = Scenario::from_toml_str(&sample_toml()).unwrap();
        assert!(!s.has_dropped(node(0x44), 0));
        assert!(s.has_dropped(node(0x44), 1));
    }

    #[test]
    fn load_rejects_parent_traversal() {
        let err = Scenario::load("../escape.toml").unwrap_err();
        assert!(matches!(err, ConfigError::UnsafePath(_)));
    }
}
