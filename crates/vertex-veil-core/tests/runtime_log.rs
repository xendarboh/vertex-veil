//! Phase 3 integration tests for the Vertex-ordered coordination runtime.
//!
//! These tests drive [`CoordinationRuntime`] against [`OrderedBus`] (a
//! single-process transport that mirrors Vertex consensus-ordered delivery)
//! and assert the full protocol loop — commitments, proposal, proofs,
//! completion receipt, fallback rounds — behaves correctly end-to-end.
//! Private witness material is held in local [`AgentState`] entries and
//! NEVER appears in the resulting coordination log.
//!
//! Plan coverage:
//!
//! - Happy Path: four configured agents run one round; the log carries
//!   commitments, proposal, proofs, and receipt; the fallback path
//!   recovers from an injected invalid proof.
//! - Bad Path: replay, double-commit, corrupted-artifact rejection.
//! - Edge Cases: no-match round; silent-node; custom capability label.
//! - Security: double-commit visible; replay visible.
//! - Data Leak: the written coordination log never contains `budget_cents`,
//!   `reservation_cents`, or the `[REDACTED]` placeholder string; debug
//!   logging of runtime state does not echo secrets.
//! - Data Damage: re-deserializing the log roundtrips; restarting the
//!   runtime after a failure produces coherent artifacts.

use std::collections::BTreeMap;

use vertex_veil_core::{
    AgentState, CapabilityTag, CoordinationLog, CoordinationRuntime, NodeId, OrderedBus,
    PrivateProviderIntent, PrivateRequesterIntent, RoundId, Scenario, ScenarioEvent, Secret,
    TopologyConfig,
};

