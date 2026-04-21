//! Round-bound commitments to private intent.
//!
//! A commitment binds a node's private intent to a specific round without
//! revealing the private fields. The scheme is deterministic, domain-tagged,
//! round-bound, and uses a **fixed-size padded preimage** hashed with
//! **blake2s-256**. The fixed layout is critical: the Phase 2 Noir circuit
//! reproduces the same byte layout using constant indexing, and then hashes
//! the preimage with `std::hash::blake2s`.
//!
//! # Hash choice
//!
//! Noir stdlib v1.0.0-beta.20 exposes `sha256_compression` (the block
//! primitive) but does not provide a full `sha256` function. `blake2s` is
//! directly available as a foreign function. Using blake2s keeps circuits
//! small and parity simple; the `blake2` Rust crate produces byte-for-byte
//! identical output for the same input.
//!
//! # Byte Layout
//!
//! All integer widths are big-endian. Strings (domain, capability bytes)
//! are UTF-8. Variable-length content is zero-padded to its maximum size,
//! with the actual length carried as a preceding u32 length prefix. Role
//! byte is `0` for requester and `1` for provider.
//!
//! ## Requester preimage (`REQUESTER_PREIMAGE_LEN` = 153 bytes)
//!
//! ```text
//! [0..4)      u32 BE domain_len  (=31)
//! [4..35)     domain bytes: "vertex-veil/v1/commit-requester" (31 bytes)
//! [35]        u8 schema_version  (=1)
//! [36..44)    u64 BE round
//! [44..76)    [u8; 32] node_id
//! [76]        u8 role            (=0)
//! [77..81)    u32 BE cap_len
//! [81..113)   [u8; MAX_CAPABILITY_BYTES]  cap_bytes, zero-padded
//! [113..121)  u64 BE budget_cents
//! [121..153)  [u8; 32] nonce
//! ```
//!
//! ## Provider preimage (`PROVIDER_PREIMAGE_LEN` = 264 bytes)
//!
//! ```text
//! [0..4)      u32 BE domain_len  (=30)
//! [4..34)     domain bytes: "vertex-veil/v1/commit-provider" (30 bytes)
//! [34]        u8 schema_version  (=1)
//! [35..43)    u64 BE round
//! [43..75)    [u8; 32] node_id
//! [75]        u8 role            (=1)
//! [76..80)    u32 BE n_claims
//! [80..224)   claim slots: MAX_CAPABILITY_CLAIMS × (4 + MAX_CAPABILITY_BYTES) = 4×36 = 144 bytes
//!                 each slot: [0..4) u32 BE claim_len
//!                            [4..36) claim_bytes, zero-padded to MAX_CAPABILITY_BYTES
//!                 unused slots (index ≥ n_claims) are all zero bytes
//! [224..232)  u64 BE reservation_cents
//! [232..264)  [u8; 32] nonce
//! ```
//!
//! `INTENT.md`'s Open Decision "Exact commitment construction shared between
//! Rust and Noir" is resolved here; `/intent-sync` worth running when Phase 2
//! lands.

use blake2::{
    digest::{consts::U32, FixedOutput},
    Blake2s256, Digest,
};

use crate::keys::NodeId;
use crate::private_intent::{PrivateProviderIntent, PrivateRequesterIntent};
use crate::shared_types::RoundId;

/// Domain tag for requester commitments (31 bytes).
pub const COMMIT_DOMAIN_REQUESTER: &str = "vertex-veil/v1/commit-requester";

/// Domain tag for provider commitments (30 bytes).
pub const COMMIT_DOMAIN_PROVIDER: &str = "vertex-veil/v1/commit-provider";

/// Commitment layout schema version.
pub const COMMIT_SCHEMA_VERSION: u8 = 1;

/// Maximum capability-tag byte length. Every tag in a commitment is
/// zero-padded to this size.
pub const MAX_CAPABILITY_BYTES: usize = 32;

/// Maximum number of capability claims a provider can carry in a single
/// commitment. Extra claims are rejected at commit time.
pub const MAX_CAPABILITY_CLAIMS: usize = 4;

/// Fixed length of the requester commitment preimage.
pub const REQUESTER_PREIMAGE_LEN: usize = 153;

/// Fixed length of the provider commitment preimage.
pub const PROVIDER_PREIMAGE_LEN: usize = 264;

// Sanity: compile-time assertion the layout arithmetic matches the constants.
const _: () = {
    let expected_req = 4 + 31 + 1 + 8 + 32 + 1 + 4 + MAX_CAPABILITY_BYTES + 8 + 32;
    assert!(expected_req == REQUESTER_PREIMAGE_LEN);
    let expected_prov =
        4 + 30 + 1 + 8 + 32 + 1 + 4 + MAX_CAPABILITY_CLAIMS * (4 + MAX_CAPABILITY_BYTES) + 8 + 32;
    assert!(expected_prov == PROVIDER_PREIMAGE_LEN);
};

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
        let hex = self.to_hex();
        write!(f, "CommitmentBytes({}…)", &hex[..8.min(hex.len())])
    }
}

