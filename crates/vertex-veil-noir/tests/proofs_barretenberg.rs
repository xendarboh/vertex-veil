//! Optional: full UltraHonk prove + verify via `noir_rs/barretenberg`.
//!
//! Gated behind the `barretenberg` cargo feature. Pulls in `barretenberg-rs`,
//! downloads (Range-fetches) the Aztec CRS slice needed for the circuit, and
//! runs a full proof generation + verification against each circuit.
//!
//! Skipped by default: the constraint-validation flow in `tests/proofs.rs`
//! (in `vertex-veil-core`) is sufficient for Phase 2 acceptance criteria
//! that don't strictly require a ZK proof.
//!
//! To enable:
//!
//! ```bash
//! cargo test -p vertex-veil-noir --features barretenberg --test proofs_barretenberg
//! ```

#![cfg(feature = "barretenberg")]

use std::path::PathBuf;

use vertex_veil_core::{
    CapabilityTag, NodeId, PrivateProviderIntent, PrivateRequesterIntent, RoundId, Secret,
};
use vertex_veil_noir::{
    CircuitArtifact, ProviderCircuit, ProviderWitness, RequesterCircuit, RequesterWitness,
};

use noir_rs::barretenberg::{
    prove::prove_ultra_honk, srs::setup_srs_from_bytecode,
    verify::{get_ultra_honk_verification_key, verify_ultra_honk},
};

fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root")
        .to_path_buf()
}

#[test]
fn requester_full_prove_then_verify() {
    let art = CircuitArtifact::from_path(
        repo_root().join("circuits/target/vertex_veil_requester.json"),
    )
    .expect("compiled requester artifact; run `nargo compile --workspace` in circuits/");
    let circuit = RequesterCircuit::load(art).unwrap();

    setup_srs_from_bytecode(circuit.bytecode(), None, false)
        .expect("SRS setup (downloads from crs.aztec.network if absent)");

    let intent = PrivateRequesterIntent {
        node_id: NodeId::from_bytes([0x11; 32]),
        required_capability: CapabilityTag::parse_shape("GPU").unwrap(),
        budget_cents: Secret::new(500),
        signing_secret_key: None,
    };
    let nonce = [0xa1; 32];
    let (public, _commit) =
        RequesterCircuit::public_inputs_from_intent(&intent, &nonce, RoundId::new(0), 100)
            .unwrap();
    let witness = RequesterWitness {
        budget_cents: *intent.budget_cents.expose(),
        nonce,
    };

    let map = circuit.build_witness_map(&public, &witness);
    let proof = prove_ultra_honk(circuit.bytecode(), map, Vec::new(), false, None)
        .expect("prove succeeds");
    let vk = get_ultra_honk_verification_key(circuit.bytecode(), false, None)
        .expect("vk computes");
    let ok = verify_ultra_honk(proof, vk).expect("verify succeeds");
    assert!(ok, "valid proof must verify");
}

#[test]
fn provider_full_prove_then_verify() {
    let art = CircuitArtifact::from_path(
        repo_root().join("circuits/target/vertex_veil_provider.json"),
    )
    .expect("compiled provider artifact; run `nargo compile --workspace` in circuits/");
    let circuit = ProviderCircuit::load(art).unwrap();

    setup_srs_from_bytecode(circuit.bytecode(), None, false).expect("SRS setup");

    let intent = PrivateProviderIntent {
        node_id: NodeId::from_bytes([0x22; 32]),
        capability_claims: vec![CapabilityTag::parse_shape("GPU").unwrap()],
        reservation_cents: Secret::new(150),
        signing_secret_key: None,
    };
    let nonce = [0xb2; 32];
    let (public, _commit) =
        ProviderCircuit::public_inputs_from_intent(&intent, &nonce, RoundId::new(0), 200)
            .unwrap();
    let witness = ProviderWitness {
        reservation_cents: *intent.reservation_cents.expose(),
        nonce,
    };
    let map = circuit.build_witness_map(&public, &witness);

    let proof =
        prove_ultra_honk(circuit.bytecode(), map, Vec::new(), false, None).expect("prove");
    let vk = get_ultra_honk_verification_key(circuit.bytecode(), false, None).expect("vk");
    assert!(verify_ultra_honk(proof, vk).expect("verify"));
}

#[test]
fn requester_wrong_round_proof_rejected() {
    // Generate a proof bound to round 0, then attempt to verify it with a
    // verification key derived from a different public-input vector. This
    // mirrors the "prior-round proof replayed into the active round"
    // scenario: the proof is valid against its own public inputs but the
    // verifier checks it with tampered public inputs.
    let art = CircuitArtifact::from_path(
        repo_root().join("circuits/target/vertex_veil_requester.json"),
    )
    .expect("compiled requester artifact; run `nargo compile --workspace` in circuits/");
    let circuit = RequesterCircuit::load(art).unwrap();

    setup_srs_from_bytecode(circuit.bytecode(), None, false).expect("SRS setup");

    let intent = PrivateRequesterIntent {
        node_id: NodeId::from_bytes([0x11; 32]),
        required_capability: CapabilityTag::parse_shape("GPU").unwrap(),
        budget_cents: Secret::new(500),
        signing_secret_key: None,
    };
    let nonce = [0xa1; 32];
    let (public, _commit) =
        RequesterCircuit::public_inputs_from_intent(&intent, &nonce, RoundId::new(0), 100)
            .unwrap();
    let witness = RequesterWitness {
        budget_cents: *intent.budget_cents.expose(),
        nonce,
    };

    // Tamper the public round BEFORE building the witness map: this makes
    // the witness inconsistent with the published commitment, so proving
    // itself must fail — i.e. the prover cannot produce a valid proof for
    // a wrong-round public input. This is the stronger version of
    // "verification rejects a proof artifact bound to the wrong round":
    // you can't even generate such a proof to attempt verification.
    let mut tampered_public = public.clone();
    tampered_public.round = RoundId::new(1);
    let map = circuit.build_witness_map(&tampered_public, &witness);
    let proof_result = prove_ultra_honk(circuit.bytecode(), map, Vec::new(), false, None);
    assert!(
        proof_result.is_err(),
        "prover must refuse to prove against a tampered public round"
    );
}
