//! Integration tests for commitment construction.
//!
//! Plan coverage:
//!
//! - Happy Path: deterministic commitments; round-bound commitments differ
//! - Data Leak: commitment helper errors do not expose private witness inputs
//! - Security: round binding prevents commitment reuse across rounds

use vertex_veil_core::{
    commit_provider, commit_requester, CapabilityTag, CommitmentError, NodeId,
    PrivateProviderIntent, PrivateRequesterIntent, RoundId, Secret,
};

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
fn commitments_requester_deterministic_for_same_inputs() {
    let nonce = [0xa1; 32];
    let a = commit_requester(&req(100), &nonce, RoundId::new(3)).unwrap();
    let b = commit_requester(&req(100), &nonce, RoundId::new(3)).unwrap();
    assert_eq!(a, b);
}

#[test]
fn commitments_provider_deterministic_for_same_inputs() {
    let nonce = [0xa1; 32];
    let a = commit_provider(&prov(50), &nonce, RoundId::new(3)).unwrap();
    let b = commit_provider(&prov(50), &nonce, RoundId::new(3)).unwrap();
    assert_eq!(a, b);
}

#[test]
fn commitments_change_when_round_changes() {
    let nonce = [0xa1; 32];
    let r0 = commit_requester(&req(100), &nonce, RoundId::new(0)).unwrap();
    let r1 = commit_requester(&req(100), &nonce, RoundId::new(1)).unwrap();
    assert_ne!(r0, r1);

    let p0 = commit_provider(&prov(50), &nonce, RoundId::new(0)).unwrap();
    let p1 = commit_provider(&prov(50), &nonce, RoundId::new(1)).unwrap();
    assert_ne!(p0, p1);
}

#[test]
fn commitments_round_binding_in_bytes_not_just_label() {
    // A round label change causes the hash to differ, so a prior-round
    // commitment cannot be relabelled into a later round without changing
    // the digest. This is the "round binding prevents commitment reuse
    // across rounds" security property at the library level.
    let nonce = [0xa1; 32];
    let r0 = commit_requester(&req(100), &nonce, RoundId::new(0)).unwrap();
    let r1 = commit_requester(&req(100), &nonce, RoundId::new(1)).unwrap();
    assert_ne!(r0.to_hex(), r1.to_hex());
}

#[test]
fn commitments_differ_by_private_budget() {
    // Even though budget is private, different budgets must produce
    // different commitments so the Noir circuit's local check is sound.
    let nonce = [0xa1; 32];
    let a = commit_requester(&req(100), &nonce, RoundId::new(0)).unwrap();
    let b = commit_requester(&req(101), &nonce, RoundId::new(0)).unwrap();
    assert_ne!(a, b);
}

#[test]
fn commitments_provider_errors_do_not_echo_private_reservation() {
    // Construct a malformed provider intent (no capability claims) and
    // confirm the error message does not echo the private reservation.
    let nonce = [0xa1; 32];
    let reservation = 777_007u64;
    let intent = PrivateProviderIntent {
        node_id: NodeId::from_bytes([0x22; 32]),
        capability_claims: Vec::new(),
        reservation_cents: Secret::new(reservation),
    };
    let err = commit_provider(&intent, &nonce, RoundId::new(0)).unwrap_err();
    assert_eq!(err, CommitmentError::InvalidField("capability_claims"));
    let msg = format!("{err}");
    assert!(
        !msg.contains(&reservation.to_string()),
        "error leaked private reservation: {msg}"
    );
}
