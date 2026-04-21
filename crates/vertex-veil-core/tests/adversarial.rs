//! Phase 3 integration tests for adversarial scenarios.
//!
//! These tests exercise the scenario loader and the runtime's adversarial
//! recovery behavior end to end. The focus is on visible detection — every
//! adversarial event either drives a fallback round or leaves a rejection
//! record that the standalone verifier could inspect.
//!
//! Plan coverage:
//!
//! - Happy Path: fallback round after failed proof selects the next
//!   proposer; runtime supports a custom capability-label config while
//!   preserving the same protocol flow.
//! - Bad Path: invalid proof, replay, double-commit, drop beyond recovery.
//! - Edge Cases: no-match round does not crash; silent node within the
//!   baseline still finalizes or aborts verifiably.
//! - Security: replay + double-commit appear in the log's ordered trace.
//! - Data Leak: scenario fixture values never surface in the coordination
//!   log even when errors mention node identities.
//! - Data Damage: failed scenarios do not mutate shared fixture baselines;
//!   repeated runs are deterministic.

use std::collections::BTreeMap;

use vertex_veil_core::{
    AgentState, CapabilityTag, CoordinationRuntime, NodeId, OrderedBus,
    PrivateProviderIntent, PrivateRequesterIntent, RoundId, Scenario, ScenarioEvent, Secret,
    StandaloneVerifier, TopologyConfig,
};

fn hex64(b: u8) -> String {
    let mut s = String::new();
    for _ in 0..32 {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn topology_text() -> String {
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
            signing_secret_key: None,
        }),
    );
    for (b, cs, p) in [
        (0x22u8, vec!["GPU", "LLM"], 500u64),
        (0x33u8, vec!["GPU"], 450u64),
        (0x44u8, vec!["CPU"], 200u64),
    ] {
        m.insert(
            NodeId::from_bytes([b; 32]),
            AgentState::provider(PrivateProviderIntent {
                node_id: NodeId::from_bytes([b; 32]),
                capability_claims: cs
                    .into_iter()
                    .map(|c| CapabilityTag::parse_shape(c).unwrap())
                    .collect(),
                reservation_cents: Secret::new(p),
                signing_secret_key: None,
            }),
        );
    }
    m
}

fn run(scenario: Scenario, max_rounds: u64) -> (TopologyConfig, vertex_veil_core::CoordinationLog) {
    let topology = TopologyConfig::from_toml_str(&topology_text()).unwrap();
    let rt = CoordinationRuntime::new(
        topology.clone(),
        OrderedBus::new(),
        baseline_agents(),
        scenario,
        max_rounds,
    )
    .unwrap();
    (topology, rt.run("adversarial-test").unwrap().log)
}

// ---------------------------------------------------------------------------
// Happy Path
// ---------------------------------------------------------------------------

#[test]
fn adversarial_fallback_picks_next_proposer() {
    // Force round 0 to fail via invalid proof by the proposer. Fallback
    // advances to round 1 where a different provider proposes.
    let mut sc = Scenario::empty();
    sc.events.push(ScenarioEvent::InjectInvalidProof {
        node: NodeId::from_bytes([0x22; 32]), // proposer for round 0 in stable order
        round: 0,
    });
    let (_, log) = run(sc, 4);
    assert!(log.finalized);
    assert!(log.final_round.value() >= 1);
    // Proposer for round 1 is a different provider.
    let p0 = log
        .proposals
        .iter()
        .find(|p| p.round == RoundId::new(0))
        .map(|p| p.proposer);
    let p1 = log
        .proposals
        .iter()
        .find(|p| p.round == log.final_round)
        .expect("final proposal recorded")
        .proposer;
    if let Some(p0) = p0 {
        assert_ne!(p0, p1);
    }
}

#[test]
fn adversarial_combined_scenario_is_rejected_visibly_but_run_finishes() {
    let mut sc = Scenario::empty();
    sc.events.push(ScenarioEvent::DoubleCommit {
        node: NodeId::from_bytes([0x22; 32]),
        round: 0,
    });
    sc.events.push(ScenarioEvent::ReplayPriorCommitment {
        node: NodeId::from_bytes([0x33; 32]),
        round: 1,
        from_round: 0,
    });
    sc.events.push(ScenarioEvent::DropNode {
        node: NodeId::from_bytes([0x44; 32]),
        after_round: 0,
    });
    sc.events.push(ScenarioEvent::InjectInvalidProof {
        node: NodeId::from_bytes([0x22; 32]),
        round: 0,
    });
    let (topology, log) = run(sc, 4);
    assert!(log.finalized);
    // All four rejection kinds surface in the log.
    let codes: Vec<&str> = log
        .rejections
        .iter()
        .map(|r| r.reason_code.as_str())
        .collect();
    assert!(codes.contains(&"duplicate_commitment"));
    assert!(codes.contains(&"public_inputs_mismatch"));
    assert!(codes.contains(&"replay_detected"));
    assert!(codes.contains(&"node_dropped"));

    // Verifier accepts the final round as valid because the adversarial
    // events were rejected before they could corrupt the finalized round.
    let report = StandaloneVerifier::new(topology).verify_log(&log);
    assert!(report.valid, "report: {:?}", report.reasons);
}

// ---------------------------------------------------------------------------
// Bad Path
// ---------------------------------------------------------------------------

#[test]
fn adversarial_replay_prior_round_rejected_in_active_round() {
    // Force fallback into round 1 then replay round-0 commitment there.
    let mut sc = Scenario::empty();
    sc.events.push(ScenarioEvent::InjectInvalidProof {
        node: NodeId::from_bytes([0x22; 32]),
        round: 0,
    });
    sc.events.push(ScenarioEvent::ReplayPriorCommitment {
        node: NodeId::from_bytes([0x33; 32]),
        round: 1,
        from_round: 0,
    });
    let (_, log) = run(sc, 4);
    assert!(log
        .rejections
        .iter()
        .any(|r| r.reason_code == "replay_detected" && r.round == RoundId::new(1)));
}

