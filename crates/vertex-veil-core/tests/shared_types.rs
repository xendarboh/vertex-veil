//! Integration tests for shared public types.
//!
//! Covers deterministic ser/de roundtrips for `PublicIntent`, `RoundId`, and
//! `RoundState`, and confirms that `Secret`-wrapped private values never
//! serialize in plaintext.

use vertex_veil_core::{
    CapabilityTag, NodeId, PrivateRequesterIntent, PublicIntent, RoundId, RoundState, Secret,
};

fn node(byte: u8) -> NodeId {
    NodeId::from_bytes([byte; 32])
}

#[test]
fn shared_types_public_intent_requester_roundtrip_is_stable() {
    let original = PublicIntent::Requester {
        node_id: node(0x11),
        round: RoundId::new(0),
        required_capability: CapabilityTag::parse_shape("GPU").unwrap(),
    };
    let json = serde_json::to_string(&original).unwrap();
    let back: PublicIntent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, original);

    // Determinism: a second encoding of the same value produces identical
    // bytes.
    let json2 = serde_json::to_string(&original).unwrap();
    assert_eq!(json, json2);
}

#[test]
fn shared_types_public_intent_provider_roundtrip() {
    let original = PublicIntent::Provider {
        node_id: node(0x22),
        round: RoundId::new(3),
        capability_claims: vec![
            CapabilityTag::parse_shape("GPU").unwrap(),
            CapabilityTag::parse_shape("LLM").unwrap(),
        ],
    };
    let json = serde_json::to_string(&original).unwrap();
    let back: PublicIntent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, original);
}

#[test]
fn shared_types_round_state_roundtrip() {
    let rs = RoundState::opening(RoundId::new(1), node(0x33));
    let json = serde_json::to_string(&rs).unwrap();
    let back: RoundState = serde_json::from_str(&json).unwrap();
    assert_eq!(back, rs);
}

#[test]
fn shared_types_debug_redacts_private_fields() {
    let pr = PrivateRequesterIntent {
        node_id: node(0x11),
        required_capability: CapabilityTag::parse_shape("GPU").unwrap(),
        budget_cents: Secret::new(12345u64),
    };
    let rendered = format!("{pr:?}");
    assert!(rendered.contains("[REDACTED]"));
    assert!(!rendered.contains("12345"));
}

#[test]
fn shared_types_secret_does_not_serialize_value() {
    let s = Secret::new(4242u64);
    let json = serde_json::to_string(&s).unwrap();
    assert_eq!(json, "\"[REDACTED]\"");
    assert!(!json.contains("4242"));
}