/// Errors produced by commitment helpers. Never echoes private witness values.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CommitmentError {
    #[error("commitment helper received invalid field: {0}")]
    InvalidField(&'static str),

    #[error("capability tag byte length {actual} exceeds MAX_CAPABILITY_BYTES={limit}")]
    CapabilityTooLong { actual: usize, limit: usize },

    #[error("provider claim count {actual} exceeds MAX_CAPABILITY_CLAIMS={limit}")]
    TooManyClaims { actual: usize, limit: usize },
}

/// Build the canonical 152-byte requester preimage.
pub fn build_requester_preimage(
    intent: &PrivateRequesterIntent,
    nonce: &[u8; 32],
    round: RoundId,
) -> Result<[u8; REQUESTER_PREIMAGE_LEN], CommitmentError> {
    let cap_bytes = intent.required_capability.as_str().as_bytes();
    if cap_bytes.is_empty() {
        return Err(CommitmentError::InvalidField("required_capability"));
    }
    if cap_bytes.len() > MAX_CAPABILITY_BYTES {
        return Err(CommitmentError::CapabilityTooLong {
            actual: cap_bytes.len(),
            limit: MAX_CAPABILITY_BYTES,
        });
    }

    let domain = COMMIT_DOMAIN_REQUESTER.as_bytes();
    debug_assert_eq!(domain.len(), 31);

    let mut buf = [0u8; REQUESTER_PREIMAGE_LEN];
    // Offsets match the module-level layout doc.
    buf[0..4].copy_from_slice(&(domain.len() as u32).to_be_bytes());
    buf[4..35].copy_from_slice(domain);
    buf[35] = COMMIT_SCHEMA_VERSION;
    buf[36..44].copy_from_slice(&round.value().to_be_bytes());
    buf[44..76].copy_from_slice(intent.node_id.as_bytes());
    buf[76] = 0; // role = requester
    buf[77..81].copy_from_slice(&(cap_bytes.len() as u32).to_be_bytes());
    buf[81..81 + cap_bytes.len()].copy_from_slice(cap_bytes);
    // Remainder of cap slot (81 + cap_len .. 113) stays zero from init.
    buf[113..121].copy_from_slice(&intent.budget_cents.expose().to_be_bytes());
    buf[121..153].copy_from_slice(nonce);
    Ok(buf)
}

/// Build the canonical 263-byte provider preimage.
pub fn build_provider_preimage(
    intent: &PrivateProviderIntent,
    nonce: &[u8; 32],
    round: RoundId,
) -> Result<[u8; PROVIDER_PREIMAGE_LEN], CommitmentError> {
    if intent.capability_claims.is_empty() {
        return Err(CommitmentError::InvalidField("capability_claims"));
    }
    if intent.capability_claims.len() > MAX_CAPABILITY_CLAIMS {
        return Err(CommitmentError::TooManyClaims {
            actual: intent.capability_claims.len(),
            limit: MAX_CAPABILITY_CLAIMS,
        });
    }
    for claim in &intent.capability_claims {
        let bytes = claim.as_str().as_bytes();
        if bytes.is_empty() {
            return Err(CommitmentError::InvalidField("capability_claim"));
        }
        if bytes.len() > MAX_CAPABILITY_BYTES {
            return Err(CommitmentError::CapabilityTooLong {
                actual: bytes.len(),
                limit: MAX_CAPABILITY_BYTES,
            });
        }
    }

    let domain = COMMIT_DOMAIN_PROVIDER.as_bytes();
    debug_assert_eq!(domain.len(), 30);

    let mut buf = [0u8; PROVIDER_PREIMAGE_LEN];
    buf[0..4].copy_from_slice(&(domain.len() as u32).to_be_bytes());
    buf[4..34].copy_from_slice(domain);
    buf[34] = COMMIT_SCHEMA_VERSION;
    buf[35..43].copy_from_slice(&round.value().to_be_bytes());
    buf[43..75].copy_from_slice(intent.node_id.as_bytes());
    buf[75] = 1; // role = provider

    let n_claims = intent.capability_claims.len();
    buf[76..80].copy_from_slice(&(n_claims as u32).to_be_bytes());

    // Claim slots start at offset 80 and occupy MAX_CAPABILITY_CLAIMS slots
    // of (4 + MAX_CAPABILITY_BYTES) bytes each.
    let slots_start = 80;
    for (idx, claim) in intent.capability_claims.iter().enumerate() {
        let slot = slots_start + idx * (4 + MAX_CAPABILITY_BYTES);
        let bytes = claim.as_str().as_bytes();
        buf[slot..slot + 4].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
        buf[slot + 4..slot + 4 + bytes.len()].copy_from_slice(bytes);
        // Remainder of slot stays zero.
    }
    // Unused slots (idx >= n_claims) remain all-zero.

    let claims_end = slots_start + MAX_CAPABILITY_CLAIMS * (4 + MAX_CAPABILITY_BYTES);
    debug_assert_eq!(claims_end, 224);
    buf[claims_end..claims_end + 8].copy_from_slice(&intent.reservation_cents.expose().to_be_bytes());
    buf[claims_end + 8..claims_end + 8 + 32].copy_from_slice(nonce);
    Ok(buf)
}