#[test]
fn adversarial_double_commit_from_single_key_rejected() {
    let mut sc = Scenario::empty();
    sc.events.push(ScenarioEvent::DoubleCommit {
        node: NodeId::from_bytes([0x33; 32]),
        round: 0,
    });
    let (_, log) = run(sc, 4);
    assert!(log
        .rejections
        .iter()
        .any(|r| r.reason_code == "duplicate_commitment"
            && r.node_id == NodeId::from_bytes([0x33; 32])
            && r.round == RoundId::new(0)));
}

#[test]
fn adversarial_drop_beyond_recoverable_aborts_verifiably() {
    // Drop the requester after round 0 AND force a fallback by invalid proof.
    // There is no requester in round 1 → run cannot finalize.
    let mut sc = Scenario::empty();
    sc.events.push(ScenarioEvent::InjectInvalidProof {
        node: NodeId::from_bytes([0x22; 32]),
        round: 0,
    });
    sc.events.push(ScenarioEvent::DropNode {
        node: NodeId::from_bytes([0x11; 32]),
        after_round: 0,
    });
    let (_, log) = run(sc, 3);
    assert!(!log.finalized);
    // The abort leaves a coherent log with a visible reason.
    assert!(log
        .rejections
        .iter()
        .any(|r| r.reason_code == "requester_missing"));
}

// ---------------------------------------------------------------------------
// Edge Cases
// ---------------------------------------------------------------------------

#[test]
fn adversarial_no_match_round_does_not_crash() {
    // Topology with only a CPU provider but the requester wants GPU.
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
            signing_secret_key: None,
        }),
    );
    agents.insert(
        NodeId::from_bytes([0x44; 32]),
        AgentState::provider(PrivateProviderIntent {
            node_id: NodeId::from_bytes([0x44; 32]),
            capability_claims: vec![CapabilityTag::parse_shape("CPU").unwrap()],
            reservation_cents: Secret::new(100),
            signing_secret_key: None,
        }),
    );
    let rt = CoordinationRuntime::new(topology, OrderedBus::new(), agents, Scenario::empty(), 2)
        .unwrap();
    let out = rt.run("no-match").unwrap();
    // Runtime terminates cleanly and emits a readable log.
    assert!(!out.finalized);
    assert!(!out.log.rejections.is_empty());
}

#[test]
fn adversarial_silent_node_within_baseline_still_finalizes() {
    let mut sc = Scenario::empty();
    sc.events.push(ScenarioEvent::DropNode {
        node: NodeId::from_bytes([0x33; 32]),
        after_round: 0,
    });
    let (_, log) = run(sc, 4);
    assert!(log.finalized);
}

// ---------------------------------------------------------------------------
// Security
// ---------------------------------------------------------------------------

#[test]
fn adversarial_ordered_trace_preserves_detection_order() {
    let mut sc = Scenario::empty();
    sc.events.push(ScenarioEvent::DoubleCommit {
        node: NodeId::from_bytes([0x22; 32]),
        round: 0,
    });
    sc.events.push(ScenarioEvent::InjectInvalidProof {
        node: NodeId::from_bytes([0x22; 32]),
        round: 0,
    });
    let (_, log) = run(sc, 4);
    // Commitment-phase rejections appear before proof-phase rejections.
    let first_dup_idx = log
        .rejections
        .iter()
        .position(|r| r.reason_code == "duplicate_commitment")
        .unwrap();
    let first_proof_idx = log
        .rejections
        .iter()
        .position(|r| r.reason_code == "public_inputs_mismatch")
        .unwrap();
    assert!(first_dup_idx < first_proof_idx);
}

// ---------------------------------------------------------------------------
// Data Leak
// ---------------------------------------------------------------------------

#[test]
fn adversarial_fixtures_do_not_surface_in_log() {
    let mut sc = Scenario::empty();
    sc.events.push(ScenarioEvent::DoubleCommit {
        node: NodeId::from_bytes([0x22; 32]),
        round: 0,
    });
    sc.events.push(ScenarioEvent::InjectInvalidProof {
        node: NodeId::from_bytes([0x22; 32]),
        round: 0,
    });
    let (_, log) = run(sc, 4);
    let json = serde_json::to_string(&log).unwrap();
    for forbidden in ["budget_cents", "reservation_cents", "[REDACTED]", "500", "450"] {
        assert!(!json.contains(forbidden));
    }
}

// ---------------------------------------------------------------------------
// Data Damage
// ---------------------------------------------------------------------------

#[test]
fn adversarial_scenario_fixture_text_parses_from_disk_fixture() {
    let text = include_str!("../../../fixtures/replay-doublecommit-drop.toml");
    let sc = Scenario::from_toml_str(text).unwrap();
    assert!(!sc.events.is_empty());
}

#[test]
fn adversarial_run_is_deterministic() {
    let mut sc = Scenario::empty();
    sc.events.push(ScenarioEvent::DoubleCommit {
        node: NodeId::from_bytes([0x33; 32]),
        round: 0,
    });
    let (_, log_a) = run(sc.clone(), 4);
    let (_, log_b) = run(sc, 4);
    assert_eq!(log_a.commitments, log_b.commitments);
    assert_eq!(log_a.proposals, log_b.proposals);
    assert_eq!(log_a.proofs, log_b.proofs);
    assert_eq!(log_a.receipts, log_b.receipts);
    assert_eq!(log_a.rejections, log_b.rejections);
}
