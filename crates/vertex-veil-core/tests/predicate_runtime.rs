//! Integration tests for the runtime match predicate and parity fixtures.
//!
//! Plan coverage:
//!
//! - Happy Path: runtime match predicate accepts a valid requester/provider
//!   pair with consistent public metadata
//! - Bad Path: prior-round proposal or proof metadata is rejected
//! - Edge Cases: custom capability labels still match
//! - Security: proposal validation rejects tampered public metadata (role
//!   mismatch fixture, identity mismatch via annotation)
//! - Data Leak: proposal-level logs never expose requester budget or
//!   provider reservation price; parity fixtures only carry public fields

use std::path::PathBuf;

use vertex_veil_core::{
    match_predicate, validate_proposal_annotation, CapabilityTag, ExpectedOutcome, NodeId,
    ParityFixture, PredicateDenial, PublicIntent, RoundId,
};

fn n(b: u8) -> NodeId {
    NodeId::from_bytes([b; 32])
}

fn fixtures_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root")
        .join("fixtures")
        .join("parity")
}

#[test]
fn predicate_runtime_accepts_valid_pair() {
    let r = PublicIntent::Requester {
        node_id: n(0x11),
        round: RoundId::new(0),
        required_capability: CapabilityTag::parse_shape("GPU").unwrap(),
    };
    let p = PublicIntent::Provider {
        node_id: n(0x22),
        round: RoundId::new(0),
        capability_claims: vec![CapabilityTag::parse_shape("GPU").unwrap()],
    };
    assert!(match_predicate(&r, &p, RoundId::new(0)).is_ok());
}

#[test]
fn predicate_runtime_rejects_prior_round_requester() {
    let r = PublicIntent::Requester {
        node_id: n(0x11),
        round: RoundId::new(0),
        required_capability: CapabilityTag::parse_shape("GPU").unwrap(),
    };
    let p = PublicIntent::Provider {
        node_id: n(0x22),
        round: RoundId::new(1),
        capability_claims: vec![CapabilityTag::parse_shape("GPU").unwrap()],
    };
    let err = match_predicate(&r, &p, RoundId::new(1)).unwrap_err();
    assert_eq!(err, PredicateDenial::RequesterRoundMismatch);
}

#[test]
fn predicate_runtime_rejects_role_mismatch() {
    let r = PublicIntent::Provider {
        node_id: n(0x11),
        round: RoundId::new(0),
        capability_claims: vec![],
    };
    let p = PublicIntent::Provider {
        node_id: n(0x22),
        round: RoundId::new(0),
        capability_claims: vec![CapabilityTag::parse_shape("GPU").unwrap()],
    };
    let err = match_predicate(&r, &p, RoundId::new(0)).unwrap_err();
    assert_eq!(err, PredicateDenial::WrongRequesterRole);
}

#[test]
fn predicate_runtime_rejects_provider_without_capability() {
    let r = PublicIntent::Requester {
        node_id: n(0x11),
        round: RoundId::new(0),
        required_capability: CapabilityTag::parse_shape("GPU").unwrap(),
    };
    let p = PublicIntent::Provider {
        node_id: n(0x22),
        round: RoundId::new(0),
        capability_claims: vec![CapabilityTag::parse_shape("CPU").unwrap()],
    };
    let err = match_predicate(&r, &p, RoundId::new(0)).unwrap_err();
    assert_eq!(err, PredicateDenial::ProviderLacksCapability);
}

#[test]
fn predicate_runtime_validate_annotation_rejects_identity_tamper() {
    let r = PublicIntent::Requester {
        node_id: n(0x11),
        round: RoundId::new(0),
        required_capability: CapabilityTag::parse_shape("GPU").unwrap(),
    };
    let p = PublicIntent::Provider {
        node_id: n(0x22),
        round: RoundId::new(0),
        capability_claims: vec![CapabilityTag::parse_shape("GPU").unwrap()],
    };
    let err = validate_proposal_annotation(
        n(0xFE), // tampered requester identity
        &r,
        n(0x22),
        &p,
        &CapabilityTag::parse_shape("GPU").unwrap(),
    )
    .unwrap_err();
    assert_eq!(err, PredicateDenial::RequesterIdentityMismatch);
}

#[test]
fn predicate_runtime_validate_annotation_rejects_capability_tamper() {
    let r = PublicIntent::Requester {
        node_id: n(0x11),
        round: RoundId::new(0),
        required_capability: CapabilityTag::parse_shape("GPU").unwrap(),
    };
    let p = PublicIntent::Provider {
        node_id: n(0x22),
        round: RoundId::new(0),
        capability_claims: vec![CapabilityTag::parse_shape("GPU").unwrap()],
    };
    let err = validate_proposal_annotation(
        n(0x11),
        &r,
        n(0x22),
        &p,
        &CapabilityTag::parse_shape("LLM").unwrap(),
    )
    .unwrap_err();
    assert_eq!(err, PredicateDenial::CapabilityAnnotationMismatch);
}

#[test]
fn predicate_runtime_matches_parity_fixture_outcomes() {
    let dir = fixtures_dir();
    let fixtures = ParityFixture::load_dir(&dir).expect("parity fixtures load");
    assert!(
        !fixtures.is_empty(),
        "expected at least one parity fixture in {dir:?}"
    );

    for fx in fixtures {
        let actual =
            ExpectedOutcome::from_runtime(match_predicate(&fx.requester, &fx.provider, fx.round));
        assert_eq!(
            actual, fx.expected,
            "fixture {:?} diverged from its expected outcome",
            fx.name
        );
    }
}

#[test]
fn predicate_runtime_parity_fixtures_are_public_only() {
    // Every parity fixture on disk must be safe to ship and fail loudly with
    // no private witness leak. Read every fixture file as raw text and
    // assert it carries only public field names.
    let dir = fixtures_dir();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for forbidden in [
            "budget_cents",
            "reservation_cents",
            "[REDACTED]",
            "witness",
        ] {
            assert!(
                !text.contains(forbidden),
                "parity fixture {:?} leaks marker {forbidden:?}",
                path
            );
        }
    }
}

#[test]
fn predicate_runtime_denial_debug_does_not_leak_numbers() {
    // PredicateDenial is the only error surface a proposer logs when
    // rejecting a candidate. Its Debug formatting must stay structural so
    // that mismatch output never embeds numeric private values.
    let denial = PredicateDenial::ProviderLacksCapability;
    let rendered = format!("{denial:?}");
    for forbidden in ["budget", "reservation", "cents", "[REDACTED]"] {
        assert!(
            !rendered.contains(forbidden),
            "PredicateDenial debug leaked {forbidden:?}"
        );
    }
}
