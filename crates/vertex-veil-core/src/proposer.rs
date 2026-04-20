//! Deterministic proposer rotation.
//!
//! The proposer for round `N` is the provider at index `N mod len` in the
//! stable-key-ordered provider list. "Stable public key order" is the
//! byte-lexicographic order defined in `crate::keys`.
//!
//! Fallback rounds advance the proposer pointer by one per round. Repeated
//! failures walk the provider ring deterministically. The same public
//! inputs always pick the same proposer, so any verifier can reproduce the
//! rotation from the coordination log alone.

use std::collections::BTreeSet;

use crate::keys::NodeId;
use crate::shared_types::RoundId;

/// Errors that can arise while computing the proposer for a round.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProposerError {
    #[error("provider list must not be empty")]
    NoProviders,

    #[error("duplicate provider key in proposer input: {0}")]
    DuplicateInput(String),
}

/// Return the proposer for the given round.
///
/// `providers_stable` must be in stable (byte-lex) order and free of
/// duplicates. The function validates both preconditions and rejects
/// violations deterministically.
pub fn proposer_for_round(
    round: RoundId,
    providers_stable: &[NodeId],
) -> Result<NodeId, ProposerError> {
    if providers_stable.is_empty() {
        return Err(ProposerError::NoProviders);
    }

    // Duplicate rejection also catches silent corruption of the input
    // ordering.
    let mut seen = BTreeSet::new();
    for p in providers_stable {
        if !seen.insert(*p) {
            return Err(ProposerError::DuplicateInput(p.to_hex()));
        }
    }

    let idx = (round.value() as usize) % providers_stable.len();
    Ok(providers_stable[idx])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(b: u8) -> NodeId {
        NodeId::from_bytes([b; 32])
    }

    #[test]
    fn deterministic_same_inputs() {
        let providers = vec![n(0x11), n(0x22), n(0x33)];
        let a = proposer_for_round(RoundId::new(0), &providers).unwrap();
        let b = proposer_for_round(RoundId::new(0), &providers).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn rotation_advances_by_one() {
        let providers = vec![n(0x11), n(0x22), n(0x33)];
        assert_eq!(proposer_for_round(RoundId::new(0), &providers).unwrap(), n(0x11));
        assert_eq!(proposer_for_round(RoundId::new(1), &providers).unwrap(), n(0x22));
        assert_eq!(proposer_for_round(RoundId::new(2), &providers).unwrap(), n(0x33));
    }

    #[test]
    fn rotation_wraps_around() {
        let providers = vec![n(0x11), n(0x22), n(0x33)];
        assert_eq!(proposer_for_round(RoundId::new(3), &providers).unwrap(), n(0x11));
        assert_eq!(proposer_for_round(RoundId::new(7), &providers).unwrap(), n(0x22));
    }

    #[test]
    fn rejects_empty_input() {
        let err = proposer_for_round(RoundId::new(0), &[]).unwrap_err();
        assert_eq!(err, ProposerError::NoProviders);
    }

    #[test]
    fn rejects_duplicate_input() {
        let providers = vec![n(0x11), n(0x11)];
        let err = proposer_for_round(RoundId::new(0), &providers).unwrap_err();
        assert!(matches!(err, ProposerError::DuplicateInput(_)));
    }
}
