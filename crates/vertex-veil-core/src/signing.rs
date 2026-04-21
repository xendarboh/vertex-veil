//! Ed25519 signing helpers for completion receipts.
//!
//! Phase 4 replaces the Phase 3 blake2s synthetic signature with real
//! ed25519. The verifier recomputes the deterministic canonical message and
//! checks the signature against the provider's configured
//! `signing_public_key` from the topology.
//!
//! When a topology or private-intent fixture predates Phase 4 and has no
//! signing keys, the runtime falls back to the legacy blake2s tag and the
//! verifier accepts it. This is a back-compat path for Phase 3 fixtures and
//! tests only; Phase 4 fixtures are required to carry ed25519 keys.
//!
//! The canonical signed payload is:
//!
//! ```text
//! "vertex-veil/v1/completion-receipt"
//!   || provider_node_id (32B)
//!   || round_u64_be   (8B)
//!   || capability_tag_bytes (variable, length-prefixed)
//! ```
//!
//! This is deterministic in the public record, so any third-party verifier
//! can recompute it without access to the signer's private key.

use blake2::{digest::FixedOutput, Blake2s256, Digest};
use ed25519_dalek::{
    Signature, Signer, SigningKey, Verifier, VerifyingKey, PUBLIC_KEY_LENGTH, SECRET_KEY_LENGTH,
    SIGNATURE_LENGTH,
};

use crate::keys::NodeId;
use crate::shared_types::RoundId;

/// Length of a serialized ed25519 signing seed in bytes.
pub const ED25519_SECRET_LEN: usize = SECRET_KEY_LENGTH;
/// Length of a serialized ed25519 public key in bytes.
pub const ED25519_PUBLIC_LEN: usize = PUBLIC_KEY_LENGTH;
/// Length of a serialized ed25519 signature in bytes.
pub const ED25519_SIG_LEN: usize = SIGNATURE_LENGTH;

/// Hex-encoded signing public key stored in [`crate::config::NodeConfig`].
///
/// The wrapper exists so the serialization form and hex validation stay in
/// one place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SigningPublicKey([u8; ED25519_PUBLIC_LEN]);

impl SigningPublicKey {
    pub const fn from_bytes(bytes: [u8; ED25519_PUBLIC_LEN]) -> Self {
        SigningPublicKey(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; ED25519_PUBLIC_LEN] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn verifying_key(&self) -> Result<VerifyingKey, SignError> {
        VerifyingKey::from_bytes(&self.0).map_err(|_| SignError::MalformedPublicKey)
    }

    pub fn from_hex(s: &str) -> Result<Self, SignError> {
        if s.len() != ED25519_PUBLIC_LEN * 2 {
            return Err(SignError::MalformedPublicKey);
        }
        if !s.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
            return Err(SignError::MalformedPublicKey);
        }
        let mut out = [0u8; ED25519_PUBLIC_LEN];
        hex::decode_to_slice(s, &mut out).map_err(|_| SignError::MalformedPublicKey)?;
        // Validate the point lives on the curve.
        VerifyingKey::from_bytes(&out).map_err(|_| SignError::MalformedPublicKey)?;
        Ok(SigningPublicKey(out))
    }
}

/// A 32-byte ed25519 signing seed. Stored inside
/// [`crate::private_intent::Secret`] for redaction.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SigningSecretSeed([u8; ED25519_SECRET_LEN]);

impl SigningSecretSeed {
    pub const fn from_bytes(bytes: [u8; ED25519_SECRET_LEN]) -> Self {
        SigningSecretSeed(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; ED25519_SECRET_LEN] {
        &self.0
    }

    pub fn signing_key(&self) -> SigningKey {
        SigningKey::from_bytes(&self.0)
    }

    pub fn public(&self) -> SigningPublicKey {
        SigningPublicKey::from_bytes(self.signing_key().verifying_key().to_bytes())
    }

    pub fn from_hex(s: &str) -> Result<Self, SignError> {
        if s.len() != ED25519_SECRET_LEN * 2 {
            return Err(SignError::MalformedSecretKey);
        }
        if !s.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
            return Err(SignError::MalformedSecretKey);
        }
        let mut out = [0u8; ED25519_SECRET_LEN];
        hex::decode_to_slice(s, &mut out).map_err(|_| SignError::MalformedSecretKey)?;
        Ok(SigningSecretSeed(out))
    }
}

impl std::fmt::Debug for SigningSecretSeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SigningSecretSeed([REDACTED])")
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SignError {
    #[error("malformed ed25519 public key")]
    MalformedPublicKey,
    #[error("malformed ed25519 secret seed")]
    MalformedSecretKey,
    #[error("signature bytes are not valid hex or wrong length")]
    MalformedSignature,
    #[error("ed25519 signature verification failed")]
    BadSignature,
}

