//! Integration tests for proposer rotation.
//!
//! Plan coverage:
//!
//! - Happy Path: fallback round selection advances to the next proposer
//!   deterministically; stable public key ordering selects the same winner
//!   across repeated runs
//! - Bad Path: duplicate provider key in ordering input fails deterministically

use vertex_veil_core::{proposer_for_round, NodeId, ProposerError, RoundId};

fn n(b: u8) -> NodeId {
    NodeId::from_bytes([b; 32])
}

#[test]
fn proposer_selection_is_deterministic_across_runs() {
    let providers = vec![n(0x11), n(0x22), n(0x33)];
    for round in 0..10u64 {
        let a = proposer_for_round(RoundId::new(round), &providers).unwrap();
        let b = proposer_for_round(RoundId::new(round), &providers).unwrap();
        assert_eq!(a, b, "round {round} must pick the same proposer twice");
    }
}

#[test]
fn proposer_rotation_advances_on_fallback_round() {
    let providers = vec![n(0x11), n(0x22), n(0x33)];
    let r0 = proposer_for_round(RoundId::new(0), &providers).unwrap();
    let r1 = proposer_for_round(RoundId::new(1), &providers).unwrap();
    let r2 = proposer_for_round(RoundId::new(2), &providers).unwrap();
    assert_eq!(r0, n(0x11));
    assert_eq!(r1, n(0x22));
    assert_eq!(r2, n(0x33));
}

#[test]
fn proposer_rotation_wraps_deterministically() {
    let providers = vec![n(0x11), n(0x22), n(0x33)];
    assert_eq!(
        proposer_for_round(RoundId::new(3), &providers).unwrap(),
        n(0x11)
    );
    assert_eq!(
        proposer_for_round(RoundId::new(4), &providers).unwrap(),
        n(0x22)
    );
}

#[test]
fn proposer_rejects_duplicate_provider_keys() {
    let providers = vec![n(0x11), n(0x11), n(0x22)];
    let err = proposer_for_round(RoundId::new(0), &providers).unwrap_err();
    assert!(matches!(err, ProposerError::DuplicateInput(_)));
}

#[test]
fn proposer_rejects_empty_provider_list() {
    let err = proposer_for_round(RoundId::new(0), &[]).unwrap_err();
    assert_eq!(err, ProposerError::NoProviders);
}