/// Hash a requester preimage with blake2s-256.
pub fn hash_preimage_requester(preimage: &[u8; REQUESTER_PREIMAGE_LEN]) -> CommitmentBytes {
    let mut hasher = Blake2s256::new();
    hasher.update(preimage);
    let mut out = [0u8; 32];
    let digest = hasher.finalize_fixed();
    out.copy_from_slice(digest.as_slice());
    CommitmentBytes(out)
}

/// Hash a provider preimage with blake2s-256.
pub fn hash_preimage_provider(preimage: &[u8; PROVIDER_PREIMAGE_LEN]) -> CommitmentBytes {
    let mut hasher = Blake2s256::new();
    hasher.update(preimage);
    let mut out = [0u8; 32];
    let digest = hasher.finalize_fixed();
    out.copy_from_slice(digest.as_slice());
    CommitmentBytes(out)
}

/// Build a requester commitment from private intent, per-round nonce, and
/// round id.
pub fn commit_requester(
    intent: &PrivateRequesterIntent,
    nonce: &[u8; 32],
    round: RoundId,
) -> Result<CommitmentBytes, CommitmentError> {
    let preimage = build_requester_preimage(intent, nonce, round)?;
    Ok(hash_preimage_requester(&preimage))
}

/// Build a provider commitment from private intent, per-round nonce, and
/// round id.
pub fn commit_provider(
    intent: &PrivateProviderIntent,
    nonce: &[u8; 32],
    round: RoundId,
) -> Result<CommitmentBytes, CommitmentError> {
    let preimage = build_provider_preimage(intent, nonce, round)?;
    Ok(hash_preimage_provider(&preimage))
}

/// Convenience: derive a deterministic 32-byte nonce from a [`NodeId`], a
/// [`RoundId`], and a caller-supplied salt. Uses blake2s for consistency
/// with the commitment hash.
pub fn derive_test_nonce(node: NodeId, round: RoundId, salt: &[u8]) -> [u8; 32] {
    let mut hasher = Blake2s256::new();
    hasher.update(b"vertex-veil/v1/test-nonce");
    hasher.update(node.as_bytes());
    hasher.update(round.value().to_be_bytes());
    hasher.update(salt);
    let digest = hasher.finalize_fixed();
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_slice());
    out
}

// Keep the compile-time digest parameter explicit to catch accidental alg changes.
#[allow(dead_code)]
fn _assert_blake2s_output_size() {
    fn assert_u32<T: FixedOutput<OutputSize = U32>>(_: &T) {}
    let h = Blake2s256::new();
    assert_u32(&h);
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
    fn requester_preimage_has_expected_length() {
        let nonce = [0xa1; 32];
        let preimage = build_requester_preimage(&req(100), &nonce, RoundId::new(0)).unwrap();
        assert_eq!(preimage.len(), REQUESTER_PREIMAGE_LEN);
    }

    #[test]
    fn provider_preimage_has_expected_length() {
        let nonce = [0xa1; 32];
        let preimage = build_provider_preimage(&prov(100), &nonce, RoundId::new(0)).unwrap();
        assert_eq!(preimage.len(), PROVIDER_PREIMAGE_LEN);
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
    fn provider_commit_differs_across_rounds() {
        let nonce = [0xb2; 32];
        let a = commit_provider(&prov(50), &nonce, RoundId::new(0)).unwrap();
        let b = commit_provider(&prov(50), &nonce, RoundId::new(1)).unwrap();
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
    fn provider_rejects_too_many_claims() {
        let mut intent = prov(50);
        intent.capability_claims = (0..(MAX_CAPABILITY_CLAIMS + 1))
            .map(|i| CapabilityTag::parse_shape(&format!("TAG_{i}")).unwrap())
            .collect();
        let err = commit_provider(&intent, &[0; 32], RoundId::new(0)).unwrap_err();
        assert!(matches!(err, CommitmentError::TooManyClaims { .. }));
    }

    #[test]
    fn provider_errors_do_not_echo_private_reservation() {
        let intent = PrivateProviderIntent {
            node_id: NodeId::from_bytes([0x22; 32]),
            capability_claims: Vec::new(),
            reservation_cents: Secret::new(777_007u64),
        };
        let err = commit_provider(&intent, &[0; 32], RoundId::new(0)).unwrap_err();
        let msg = format!("{err}");
        assert!(!msg.contains("777_007"));
        assert!(!msg.contains("777007"));
    }
}
