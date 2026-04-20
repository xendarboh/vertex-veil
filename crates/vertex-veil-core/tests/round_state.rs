//! Integration tests for the round state machine and candidate formation.
//!
//! Plan coverage (this file is the densest in Phase 1):
//!
//! - Happy Path: candidate formation, stable-key winner, fallback advance
//! - Bad Path: unknown-commitment proposal, missing-capability, invalid
//!   round transition, double-commit, prior-round metadata rejection
//! - Edge Cases: no feasible providers, tied feasibility, silent provider
//! - Security: round binding, tampered proposal metadata, double-commit,
//!   replay detection
//! - Data Leak: Debug output for round state stays public-only
//! - Data Damage: atomic rejection, roundtrip-stable ordering, proposal
//!   rejection does not corrupt commitments, double-commit rejection is
//!   idempotent

use std::path::PathBuf;

use vertex_veil_core::{
    commit_provider, commit_requester, derive_candidate, derive_test_nonce, CapabilityTag,
    CommitmentRecord, NodeId, PredicateDenial, PrivateProviderIntent, PrivateRequesterIntent,
    ProposalRecord, PublicIntent, RoundError, RoundId, RoundMachine, Secret, TopologyConfig,
};

fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root")
        .to_path_buf()
}

fn load_baseline() -> TopologyConfig {
    TopologyConfig::load(repo_root().join("fixtures/topology-4node.toml"))
        .expect("baseline topology loads")
}

fn baseline_node_ids() -> (NodeId, NodeId, NodeId, NodeId) {
    (
        NodeId::from_bytes([0x11; 32]),
        NodeId::from_bytes([0x22; 32]),
        NodeId::from_bytes([0x33; 32]),
        NodeId::from_bytes([0x44; 32]),
    )
}

fn gpu() -> CapabilityTag {
    CapabilityTag::parse_shape("GPU").unwrap()
}

fn llm() -> CapabilityTag {
    CapabilityTag::parse_shape("LLM").unwrap()
}

fn cpu() -> CapabilityTag {
    CapabilityTag::parse_shape("CPU").unwrap()
}

fn mk_requester_commit(round: RoundId, budget: u64) -> CommitmentRecord {
    let intent = PrivateRequesterIntent {
        node_id: NodeId::from_bytes([0x11; 32]),
        required_capability: gpu(),
        budget_cents: Secret::new(budget),
    };
    let nonce = derive_test_nonce(intent.node_id, round, b"requester");
    let bytes = commit_requester(&intent, &nonce, round).unwrap();
    CommitmentRecord {
        node_id: intent.node_id,
        round,
        commitment_hex: bytes.to_hex(),
        public_intent: PublicIntent::Requester {
            node_id: intent.node_id,
            round,
            required_capability: intent.required_capability,
        },
    }
}

fn mk_provider_commit(
    node: NodeId,
    round: RoundId,
    claims: Vec<CapabilityTag>,
    reservation: u64,
) -> CommitmentRecord {
    let intent = PrivateProviderIntent {
        node_id: node,
        capability_claims: claims.clone(),
        reservation_cents: Secret::new(reservation),
    };
    let nonce = derive_test_nonce(node, round, b"provider");
    let bytes = commit_provider(&intent, &nonce, round).unwrap();
    CommitmentRecord {
        node_id: node,
        round,
        commitment_hex: bytes.to_hex(),
        public_intent: PublicIntent::Provider {
            node_id: node,
            round,
            capability_claims: claims,
        },
    }
}

fn seed_machine_round_0() -> RoundMachine {
    let cfg = load_baseline();
    let mut m = RoundMachine::new(cfg, RoundId::new(0));
    let (_r, p1, p2, p3) = baseline_node_ids();
    m.accept_commitment(mk_requester_commit(RoundId::new(0), 5_000))
        .unwrap();
    m.accept_commitment(mk_provider_commit(
        p1,
        RoundId::new(0),
        vec![gpu(), llm()],
        3_000,
    ))
    .unwrap();
    m.accept_commitment(mk_provider_commit(p2, RoundId::new(0), vec![gpu()], 2_800))
        .unwrap();
    m.accept_commitment(mk_provider_commit(p3, RoundId::new(0), vec![cpu()], 2_500))
        .unwrap();
    m
}