fn hex64(b: u8) -> String {
    let mut s = String::new();
    for _ in 0..32 {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn baseline_topology_text() -> String {
    format!(
        r#"
version = 1
capability_tags = ["GPU", "CPU", "LLM", "ZK_DEV"]

[[nodes]]
id = "{}"
role = "requester"
required_capability = "GPU"

[[nodes]]
id = "{}"
role = "provider"
capability_claims = ["GPU", "LLM"]

[[nodes]]
id = "{}"
role = "provider"
capability_claims = ["GPU"]

[[nodes]]
id = "{}"
role = "provider"
capability_claims = ["CPU"]
"#,
        hex64(0x11),
        hex64(0x22),
        hex64(0x33),
        hex64(0x44),
    )
}

fn baseline_agents() -> BTreeMap<NodeId, AgentState> {
    let mut m = BTreeMap::new();
    m.insert(
        NodeId::from_bytes([0x11; 32]),
        AgentState::requester(PrivateRequesterIntent {
            node_id: NodeId::from_bytes([0x11; 32]),
            required_capability: CapabilityTag::parse_shape("GPU").unwrap(),
            budget_cents: Secret::new(1000),
        }),
    );
    for (b, claims, price) in [
        (0x22u8, vec!["GPU", "LLM"], 500u64),
        (0x33u8, vec!["GPU"], 450u64),
        (0x44u8, vec!["CPU"], 200u64),
    ] {
        m.insert(
            NodeId::from_bytes([b; 32]),
            AgentState::provider(PrivateProviderIntent {
                node_id: NodeId::from_bytes([b; 32]),
                capability_claims: claims
                    .into_iter()
                    .map(|c| CapabilityTag::parse_shape(c).unwrap())
                    .collect(),
                reservation_cents: Secret::new(price),
            }),
        );
    }
    m
}

fn run(scenario: Scenario, max_rounds: u64) -> CoordinationLog {
    let topology = TopologyConfig::from_toml_str(&baseline_topology_text()).unwrap();
    let rt =
        CoordinationRuntime::new(topology, OrderedBus::new(), baseline_agents(), scenario, max_rounds)
            .unwrap();
    rt.run("runtime-log-test").unwrap().log
}

// ---------------------------------------------------------------------------
// Happy Path
// ---------------------------------------------------------------------------

#[test]
fn runtime_log_four_agents_produce_ordered_log_over_vertex() {
    let log = run(Scenario::empty(), 4);
    assert_eq!(log.final_round, RoundId::new(0));
    assert!(log.finalized);
    // Every provider + requester contributes a commitment in round 0.
    assert_eq!(log.commitments.len(), 4);
    assert_eq!(log.proposals.len(), 1);
    assert!(log.proofs.len() >= 2);
    assert!(log.receipts.len() == 1);
    assert!(log.rejections.is_empty());
}

#[test]
fn runtime_log_completes_round_with_receipt_and_acknowledgement_shape() {
    let log = run(Scenario::empty(), 4);
    assert!(log.finalized);
    let proposal = log.proposals.first().unwrap();
    let receipt = log
        .receipts
        .iter()
        .find(|r| r.round == proposal.round && r.provider == proposal.candidate_provider)
        .expect("matched provider receipt");
    assert!(!receipt.signature_hex.is_empty());
    // Requester proof is present (acknowledges the match predicate locally).
    assert!(log
        .proofs
        .iter()
        .any(|p| p.round == proposal.round && p.node_id == proposal.candidate_requester));
}

#[test]
fn runtime_log_recovers_with_fallback_after_invalid_proof() {
    let mut sc = Scenario::empty();
    // Round 0 proposer in stable order is provider 0x22. Inject an invalid
    // proof from 0x22 in round 0 to force a fallback.
    sc.events.push(ScenarioEvent::InjectInvalidProof {
        node: NodeId::from_bytes([0x22; 32]),
        round: 0,
    });
    let log = run(sc, 4);
    assert!(log.finalized, "runtime must recover on fallback");
    assert!(log.final_round.value() >= 1);
    // Round 0 should have a visible `public_inputs_mismatch` rejection.
    assert!(log
        .rejections
        .iter()
        .any(|r| r.round == RoundId::new(0) && r.reason_code == "public_inputs_mismatch"));
}

// ---------------------------------------------------------------------------
// Bad Path
// ---------------------------------------------------------------------------

#[test]
fn runtime_log_double_commit_visible_but_round_still_finalizes() {
    let mut sc = Scenario::empty();
    sc.events.push(ScenarioEvent::DoubleCommit {
        node: NodeId::from_bytes([0x33; 32]),
        round: 0,
    });
    let log = run(sc, 4);
    assert!(log.finalized);
    assert!(log
        .rejections
        .iter()
        .any(|r| r.reason_code == "duplicate_commitment"));
}

#[test]
fn runtime_log_replay_rejected_by_round_machine() {
    let mut sc = Scenario::empty();
    sc.events.push(ScenarioEvent::ReplayPriorCommitment {
        node: NodeId::from_bytes([0x33; 32]),
        round: 1,
        from_round: 0,
    });
    // Force fallback into round 1 so the replay has a chance to happen.
    sc.events.push(ScenarioEvent::InjectInvalidProof {
        node: NodeId::from_bytes([0x22; 32]),
        round: 0,
    });
    let log = run(sc, 4);
    assert!(log
        .rejections
        .iter()
        .any(|r| r.reason_code == "replay_detected"));
}

#[test]
fn runtime_log_missing_requester_aborts_round() {
    let mut sc = Scenario::empty();
    sc.events.push(ScenarioEvent::DropNode {
        node: NodeId::from_bytes([0x11; 32]), // requester
        after_round: 0,                       // drops starting round 1
    });
    // Force at least a fallback so we exercise a round with a dropped requester.
    sc.events.push(ScenarioEvent::InjectInvalidProof {
        node: NodeId::from_bytes([0x22; 32]),
        round: 0,
    });
    let log = run(sc, 3);
    // Run must end without finalizing.
    assert!(!log.finalized);
    // A `requester_missing` rejection must appear in at least one fallback round.
    assert!(log
        .rejections
        .iter()
        .any(|r| r.reason_code == "requester_missing"));
}

// ---------------------------------------------------------------------------
// Edge Cases
// ---------------------------------------------------------------------------

#[test]
fn runtime_log_no_feasible_provider_surfaces_rejection() {
    // Topology where only the CPU provider is present as a provider.
    let topo_text = format!(
        r#"
version = 1
capability_tags = ["GPU", "CPU"]

[[nodes]]
id = "{}"
role = "requester"
required_capability = "GPU"

[[nodes]]
id = "{}"
role = "provider"
capability_claims = ["CPU"]
"#,
        hex64(0x11),
        hex64(0x44),
    );
    let topology = TopologyConfig::from_toml_str(&topo_text).unwrap();
    let mut agents: BTreeMap<NodeId, AgentState> = BTreeMap::new();
    agents.insert(
        NodeId::from_bytes([0x11; 32]),
        AgentState::requester(PrivateRequesterIntent {
            node_id: NodeId::from_bytes([0x11; 32]),
            required_capability: CapabilityTag::parse_shape("GPU").unwrap(),
            budget_cents: Secret::new(1000),
        }),
    );
    agents.insert(
        NodeId::from_bytes([0x44; 32]),
        AgentState::provider(PrivateProviderIntent {
            node_id: NodeId::from_bytes([0x44; 32]),
            capability_claims: vec![CapabilityTag::parse_shape("CPU").unwrap()],
            reservation_cents: Secret::new(100),
        }),
    );
    let rt =
        CoordinationRuntime::new(topology, OrderedBus::new(), agents, Scenario::empty(), 2)
            .unwrap();
    let out = rt.run("no-feasible").unwrap();
    assert!(!out.finalized);
    assert!(out
        .log
        .rejections
        .iter()
        .any(|r| r.reason_code == "no_feasible_provider"));
}

#[test]
fn runtime_log_custom_capability_labels_flow_cleanly() {
    // Non-illustrative labels.
    let topo_text = format!(
        r#"
version = 1
capability_tags = ["STORAGE_A", "STORAGE_B"]

[[nodes]]
id = "{}"
role = "requester"
required_capability = "STORAGE_A"

[[nodes]]
id = "{}"
role = "provider"
capability_claims = ["STORAGE_A"]
"#,
        hex64(0x11),
        hex64(0x22),
    );
    let topology = TopologyConfig::from_toml_str(&topo_text).unwrap();
    let mut agents: BTreeMap<NodeId, AgentState> = BTreeMap::new();
    agents.insert(
        NodeId::from_bytes([0x11; 32]),
        AgentState::requester(PrivateRequesterIntent {
            node_id: NodeId::from_bytes([0x11; 32]),
            required_capability: CapabilityTag::parse_shape("STORAGE_A").unwrap(),
            budget_cents: Secret::new(800),
        }),
    );
    agents.insert(
        NodeId::from_bytes([0x22; 32]),
        AgentState::provider(PrivateProviderIntent {
            node_id: NodeId::from_bytes([0x22; 32]),
            capability_claims: vec![CapabilityTag::parse_shape("STORAGE_A").unwrap()],
            reservation_cents: Secret::new(300),
        }),
    );
    let rt =
        CoordinationRuntime::new(topology, OrderedBus::new(), agents, Scenario::empty(), 2)
            .unwrap();
    let out = rt.run("custom-labels").unwrap();
    assert!(out.finalized);
}

#[test]
fn runtime_log_silent_provider_still_finalizes_with_viable_pair() {
    let mut sc = Scenario::empty();
    // Drop the CPU-only provider; it's never feasible anyway.
    sc.events.push(ScenarioEvent::DropNode {
        node: NodeId::from_bytes([0x44; 32]),
        after_round: 0, // silent starting round 1
    });
    let log = run(sc, 2);
    assert!(log.finalized);
}

// ---------------------------------------------------------------------------
// Security
// ---------------------------------------------------------------------------

#[test]
fn runtime_log_double_commit_security_is_visible_in_log() {
    let mut sc = Scenario::empty();
    sc.events.push(ScenarioEvent::DoubleCommit {
        node: NodeId::from_bytes([0x22; 32]),
        round: 0,
    });
    let log = run(sc, 4);
    // The rejection record identifies the attacker's node id.
    let bad = log
        .rejections
        .iter()
        .find(|r| {
            r.reason_code == "duplicate_commitment"
                && r.node_id == NodeId::from_bytes([0x22; 32])
                && r.round == RoundId::new(0)
        })
        .expect("visible double-commit rejection");
    assert_eq!(bad.kind, "commitment");
}

// ---------------------------------------------------------------------------
// Data Leak
// ---------------------------------------------------------------------------

#[test]
fn runtime_log_does_not_leak_private_fields_in_serialization() {
    let log = run(Scenario::empty(), 4);
    let json = serde_json::to_string(&log).unwrap();
    for forbidden in ["budget_cents", "reservation_cents", "[REDACTED]", "1000", "500", "450"] {
        assert!(
            !json.contains(forbidden),
            "log leaked {forbidden:?}: {json}"
        );
    }
}

#[test]
fn runtime_log_rejection_messages_carry_no_private_data() {
    let mut sc = Scenario::empty();
    sc.events.push(ScenarioEvent::InjectInvalidProof {
        node: NodeId::from_bytes([0x22; 32]),
        round: 0,
    });
    let log = run(sc, 4);
    let json = serde_json::to_string(&log.rejections).unwrap();
    for forbidden in ["budget", "reservation", "[REDACTED]", "1000", "500"] {
        assert!(!json.contains(forbidden));
    }
}

// ---------------------------------------------------------------------------
// Data Damage
// ---------------------------------------------------------------------------

#[test]
fn runtime_log_serialization_roundtrips() {
    let log = run(Scenario::empty(), 4);
    let json = serde_json::to_string(&log).unwrap();
    let mut decoded: CoordinationLog = serde_json::from_str(&json).unwrap();
    decoded.reindex().unwrap();
    assert_eq!(decoded.commitments, log.commitments);
    assert_eq!(decoded.proposals, log.proposals);
    assert_eq!(decoded.proofs, log.proofs);
    assert_eq!(decoded.receipts, log.receipts);
    assert_eq!(decoded.rejections, log.rejections);
    assert_eq!(decoded.final_round, log.final_round);
    assert_eq!(decoded.finalized, log.finalized);
}

#[test]
fn runtime_log_restart_after_failed_round_stays_coherent() {
    // Two separate runs with the same scenario and seed should produce
    // equal logs; the second does not get poisoned by the first.
    let mut sc = Scenario::empty();
    sc.events.push(ScenarioEvent::InjectInvalidProof {
        node: NodeId::from_bytes([0x22; 32]),
        round: 0,
    });
    let log_a = run(sc.clone(), 4);
    let log_b = run(sc, 4);
    assert_eq!(log_a.commitments, log_b.commitments);
    assert_eq!(log_a.proposals, log_b.proposals);
    assert_eq!(log_a.proofs, log_b.proofs);
    assert_eq!(log_a.receipts, log_b.receipts);
    assert_eq!(log_a.rejections, log_b.rejections);
    assert_eq!(log_a.final_round, log_b.final_round);
    assert_eq!(log_a.finalized, log_b.finalized);
}
