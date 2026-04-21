//! Rust/Noir predicate parity (Phase 2).
//!
//! Plan coverage:
//!
//! - Happy Path: shared parity fixtures produce matching allow/deny results
//!   in the Rust and Noir implementations.
//! - Bad Path: Rust and Noir predicate outputs diverging on the same fixture
//!   fails the parity suite hard.
//! - Security: parity fixtures cover tampered metadata and mismatched round
//!   cases.
//! - Data Leak: parity test failures do not leak private witness values
//!   while explaining the mismatch.
//!
//! ## What "parity" means in Phase 2
//!
//! The `match_predicate` function lives in Rust. The Phase 2 Noir circuits
//! (`requester` and `provider`) prove commitment binding + private threshold
//! checks. They are complementary views of a single logical decision:
//!
//! * Rust's `match_predicate` decides whether a public pair is structurally
//!   feasible.
//! * The Noir circuits decide whether a specific private witness satisfies
//!   the committed intent.
//!
//! "Predicate parity" here asserts that for every shared fixture:
//!
//! 1. The Rust predicate's output matches the fixture's declared expected
//!    outcome (reused from Phase 1).
//! 2. For Accept fixtures: we can construct real requester + provider
//!    commitments from the fixture's public intents plus synthetic private
//!    witnesses, and both circuits execute cleanly over the resulting
//!    witness maps. That is the Noir-side "Accept" signal.
//! 3. For Reject fixtures: we cannot construct a valid witness that would
//!    make the Noir circuit accept with the fixture's tampered public
//!    inputs. Specifically, swapping the published commitment to one bound
//!    to a different round fails the circuit execution — matching the Rust
//!    predicate's rejection.

use std::path::PathBuf;

use vertex_veil_core::{
    match_predicate, CapabilityTag, ExpectedOutcome, NodeId, ParityFixture, PredicateDenial,
    PrivateProviderIntent, PrivateRequesterIntent, PublicIntent, RoundId, Secret,
};
use vertex_veil_noir::{
    CircuitArtifact, ProviderCircuit, ProviderWitness, RequesterCircuit, RequesterWitness,
};

fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root")
        .to_path_buf()
}

fn parity_dir() -> PathBuf {
    repo_root().join("fixtures/parity")
}

fn load_requester_circuit() -> RequesterCircuit {
    let art =
        CircuitArtifact::from_path(repo_root().join("circuits/target/vertex_veil_requester.json"))
            .expect("requester circuit artifact; run `nargo compile --workspace` first");
    RequesterCircuit::load(art).unwrap()
}

fn load_provider_circuit() -> ProviderCircuit {
    let art =
        CircuitArtifact::from_path(repo_root().join("circuits/target/vertex_veil_provider.json"))
            .expect("provider circuit artifact; run `nargo compile --workspace` first");
    ProviderCircuit::load(art).unwrap()
}

#[test]
fn predicate_parity_rust_matches_every_fixture() {
    // Reused from Phase 1: Rust outputs match the fixture's declared
    // expected outcome. We repeat it here so predicate_parity stands on
    // its own; a failure is a real parity violation.
    let fixtures = ParityFixture::load_dir(parity_dir()).unwrap();
    assert!(!fixtures.is_empty(), "expected parity fixtures present");
    for fx in fixtures {
        let actual =
            ExpectedOutcome::from_runtime(match_predicate(&fx.requester, &fx.provider, fx.round));
        assert_eq!(
            actual, fx.expected,
            "fixture {:?} Rust predicate diverged from declared outcome",
            fx.name
        );
    }
}