#[test]
fn round_state_candidate_formation_finds_feasible_provider() {
    let m = seed_machine_round_0();
    let (requester_id, p1, _, _) = baseline_node_ids();
    let req = m.commitments().get(&requester_id).unwrap().clone();
    let providers: Vec<CommitmentRecord> = m
        .commitments()
        .values()
        .filter(|c| c.node_id != requester_id)
        .cloned()
        .collect();
    let candidate = derive_candidate(RoundId::new(0), &req, &providers)
        .unwrap()
        .expect("feasible provider exists");
    // Stable key order: 0x22 < 0x33, and 0x44 (CPU-only) is filtered out.
    assert_eq!(candidate.requester, requester_id);
    assert_eq!(candidate.provider, p1);
    assert_eq!(candidate.matched_capability, gpu());
}

#[test]
fn round_state_stable_key_winner_is_consistent_across_runs() {
    for _ in 0..5 {
        let m = seed_machine_round_0();
        let (requester_id, p1, _, _) = baseline_node_ids();
        let req = m.commitments().get(&requester_id).unwrap().clone();
        let providers: Vec<CommitmentRecord> = m
            .commitments()
            .values()
            .filter(|c| c.node_id != requester_id)
            .cloned()
            .collect();
        let c = derive_candidate(RoundId::new(0), &req, &providers)
            .unwrap()
            .unwrap();
        assert_eq!(c.provider, p1);
    }
}

#[test]
fn round_state_candidate_formation_excludes_incompatible_providers() {
    let cfg = load_baseline();
    let (_r, _p1, _p2, p3) = baseline_node_ids();
    let req = mk_requester_commit(RoundId::new(0), 5_000);
    // Only a CPU-only provider in the set — infeasible for a GPU request.
    let providers = vec![mk_provider_commit(p3, RoundId::new(0), vec![cpu()], 2_500)];
    let outcome = derive_candidate(RoundId::new(0), &req, &providers).unwrap();
    assert!(
        outcome.is_none(),
        "no feasible provider must return Ok(None), not a forced match"
    );
    // And the machine still advances cleanly.
    let mut m = RoundMachine::new(cfg, RoundId::new(0));
    m.accept_commitment(req).unwrap();
    for p in providers {
        m.accept_commitment(p).unwrap();
    }
    m.advance_fallback().unwrap();
    assert_eq!(m.current_round(), RoundId::new(1));
}

#[test]
fn round_state_tied_feasibility_resolves_deterministically() {
    let cfg = load_baseline();
    let (_r, p1, p2, _) = baseline_node_ids();
    let mut m = RoundMachine::new(cfg, RoundId::new(0));
    m.accept_commitment(mk_requester_commit(RoundId::new(0), 5_000))
        .unwrap();
    m.accept_commitment(mk_provider_commit(p1, RoundId::new(0), vec![gpu()], 2_800))
        .unwrap();
    m.accept_commitment(mk_provider_commit(p2, RoundId::new(0), vec![gpu()], 2_800))
        .unwrap();
    let (requester_id, _, _, _) = baseline_node_ids();
    let req = m.commitments().get(&requester_id).unwrap().clone();
    let providers: Vec<CommitmentRecord> = m
        .commitments()
        .values()
        .filter(|c| c.node_id != requester_id)
        .cloned()
        .collect();
    let c = derive_candidate(RoundId::new(0), &req, &providers)
        .unwrap()
        .unwrap();
    // Tied feasibility: lower stable key wins.
    assert_eq!(c.provider, p1);
}

#[test]
fn round_state_rejects_double_commit_from_same_key() {
    let mut m = seed_machine_round_0();
    let (_r, p1, _, _) = baseline_node_ids();
    let extra = mk_provider_commit(p1, RoundId::new(0), vec![gpu()], 2_900);
    let before_len = m.commitments().len();
    let err = m.accept_commitment(extra).unwrap_err();
    assert!(matches!(err, RoundError::DuplicateCommitment { round: 0 }));
    // State unchanged: commitment count and proposal are identical.
    assert_eq!(m.commitments().len(), before_len);
    assert!(m.proposal().is_none());
}

#[test]
fn round_state_rejects_prior_round_commitment_in_current_round() {
    let mut m = seed_machine_round_0();
    // An attacker submits a commitment record labelled with round 0 after
    // the machine has advanced to round 1.
    m.advance_fallback().unwrap();
    assert_eq!(m.current_round(), RoundId::new(1));
    let (_r, p2, _, _) = baseline_node_ids();
    let stale = mk_provider_commit(p2, RoundId::new(0), vec![gpu()], 2_800);
    let err = m.accept_commitment(stale).unwrap_err();
    assert!(matches!(err, RoundError::RoundMismatch { .. }));
}

