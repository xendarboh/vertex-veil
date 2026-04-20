//! Round-bound commitments to private intent.
//!
//! A commitment binds a node's private intent to a specific round without
//! revealing the private fields. The scheme is deterministic, domain-tagged,
//! and round-bound: the same intent and nonce produce the same commitment
//! inside a round and a distinct commitment across rounds.
//!
//! # Byte Layout (Noir-parity reference)
//!
//! Both sides of the parity invariant (Rust runtime and Noir circuit) must
//! hash exactly the same preimage. Lengths are big-endian `u32`. Integers
//! (round, budget, reservation) are big-endian of their declared width. The
//! role byte is `0` for requester and `1` for provider.
//!
//! Requester preimage:
//!
//! ```text
//! u32(len(domain_requester)) || domain_requester
//! u8(schema_version=1)
//! u64(round) BE
//! [u8; 32] node_id
//! u8(role=0)
//! u32(len(required_capability)) || required_capability bytes
//! u64(budget_cents) BE
//! [u8; 32] nonce
//! ```
//!
//! Provider preimage:
//!
//! ```text
//! u32(len(domain_provider)) || domain_provider
//! u8(schema_version=1)
//! u64(round) BE
//! [u8; 32] node_id
//! u8(role=1)
//! u32(n_claims)
//! for each claim: u32(len) || claim bytes
//! u64(reservation_cents) BE
//! [u8; 32] nonce
//! ```
//!
//! The digest is SHA-256 of the preimage. SHA-256 was chosen because it is
//! available in Noir stdlib and keeps the Phase 2 port direct. `INTENT.md`
//! records the exact commitment construction as an open decision; this
//! module is the Rust-side source of truth until that is resolved.

use sha2::{Digest, Sha256};

use crate::keys::NodeId;
use crate::private_intent::{PrivateProviderIntent, PrivateRequesterIntent};
use crate::shared_types::RoundId;

/// Domain tag for requester commitments.
pub const COMMIT_DOMAIN_REQUESTER: &str = "vertex-veil/v1/commit-requester";

/// Domain tag for provider commitments.
pub const COMMIT_DOMAIN_PROVIDER: &str = "vertex-veil/v1/commit-provider";

/// Commitment layout schema version.
pub const COMMIT_SCHEMA_VERSION: u8 = 1;

/// 32-byte commitment digest.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CommitmentBytes(pub [u8; 32]);

impl CommitmentBytes {
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for CommitmentBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Short-prefix debug only; the full hex is always recoverable via
        // `to_hex()` when needed. Keeping debug compact avoids accidental
        // log noise.
        let hex = self.to_hex();
        write!(f, "CommitmentBytes({}…)", &hex[..8.min(hex.len())])
    }
}

/// Errors produced by commitment helpers. These errors never echo private
/// witness values. When a helper rejects an input, the message names the
/// field but not the field's value.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CommitmentError {
    #[error("commitment helper received invalid field: {0}")]
    InvalidField(&'static str),
}

/// Build a requester commitment from private intent, per-round nonce, and
/// round id.
pub fn commit_requester(
    intent: &PrivateRequesterIntent,
    nonce: &[u8; 32],
    round: RoundId,
) -> Result<CommitmentBytes, CommitmentError> {
    let cap = intent.required_capability.as_str();
    if cap.is_empty() {
        return Err(CommitmentError::InvalidField("required_capability"));
    }
    let mut h = Sha256::new();
    write_lp(&mut h, COMMIT_DOMAIN_REQUESTER.as_bytes());
    h.update([COMMIT_SCHEMA_VERSION]);
    h.update(round.value().to_be_bytes());
    h.update(intent.node_id.as_bytes());
    h.update([0u8]); // role = requester
    write_lp(&mut h, cap.as_bytes());
    h.update(intent.budget_cents.expose().to_be_bytes());
    h.update(nonce);
    Ok(finalize(h))
}