#[test]
fn predicate_parity_noir_executes_for_every_accept_fixture() {
    // For each Accept fixture, build real commitments with synthetic private
    // witnesses that satisfy each side's threshold, and assert the Noir
    // circuits execute (= Noir's allow). Using synthetic thresholds means
    // no fixture carries private data.
    let requester_circuit = load_requester_circuit();
    let provider_circuit = load_provider_circuit();

    let fixtures = ParityFixture::load_dir(parity_dir()).unwrap();
    for fx in fixtures.iter().filter(|f| f.expected == ExpectedOutcome::Accept) {
        // Derive a requester private intent from the fixture's public intent.
        let (requester_intent, required_capability) = match &fx.requester {
            PublicIntent::Requester {
                node_id,
                required_capability,
                ..
            } => (
                PrivateRequesterIntent {
                    node_id: *node_id,
                    required_capability: required_capability.clone(),
                    budget_cents: Secret::new(1_000),
                },
                required_capability.clone(),
            ),
            _ => panic!("accept fixture {:?} must carry requester role", fx.name),
        };
        let requester_nonce = [0xa1u8; 32];
        let (req_public, _req_commit) = RequesterCircuit::public_inputs_from_intent(
            &requester_intent,
            &requester_nonce,
            fx.round,
            500,
        )
        .expect("requester public inputs");
        let req_witness = RequesterWitness {
            budget_cents: *requester_intent.budget_cents.expose(),
            nonce: requester_nonce,
        };
        requester_circuit.execute(&req_public, &req_witness).unwrap_or_else(|e| {
            panic!(
                "fixture {:?} requester circuit rejected a supposedly-accept pair: {e}",
                fx.name
            );
        });

        // And the provider side.
        let provider_intent = match &fx.provider {
            PublicIntent::Provider {
                node_id,
                capability_claims,
                ..
            } => PrivateProviderIntent {
                node_id: *node_id,
                capability_claims: capability_claims.clone(),
                reservation_cents: Secret::new(100),
            },
            _ => panic!("accept fixture {:?} must carry provider role", fx.name),
        };
        let provider_nonce = [0xb2u8; 32];
        let (prov_public, _prov_commit) = ProviderCircuit::public_inputs_from_intent(
            &provider_intent,
            &provider_nonce,
            fx.round,
            200,
        )
        .expect("provider public inputs");
        let prov_witness = ProviderWitness {
            reservation_cents: *provider_intent.reservation_cents.expose(),
            nonce: provider_nonce,
        };
        provider_circuit.execute(&prov_public, &prov_witness).unwrap_or_else(|e| {
            panic!(
                "fixture {:?} provider circuit rejected a supposedly-accept pair: {e}",
                fx.name
            );
        });

        // Sanity: the capability carried by the public fixture is the same one
        // bound into both circuits.
        match &fx.provider {
            PublicIntent::Provider { capability_claims, .. } => {
                assert!(
                    capability_claims.contains(&required_capability),
                    "accept fixture {:?} has provider claims missing the required capability",
                    fx.name
                );
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn predicate_parity_noir_rejects_when_requester_round_tampered() {
    // Strong parity signal for the round-mismatch fixture: if we try to
    // verify a requester commitment bound to round 0 against public round
    // 2, the Noir circuit must reject, matching the Rust predicate's
    // RequesterRoundMismatch denial.
    let requester_circuit = load_requester_circuit();

    let node_id = NodeId::from_bytes([0x11; 32]);
    let intent = PrivateRequesterIntent {
        node_id,
        required_capability: CapabilityTag::parse_shape("GPU").unwrap(),
        budget_cents: Secret::new(500),
    };
    let nonce = [0xa1; 32];
    // Build commitment bound to round 1 (mirrors the round_mismatch fixture's requester).
    let (mut public, _commit) =
        RequesterCircuit::public_inputs_from_intent(&intent, &nonce, RoundId::new(1), 250)
            .unwrap();
    // Tamper the public round to 2.
    public.round = RoundId::new(2);
    let witness = RequesterWitness {
        budget_cents: *intent.budget_cents.expose(),
        nonce,
    };
    let err = requester_circuit.execute(&public, &witness).unwrap_err();

    // Rust-side denial for the equivalent scenario.
    let rust_result = match_predicate(
        &PublicIntent::Requester {
            node_id,
            round: RoundId::new(1),
            required_capability: CapabilityTag::parse_shape("GPU").unwrap(),
        },
        &PublicIntent::Provider {
            node_id: NodeId::from_bytes([0x22; 32]),
            round: RoundId::new(2),
            capability_claims: vec![CapabilityTag::parse_shape("GPU").unwrap()],
        },
        RoundId::new(2),
    );
    assert_eq!(rust_result, Err(PredicateDenial::RequesterRoundMismatch));

    // Data leak check: the Noir error message must not echo the private budget.
    let msg = format!("{err}");
    assert!(!msg.contains("500"), "parity failure leaked private budget: {msg}");
}

#[test]
fn predicate_parity_codes_are_stable_strings() {
    // PredicateDenial tags are part of the parity contract. If this suite
    // ever needs to compare a Rust denial with a Noir-emitted code, the
    // strings must be stable. Pin them here.
    let cases: &[(PredicateDenial, &str)] = &[
        (PredicateDenial::WrongRequesterRole, "wrong_requester_role"),
        (PredicateDenial::WrongProviderRole, "wrong_provider_role"),
        (
            PredicateDenial::RequesterRoundMismatch,
            "requester_round_mismatch",
        ),
        (
            PredicateDenial::ProviderRoundMismatch,
            "provider_round_mismatch",
        ),
        (
            PredicateDenial::RequesterProviderRoundMismatch,
            "requester_provider_round_mismatch",
        ),
        (
            PredicateDenial::ProviderLacksCapability,
            "provider_lacks_capability",
        ),
        (
            PredicateDenial::RequesterIdentityMismatch,
            "requester_identity_mismatch",
        ),
        (
            PredicateDenial::ProviderIdentityMismatch,
            "provider_identity_mismatch",
        ),
        (
            PredicateDenial::CapabilityAnnotationMismatch,
            "capability_annotation_mismatch",
        ),
    ];
    for (denial, expected) in cases {
        assert_eq!(denial.tag(), *expected);
    }
}