#[test]
fn round_state_detects_replay_of_prior_round_hex() {
    let mut m = seed_machine_round_0();
    let replay_source = m.commitments().values().next().cloned().unwrap();
    m.advance_fallback().unwrap();
    let (r_id, _, _, _) = baseline_node_ids();
    // Build a fresh round-1 requester commitment but splice in the old hex.
    let mut replay = mk_requester_commit(RoundId::new(1), 7_777);
    replay.commitment_hex = replay_source.commitment_hex.clone();
    let _ = r_id; // only used for mirror variable naming clarity
    let err = m.accept_commitment(replay).unwrap_err();
    assert_eq!(err, RoundError::ReplayDetected);
}

#[test]
fn round_state_rejects_unknown_node_commitment() {
    let cfg = load_baseline();
    let mut m = RoundMachine::new(cfg, RoundId::new(0));
    let unknown = NodeId::from_bytes([0xEE; 32]);
    let c = mk_provider_commit(unknown, RoundId::new(0), vec![gpu()], 2_000);
    let err = m.accept_commitment(c).unwrap_err();
    assert_eq!(err, RoundError::UnknownNode);
}

#[test]
fn round_state_rejects_role_mismatched_commitment() {
    let cfg = load_baseline();
    let mut m = RoundMachine::new(cfg, RoundId::new(0));
    let (r_id, _, _, _) = baseline_node_ids();
    // The requester key submits a provider-role public intent.
    let c = mk_provider_commit(r_id, RoundId::new(0), vec![gpu()], 1_000);
    let err = m.accept_commitment(c).unwrap_err();
    assert_eq!(err, RoundError::RoleMismatch);
}

#[test]
fn round_state_rejects_backward_transition() {
    let mut m = seed_machine_round_0();
    m.advance_fallback().unwrap();
    let err = m.advance_to(RoundId::new(0)).unwrap_err();
    assert_eq!(err, RoundError::BackwardTransition);
}

#[test]
fn round_state_proposer_rotation_follows_fallback() {
    let mut m = seed_machine_round_0();
    // Seed a tampered proposal to force a fallback.
    m.advance_fallback().unwrap();
    // Reseed round 1 with fresh commitments.
    let (_r, p1, p2, p3) = baseline_node_ids();
    m.accept_commitment(mk_requester_commit(RoundId::new(1), 5_000))
        .unwrap();
    m.accept_commitment(mk_provider_commit(p1, RoundId::new(1), vec![gpu()], 3_000))
        .unwrap();
    m.accept_commitment(mk_provider_commit(p2, RoundId::new(1), vec![gpu()], 2_800))
        .unwrap();
    m.accept_commitment(mk_provider_commit(p3, RoundId::new(1), vec![cpu()], 2_500))
        .unwrap();
    // Round 0 proposer was the first provider; round 1 proposer is the
    // second.
    let round0_expected = p1;
    let round1_expected = p2;
    // Confirm the rotation moved forward.
    assert_ne!(round0_expected, round1_expected);
    assert_eq!(m.current_proposer().unwrap(), round1_expected);
}

#[test]
fn round_state_accepts_valid_proposal() {
    let mut m = seed_machine_round_0();
    let proposer = m.current_proposer().unwrap();
    let (r_id, p1, _, _) = baseline_node_ids();
    let proposal = ProposalRecord {
        proposer,
        round: RoundId::new(0),
        candidate_requester: r_id,
        candidate_provider: p1,
        matched_capability: gpu(),
    };
    m.accept_proposal(proposal.clone()).unwrap();
    assert_eq!(m.proposal(), Some(&proposal));
}

#[test]
fn round_state_rejects_proposal_referencing_unknown_commitment() {
    let mut m = seed_machine_round_0();
    let proposer = m.current_proposer().unwrap();
    let (r_id, _, _, _) = baseline_node_ids();
    let phantom_provider = NodeId::from_bytes([0xBE; 32]);
    let before = m.commitments().clone();
    let err = m
        .accept_proposal(ProposalRecord {
            proposer,
            round: RoundId::new(0),
            candidate_requester: r_id,
            candidate_provider: phantom_provider,
            matched_capability: gpu(),
        })
        .unwrap_err();
    assert_eq!(err, RoundError::UnknownProviderCommitment);
    // Data Damage: rejected proposal did not corrupt commitments.
    assert_eq!(m.commitments(), &before);
    assert!(m.proposal().is_none());
}