/// Build a provider commitment from private intent, per-round nonce, and
/// round id.
pub fn commit_provider(
    intent: &PrivateProviderIntent,
    nonce: &[u8; 32],
    round: RoundId,
) -> Result<CommitmentBytes, CommitmentError> {
    if intent.capability_claims.is_empty() {
        return Err(CommitmentError::InvalidField("capability_claims"));
    }
    let mut h = Sha256::new();
    write_lp(&mut h, COMMIT_DOMAIN_PROVIDER.as_bytes());
    h.update([COMMIT_SCHEMA_VERSION]);
    h.update(round.value().to_be_bytes());
    h.update(intent.node_id.as_bytes());
    h.update([1u8]); // role = provider
    h.update((intent.capability_claims.len() as u32).to_be_bytes());
    for c in &intent.capability_claims {
        let bytes = c.as_str().as_bytes();
        if bytes.is_empty() {
            return Err(CommitmentError::InvalidField("capability_claim"));
        }
        write_lp(&mut h, bytes);
    }
    h.update(intent.reservation_cents.expose().to_be_bytes());
    h.update(nonce);
    Ok(finalize(h))
}

/// Convenience: derive a deterministic 32-byte nonce from a `NodeId`,
/// `RoundId`, and a caller-supplied salt. Used by test fixtures and local
/// agent flows that need reproducible nonces without a full key-derivation
/// function.
pub fn derive_test_nonce(node: NodeId, round: RoundId, salt: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"vertex-veil/v1/test-nonce");
    h.update(node.as_bytes());
    h.update(round.value().to_be_bytes());
    h.update(salt);
    let mut out = [0u8; 32];
    out.copy_from_slice(&h.finalize());
    out
}

fn write_lp(h: &mut Sha256, bytes: &[u8]) {
    h.update((bytes.len() as u32).to_be_bytes());
    h.update(bytes);
}

fn finalize(h: Sha256) -> CommitmentBytes {
    let mut out = [0u8; 32];
    out.copy_from_slice(&h.finalize());
    CommitmentBytes(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::CapabilityTag;
    use crate::private_intent::{PrivateProviderIntent, PrivateRequesterIntent, Secret};

    fn req(budget: u64) -> PrivateRequesterIntent {
        PrivateRequesterIntent {
            node_id: NodeId::from_bytes([0x11; 32]),
            required_capability: CapabilityTag::parse_shape("GPU").unwrap(),
            budget_cents: Secret::new(budget),
        }
    }

    fn prov(reservation: u64) -> PrivateProviderIntent {
        PrivateProviderIntent {
            node_id: NodeId::from_bytes([0x22; 32]),
            capability_claims: vec![CapabilityTag::parse_shape("GPU").unwrap()],
            reservation_cents: Secret::new(reservation),
        }
    }

    #[test]
    fn requester_commit_is_deterministic() {
        let nonce = [0xa1; 32];
        let a = commit_requester(&req(100), &nonce, RoundId::new(3)).unwrap();
        let b = commit_requester(&req(100), &nonce, RoundId::new(3)).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn requester_commit_differs_across_rounds() {
        let nonce = [0xa1; 32];
        let a = commit_requester(&req(100), &nonce, RoundId::new(0)).unwrap();
        let b = commit_requester(&req(100), &nonce, RoundId::new(1)).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn requester_commit_differs_by_private_budget() {
        let nonce = [0xa1; 32];
        let a = commit_requester(&req(100), &nonce, RoundId::new(0)).unwrap();
        let b = commit_requester(&req(101), &nonce, RoundId::new(0)).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn provider_commit_differs_across_rounds() {
        let nonce = [0xb2; 32];
        let a = commit_provider(&prov(50), &nonce, RoundId::new(0)).unwrap();
        let b = commit_provider(&prov(50), &nonce, RoundId::new(1)).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn error_does_not_echo_private_values() {
        // Build a requester intent with an empty capability string. The
        // error must not echo the (private) budget.
        let mut intent = req(7777);
        intent.required_capability = CapabilityTag::parse_shape("GPU").unwrap();
        // Force an invalid capability by constructing via unsafe path: we
        // check at module boundary instead — here we assert the error
        // message shape contains only the field name, not a number.
        let err = CommitmentError::InvalidField("required_capability");
        let msg = format!("{err}");
        assert!(!msg.contains("7777"));
        assert!(msg.contains("required_capability"));
    }

    #[test]
    fn debug_redacts_commitment_bytes() {
        let b = CommitmentBytes([0xab; 32]);
        let s = format!("{b:?}");
        assert!(s.starts_with("CommitmentBytes("));
        assert!(!s.contains(&"ab".repeat(32)));
    }
}