/// Build the canonical receipt payload (public, reproducible).
pub fn receipt_message(provider: NodeId, round: RoundId, capability: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(
        b"vertex-veil/v1/completion-receipt".len() + 32 + 8 + 4 + capability.len(),
    );
    buf.extend_from_slice(b"vertex-veil/v1/completion-receipt");
    buf.extend_from_slice(provider.as_bytes());
    buf.extend_from_slice(&round.value().to_be_bytes());
    let cap = capability.as_bytes();
    buf.extend_from_slice(&(cap.len() as u32).to_be_bytes());
    buf.extend_from_slice(cap);
    buf
}

/// Ed25519-sign the canonical receipt message with the given seed.
pub fn sign_receipt_ed25519(
    seed: &SigningSecretSeed,
    provider: NodeId,
    round: RoundId,
    capability: &str,
) -> [u8; ED25519_SIG_LEN] {
    let sk = seed.signing_key();
    let msg = receipt_message(provider, round, capability);
    sk.sign(&msg).to_bytes()
}

/// Verify an ed25519 signature on the canonical receipt message.
pub fn verify_receipt_ed25519(
    public: &SigningPublicKey,
    signature_hex: &str,
    provider: NodeId,
    round: RoundId,
    capability: &str,
) -> Result<(), SignError> {
    let raw = hex::decode(signature_hex).map_err(|_| SignError::MalformedSignature)?;
    if raw.len() != ED25519_SIG_LEN {
        return Err(SignError::MalformedSignature);
    }
    let mut arr = [0u8; ED25519_SIG_LEN];
    arr.copy_from_slice(&raw);
    let sig = Signature::from_bytes(&arr);
    let vk = public.verifying_key()?;
    let msg = receipt_message(provider, round, capability);
    vk.verify(&msg, &sig).map_err(|_| SignError::BadSignature)
}

/// Legacy blake2s-256 deterministic "signature" used by Phase 3 fixtures
/// that lack ed25519 keys. A real verifier recomputes the same tag.
pub fn legacy_signature(provider: NodeId, round: RoundId, capability: &str) -> [u8; 32] {
    let mut h = Blake2s256::new();
    h.update(b"vertex-veil/v1/completion-receipt");
    h.update(provider.as_bytes());
    h.update(round.value().to_be_bytes());
    h.update(capability.as_bytes());
    let d = h.finalize_fixed();
    let mut out = [0u8; 32];
    out.copy_from_slice(d.as_slice());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_node() -> NodeId {
        NodeId::from_bytes([0x42; 32])
    }

    #[test]
    fn ed25519_roundtrip() {
        let seed = SigningSecretSeed::from_bytes([7u8; 32]);
        let pub_hex = seed.public().to_hex();
        let pk = SigningPublicKey::from_hex(&pub_hex).unwrap();
        let sig = sign_receipt_ed25519(&seed, sample_node(), RoundId::new(3), "GPU");
        let sig_hex = hex::encode(sig);
        verify_receipt_ed25519(&pk, &sig_hex, sample_node(), RoundId::new(3), "GPU").unwrap();
    }

    #[test]
    fn ed25519_detects_tamper() {
        let seed = SigningSecretSeed::from_bytes([7u8; 32]);
        let pk_hex = seed.public().to_hex();
        let pk = SigningPublicKey::from_hex(&pk_hex).unwrap();
        let sig = sign_receipt_ed25519(&seed, sample_node(), RoundId::new(3), "GPU");
        let sig_hex = hex::encode(sig);
        assert!(matches!(
            verify_receipt_ed25519(&pk, &sig_hex, sample_node(), RoundId::new(9), "GPU"),
            Err(SignError::BadSignature)
        ));
        assert!(matches!(
            verify_receipt_ed25519(&pk, &sig_hex, sample_node(), RoundId::new(3), "CPU"),
            Err(SignError::BadSignature)
        ));
    }

    #[test]
    fn public_key_hex_rejects_malformed_input() {
        // Wrong length.
        assert!(SigningPublicKey::from_hex("abcd").is_err());
        // Non-hex characters.
        let mut bad = "g".repeat(64);
        assert!(SigningPublicKey::from_hex(&bad).is_err());
        // Uppercase is explicitly rejected by our canonical form.
        bad = "A".repeat(64);
        assert!(SigningPublicKey::from_hex(&bad).is_err());
    }

    #[test]
    fn secret_debug_redacts() {
        let s = SigningSecretSeed::from_bytes([1u8; 32]);
        let d = format!("{:?}", s);
        assert!(d.contains("[REDACTED]"));
    }

    #[test]
    fn legacy_signature_is_deterministic() {
        let a = legacy_signature(sample_node(), RoundId::new(1), "GPU");
        let b = legacy_signature(sample_node(), RoundId::new(1), "GPU");
        assert_eq!(a, b);
    }
}