#[test]
fn round_state_rejects_proposal_with_wrong_proposer() {
    let mut m = seed_machine_round_0();
    let (r_id, p1, _, _) = baseline_node_ids();
    let proposal = ProposalRecord {
        // Wrong proposer: use p1 when the real proposer is whichever the
        // rotation picks. We construct a guaranteed-wrong value by
        // flipping all bytes of the true proposer.
        proposer: NodeId::from_bytes([0xFE; 32]),
        round: RoundId::new(0),
        candidate_requester: r_id,
        candidate_provider: p1,
        matched_capability: gpu(),
    };
    let err = m.accept_proposal(proposal).unwrap_err();
    assert_eq!(err, RoundError::WrongProposer);
}

#[test]
fn round_state_rejects_tampered_capability_annotation() {
    let mut m = seed_machine_round_0();
    let proposer = m.current_proposer().unwrap();
    let (r_id, p1, _, _) = baseline_node_ids();
    let before = m.commitments().clone();
    let err = m
        .accept_proposal(ProposalRecord {
            proposer,
            round: RoundId::new(0),
            candidate_requester: r_id,
            candidate_provider: p1,
            // Requester asked for GPU; tampering the annotation to LLM
            // must fail the annotation check.
            matched_capability: llm(),
        })
        .unwrap_err();
    assert_eq!(
        err,
        RoundError::TamperedProposal(PredicateDenial::CapabilityAnnotationMismatch)
    );
    assert_eq!(m.commitments(), &before);
}

#[test]
fn round_state_rejects_prior_round_proposal() {
    let mut m = seed_machine_round_0();
    m.advance_fallback().unwrap();
    let proposer = m.current_proposer().unwrap();
    let (r_id, p1, _, _) = baseline_node_ids();
    let err = m
        .accept_proposal(ProposalRecord {
            proposer,
            round: RoundId::new(0), // stale
            candidate_requester: r_id,
            candidate_provider: p1,
            matched_capability: gpu(),
        })
        .unwrap_err();
    assert!(matches!(err, RoundError::RoundMismatch { expected: 1, actual: 0 }));
}

#[test]
fn round_state_silent_provider_does_not_block_advance() {
    let cfg = load_baseline();
    let mut m = RoundMachine::new(cfg, RoundId::new(0));
    let (_r, p1, _p2_silent, p3) = baseline_node_ids();
    m.accept_commitment(mk_requester_commit(RoundId::new(0), 5_000))
        .unwrap();
    m.accept_commitment(mk_provider_commit(p1, RoundId::new(0), vec![gpu()], 2_800))
        .unwrap();
    // p2 stays silent.
    m.accept_commitment(mk_provider_commit(p3, RoundId::new(0), vec![cpu()], 2_500))
        .unwrap();
    // Advance cleanly even though p2 never committed.
    m.advance_fallback().unwrap();
    assert_eq!(m.current_round(), RoundId::new(1));
    assert_eq!(m.finalized_round_count(), 1);
    let past = m.past_commitments(0).unwrap();
    assert_eq!(past.len(), 3); // req + p1 + p3 only
}

#[test]
fn round_state_commitment_ordering_stable_after_serialization_roundtrip() {
    let m = seed_machine_round_0();
    let mut ids: Vec<NodeId> = m.commitments().keys().copied().collect();
    ids.sort();
    let ids_before = ids.clone();

    // Serialize and deserialize each commitment independently; the stable
    // ordering of keys must be preserved.
    let records: Vec<CommitmentRecord> = ids
        .iter()
        .map(|id| m.commitments().get(id).unwrap().clone())
        .collect();
    let json = serde_json::to_string(&records).unwrap();
    let back: Vec<CommitmentRecord> = serde_json::from_str(&json).unwrap();
    let mut ids_after: Vec<NodeId> = back.iter().map(|r| r.node_id).collect();
    ids_after.sort();
    assert_eq!(ids_before, ids_after);
}

#[test]
fn round_state_debug_output_is_public_only() {
    let m = seed_machine_round_0();
    let rendered = format!("{m:?}");
    for forbidden in ["budget", "reservation", "5000", "2800", "2500", "[REDACTED]"] {
        assert!(
            !rendered.contains(forbidden),
            "RoundMachine debug leaked marker {forbidden:?}"
        );
    }
}
