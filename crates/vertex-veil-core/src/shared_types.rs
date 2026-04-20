//! Shared public protocol types.
//!
//! These types are safe to serialize into a public coordination record. They
//! carry no private witness material. [`Secret`]-wrapped fields are
//! structurally prohibited here; the public artifact schema in
//! [`crate::artifacts`] is built from these types alone.
//!
//! [`Secret`]: crate::private_intent::Secret

use serde::{Deserialize, Serialize};

use crate::capability::CapabilityTag;
use crate::config::Role;
use crate::keys::NodeId;

/// Round counter. Monotonic, stable across the coordination log.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct RoundId(pub u64);

impl RoundId {
    pub const fn new(n: u64) -> Self {
        RoundId(n)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    pub const fn next(self) -> Self {
        RoundId(self.0 + 1)
    }
}

/// Public portion of a node's intent for a given round.
///
/// Requesters publish a coarse required capability. Providers publish their
/// capability claims. Neither side exposes price constraints.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum PublicIntent {
    Requester {
        node_id: NodeId,
        round: RoundId,
        required_capability: CapabilityTag,
    },
    Provider {
        node_id: NodeId,
        round: RoundId,
        capability_claims: Vec<CapabilityTag>,
    },
}

impl PublicIntent {
    pub fn node_id(&self) -> NodeId {
        match self {
            PublicIntent::Requester { node_id, .. } | PublicIntent::Provider { node_id, .. } => {
                *node_id
            }
        }
    }

    pub fn round(&self) -> RoundId {
        match self {
            PublicIntent::Requester { round, .. } | PublicIntent::Provider { round, .. } => *round,
        }
    }

    pub fn role(&self) -> Role {
        match self {
            PublicIntent::Requester { .. } => Role::Requester,
            PublicIntent::Provider { .. } => Role::Provider,
        }
    }
}

/// Snapshot of round state used by library-level logic. Phase 1 will expand
/// this with proposer rotation and fallback advancement; Phase 0 only needs
/// the canonical shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundState {
    pub round: RoundId,
    pub proposer: NodeId,
    pub finalized: bool,
}

impl RoundState {
    pub fn opening(round: RoundId, proposer: NodeId) -> Self {
        RoundState {
            round,
            proposer,
            finalized: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_node() -> NodeId {
        NodeId::from_bytes([0x11; 32])
    }

    #[test]
    fn round_id_roundtrips() {
        let json = serde_json::to_string(&RoundId::new(7)).unwrap();
        assert_eq!(json, "7");
        let back: RoundId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, RoundId::new(7));
    }

    #[test]
    fn public_intent_requester_roundtrips() {
        let cap = CapabilityTag::parse_shape("GPU").unwrap();
        let pi = PublicIntent::Requester {
            node_id: sample_node(),
            round: RoundId::new(0),
            required_capability: cap,
        };
        let json = serde_json::to_string(&pi).unwrap();
        let back: PublicIntent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, pi);
    }

    #[test]
    fn public_intent_provider_roundtrips() {
        let cap = CapabilityTag::parse_shape("GPU").unwrap();
        let pi = PublicIntent::Provider {
            node_id: sample_node(),
            round: RoundId::new(3),
            capability_claims: vec![cap],
        };
        let json = serde_json::to_string(&pi).unwrap();
        let back: PublicIntent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, pi);
    }
}
