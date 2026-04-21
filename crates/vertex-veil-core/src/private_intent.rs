//! Private intent types with built-in redaction.
//!
//! Private witness values (requester budget, provider reservation price)
//! never appear in public coordination artifacts. To make that guarantee
//! difficult to violate accidentally, every private field is wrapped in
//! [`Secret<T>`], which:
//!
//! - displays as `[REDACTED]` in `Debug`
//! - serializes as the literal string `"[REDACTED]"`
//! - requires an explicit `expose()` call to access the inner value
//!
//! The public coordination schema in [`crate::artifacts`] deliberately
//! contains no [`Secret`] field. Wrapping private data in [`Secret`] is
//! therefore the last-line defense; field-level segregation is still the
//! primary guarantee.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::capability::CapabilityTag;
use crate::keys::NodeId;
use crate::signing::SigningSecretSeed;

/// Wrapper that redacts its inner value in `Debug` and default serialization.
///
/// Use [`Secret::new`] to wrap a private value, [`Secret::expose`] to retrieve
/// it for a circuit witness or a local check.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret<T>(T);

impl<T> Secret<T> {
    pub fn new(value: T) -> Self {
        Secret(value)
    }

    /// Expose the inner value. Callers should only do this for witness
    /// generation or local predicate checks, never for public records.
    pub fn expose(&self) -> &T {
        &self.0
    }

    /// Consume the wrapper and return the inner value. Same audit rules as
    /// [`expose`](Self::expose).
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([REDACTED])")
    }
}

impl<T> Serialize for Secret<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("[REDACTED]")
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Secret<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Secret)
    }
}

/// Requester-side private intent. Lives in fixtures and local agent state;
/// never appears in a public artifact.
#[derive(Clone)]
pub struct PrivateRequesterIntent {
    pub node_id: NodeId,
    pub required_capability: CapabilityTag,
    pub budget_cents: Secret<u64>,
    /// Optional ed25519 signing seed. Phase 4 populates this from the
    /// private-intent fixture; Phase 3 fixtures without the field fall back
    /// to the legacy blake2s tag path.
    pub signing_secret_key: Option<Secret<SigningSecretSeed>>,
}

impl PrivateRequesterIntent {
    /// Build a requester intent without an ed25519 signing seed. The runtime
    /// falls back to the legacy blake2s tag for receipt signing.
    pub fn new(
        node_id: NodeId,
        required_capability: CapabilityTag,
        budget_cents: u64,
    ) -> Self {
        PrivateRequesterIntent {
            node_id,
            required_capability,
            budget_cents: Secret::new(budget_cents),
            signing_secret_key: None,
        }
    }

    /// Build a requester intent with an ed25519 signing seed.
    pub fn with_signing(mut self, seed: SigningSecretSeed) -> Self {
        self.signing_secret_key = Some(Secret::new(seed));
        self
    }
}

impl fmt::Debug for PrivateRequesterIntent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrivateRequesterIntent")
            .field("node_id", &self.node_id)
            .field("required_capability", &self.required_capability)
            .field("budget_cents", &self.budget_cents)
            .field("signing_secret_key", &self.signing_secret_key)
            .finish()
    }
}

/// Provider-side private intent. Lives in fixtures and local agent state;
/// never appears in a public artifact.
#[derive(Clone)]
pub struct PrivateProviderIntent {
    pub node_id: NodeId,
    pub capability_claims: Vec<CapabilityTag>,
    pub reservation_cents: Secret<u64>,
    /// Optional ed25519 signing seed. See [`PrivateRequesterIntent`].
    pub signing_secret_key: Option<Secret<SigningSecretSeed>>,
}

impl PrivateProviderIntent {
    /// Build a provider intent without an ed25519 signing seed. The runtime
    /// falls back to the legacy blake2s tag for receipt signing.
    pub fn new(
        node_id: NodeId,
        capability_claims: Vec<CapabilityTag>,
        reservation_cents: u64,
    ) -> Self {
        PrivateProviderIntent {
            node_id,
            capability_claims,
            reservation_cents: Secret::new(reservation_cents),
            signing_secret_key: None,
        }
    }

    /// Build a provider intent with an ed25519 signing seed.
    pub fn with_signing(mut self, seed: SigningSecretSeed) -> Self {
        self.signing_secret_key = Some(Secret::new(seed));
        self
    }
}

impl fmt::Debug for PrivateProviderIntent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrivateProviderIntent")
            .field("node_id", &self.node_id)
            .field("capability_claims", &self.capability_claims)
            .field("reservation_cents", &self.reservation_cents)
            .field("signing_secret_key", &self.signing_secret_key)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_debug_is_redacted() {
        let s = Secret::new(1234u64);
        let text = format!("{:?}", s);
        assert!(text.contains("[REDACTED]"));
        assert!(!text.contains("1234"));
    }

    #[test]
    fn secret_serializes_as_redacted_string() {
        let s = Secret::new(1234u64);
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"[REDACTED]\"");
    }

    #[test]
    fn expose_returns_inner() {
        let s = Secret::new(99u64);
        assert_eq!(*s.expose(), 99);
    }
}
