//! Coordination runtime for Phase 3.
//!
//! The runtime drives a complete Vertex Veil round lifecycle: collect
//! commitments, elect a proposer, gather proof artifacts for the matched
//! pair, persist a completion receipt, and advance through fallback rounds
//! when anything in the lifecycle fails. It is the single authority
//! responsible for turning an ordered stream of [`CoordinationMessage`]s into
//! a [`crate::CoordinationLog`].
//!
//! # Transport
//!
//! The runtime is parameterized over [`CoordinationTransport`], a
//! narrow trait that captures the exact thing Vertex provides: consensus-
//! ordered broadcast of arbitrary byte-blob messages.
//!
//! - [`OrderedBus`] is the default implementation used by unit tests and by
//!   the Phase 3 single-process demo. It mirrors Vertex's consensus ordering
//!   guarantee within a single process by preserving the order messages are
//!   broadcast.
//! - A real `tashi-vertex` transport that wraps `Engine::send_transaction` /
//!   `Engine::recv_message` is a drop-in Phase 4 hardening step. No protocol
//!   logic depends on transport specifics.
//!
//! # Protocol loop
//!
//! One round is:
//!
//! 1. Each non-dropped agent publishes its public commitment. Injected
//!    double-commits and replays surface as rejections on the log.
//! 2. The proposer-for-round derives a candidate from the accepted
//!    commitments and publishes a proposal.
//! 3. The candidate requester and matched provider each publish a proof
//!    artifact. Injected tampered proofs are rejected and logged.
//! 4. The matched provider publishes a completion receipt; the round is
//!    finalized.
//!
//! Any failure in steps 2-4 advances to the next round (fallback).
//! The runtime records every rejection in [`crate::CoordinationLog::rejections`]
//! so the standalone verifier can reconstruct the adversarial trace from
//! public data alone.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use blake2::{digest::FixedOutput, Blake2s256, Digest};
use serde::{Deserialize, Serialize};

use crate::adversarial::{Scenario, ScenarioEvent};
use crate::artifacts::{
    CommitmentRecord, CompletionReceiptRecord, CoordinationLog, ProofArtifactRecord,
    ProposalRecord, RejectionRecord,
};
use crate::candidate::derive_candidate;
use crate::commitments::{commit_provider, commit_requester, derive_test_nonce};
use crate::config::{Role, TopologyConfig};
use crate::keys::NodeId;
use crate::predicate::match_predicate;
use crate::private_intent::{PrivateProviderIntent, PrivateRequesterIntent};
use crate::proposer::proposer_for_round;
use crate::round_machine::{RoundError, RoundMachine};
use crate::shared_types::{PublicIntent, RoundId};

/// Envelope for every coordination-bus message. Transport-agnostic.
///
/// The envelope carries the originating node id alongside the payload so the
/// runtime can apply origin-checks against the topology before mutating
/// state. (In a real Vertex deployment the creator is derived from the
/// transaction signer; the transport wrapper fills this in at receive time.)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoordinationMessage {
    pub origin: NodeId,
    pub payload: MessagePayload,
}

/// Payload variants broadcast on the coordination bus.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessagePayload {
    Commitment(CommitmentRecord),
    Proposal(ProposalRecord),
    Proof(ProofArtifactRecord),
    Receipt(CompletionReceiptRecord),
}

/// Errors returned by transport implementations.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TransportError {
    #[error("transport closed")]
    Closed,
}

/// Narrow transport abstraction that mirrors Vertex consensus-ordered
/// delivery. Implementations must preserve the order in which broadcasts
/// are submitted to [`broadcast`](Self::broadcast).
pub trait CoordinationTransport {
    fn broadcast(&mut self, msg: CoordinationMessage) -> Result<(), TransportError>;
    fn next_ordered(&mut self) -> Option<CoordinationMessage>;
    /// Hint for the runtime that no new events are coming before it drains
    /// the current round. Default no-op is fine for in-process transports.
    fn flush(&mut self) {}
}

/// In-process ordered broadcast bus. Preserves FIFO order across all
/// broadcasters, which matches Vertex's consensus-ordered delivery guarantee
/// for single-process Phase 3 runs.
#[derive(Debug, Default)]
pub struct OrderedBus {
    queue: VecDeque<CoordinationMessage>,
}

impl OrderedBus {
    pub fn new() -> Self {
        OrderedBus {
            queue: VecDeque::new(),
        }
    }
}

impl CoordinationTransport for OrderedBus {
    fn broadcast(&mut self, msg: CoordinationMessage) -> Result<(), TransportError> {
        self.queue.push_back(msg);
        Ok(())
    }
    fn next_ordered(&mut self) -> Option<CoordinationMessage> {
        self.queue.pop_front()
    }
}

/// Narratable runtime events. Implementations receive a callback for each
/// significant protocol milestone so the agent binary can map them to
/// human-readable stdout tags (`[VERTEX]`, `[COORD]`, `[ABORT]`) without the
/// runtime itself touching stdout. Default no-op implementations keep the
/// trait additive: existing call sites that do not install an observer are
/// unaffected.
pub trait RuntimeObserver: Send + Sync {
    fn on_round_committed(&self, _round: RoundId, _finalized: bool) {}
    fn on_commitment(&self, _node: NodeId, _round: RoundId) {}
    fn on_proposal(
        &self,
        _proposer: NodeId,
        _round: RoundId,
        _matched_capability: &str,
    ) {
    }
    fn on_proof_verified(&self, _node: NodeId, _round: RoundId) {}
    fn on_receipt(&self, _provider: NodeId, _round: RoundId) {}
    fn on_abort(&self, _reason: &str, _round: RoundId) {}
}

/// Default observer that drops every event. Used when the runtime is
/// constructed without an explicit observer (the Phase 3/4 in-process path).
#[derive(Debug, Default)]
pub struct NoopObserver;
impl RuntimeObserver for NoopObserver {}

/// Per-agent state held by the runtime so it can generate commitments and
/// proofs on behalf of each configured node.
#[derive(Clone, Debug)]
pub struct AgentState {
    pub node_id: NodeId,
    pub role: Role,
    pub requester: Option<PrivateRequesterIntent>,
    pub provider: Option<PrivateProviderIntent>,
}

impl AgentState {
    pub fn requester(intent: PrivateRequesterIntent) -> Self {
        AgentState {
            node_id: intent.node_id,
            role: Role::Requester,
            requester: Some(intent),
            provider: None,
        }
    }

    pub fn provider(intent: PrivateProviderIntent) -> Self {
        AgentState {
            node_id: intent.node_id,
            role: Role::Provider,
            requester: None,
            provider: Some(intent),
        }
    }
}

/// Errors that abort a runtime run outright. Per-round rejections are NOT
/// errors; they are logged and drive fallback rounds.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RuntimeError {
    #[error("topology must contain exactly one requester")]
    NoRequester,
    #[error("no providers configured")]
    NoProviders,
    #[error("missing private intent for node {0}")]
    MissingPrivateIntent(String),
    #[error("round state machine error: {0}")]
    Round(#[from] RoundError),
    #[error("proposer rotation error: {0}")]
    Proposer(#[from] crate::proposer::ProposerError),
    #[error("commitment helper rejected input: {0}")]
    Commitment(#[from] crate::commitments::CommitmentError),
    #[error("exceeded max rounds ({0}) without finalizing or verifiably aborting")]
    ExceededMaxRounds(u64),
    #[error("transport error: {0}")]
    Transport(#[from] TransportError),
}

/// Outcome of a runtime run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeOutcome {
    pub log: CoordinationLog,
    pub finalized: bool,
    pub final_round: RoundId,
}

/// Coordination runtime, parameterized over the transport.
pub struct CoordinationRuntime<T: CoordinationTransport> {
    topology: TopologyConfig,
    transport: T,
    log: CoordinationLog,
    agents: BTreeMap<NodeId, AgentState>,
    round_machine: RoundMachine,
    scenario: Scenario,
    /// `(node_id, round)` pairs where an invalid proof injection applies.
    invalid_proof_schedule: BTreeSet<(NodeId, RoundId)>,
    /// Historical commitments (hex) per (node, original_round). Used to
    /// replay a prior commitment when the scenario asks for it.
    historical_commitments: BTreeMap<(NodeId, RoundId), CommitmentRecord>,
    max_rounds: u64,
    /// Per-run salt so commitments vary between tests; also makes
    /// nonces deterministic for a given (run_id, node_id, round) triple.
    run_salt: [u8; 32],
    /// Observer receives per-protocol-milestone callbacks so the agent
    /// binary can render narratable stdout tags. Default: no-op.
    observer: Box<dyn RuntimeObserver>,
}

impl<T: CoordinationTransport> CoordinationRuntime<T> {
    pub fn new(
        topology: TopologyConfig,
        transport: T,
        agents: BTreeMap<NodeId, AgentState>,
        scenario: Scenario,
        max_rounds: u64,
    ) -> Result<Self, RuntimeError> {
        if topology
            .nodes
            .iter()
            .filter(|n| n.role == Role::Requester)
            .count()
            != 1
        {
            return Err(RuntimeError::NoRequester);
        }
        if topology
            .nodes
            .iter()
            .filter(|n| n.role == Role::Provider)
            .count()
            == 0
        {
            return Err(RuntimeError::NoProviders);
        }
        // In-process demo: every topology node has a local agent.
        // Multi-process `node` subcommand: only this process's own agent is
        // present, and we accept that as long as every supplied agent
        // matches a topology node (reverse direction of the check still
        // holds).
        for id in agents.keys() {
            if !topology.nodes.iter().any(|n| &n.id == id) {
                return Err(RuntimeError::MissingPrivateIntent(id.to_hex()));
            }
        }
        if agents.is_empty() {
            return Err(RuntimeError::MissingPrivateIntent(
                "at least one local agent required".into(),
            ));
        }

        let invalid_proof_schedule = scenario
            .events
            .iter()
            .filter_map(|e| match e {
                ScenarioEvent::InjectInvalidProof { node, round } => {
                    Some((*node, RoundId::new(*round)))
                }
                _ => None,
            })
            .collect();

        let round_machine = RoundMachine::new(topology.clone(), RoundId::new(0));
        Ok(CoordinationRuntime {
            topology,
            transport,
            log: CoordinationLog::new("run-pending"),
            agents,
            round_machine,
            scenario,
            invalid_proof_schedule,
            historical_commitments: BTreeMap::new(),
            max_rounds,
            run_salt: [0x5au8; 32],
            observer: Box::new(NoopObserver),
        })
    }

    /// Set a deterministic salt for nonce derivation. Tests can pin this so
    /// commitments stay reproducible across runs.
    pub fn with_run_salt(mut self, salt: [u8; 32]) -> Self {
        self.run_salt = salt;
        self
    }

    /// Install a [`RuntimeObserver`] that receives per-event callbacks. The
    /// default observer is a no-op — call this from the `node` subcommand
    /// to render narratable stdout tags.
    pub fn with_observer(mut self, observer: Box<dyn RuntimeObserver>) -> Self {
        self.observer = observer;
        self
    }

    /// Run the protocol loop until a round finalizes, the run aborts
    /// verifiably, or the max-round bound is exceeded.
    pub fn run(mut self, run_id: impl Into<String>) -> Result<RuntimeOutcome, RuntimeError> {
        self.log = CoordinationLog::new(run_id);

        let mut finalized = false;
        let mut final_round = RoundId::new(0);
        for _ in 0..self.max_rounds {
            let round = self.round_machine.current_round();
            final_round = round;
            let outcome = self.run_one_round(round)?;
            self.observer
                .on_round_committed(round, matches!(outcome, RoundOutcome::Finalized));
            match outcome {
                RoundOutcome::Finalized => {
                    finalized = true;
                    break;
                }
                RoundOutcome::Fallback => {
                    self.round_machine.advance_fallback()?;
                }
            }
        }

        self.log.set_final_round(final_round, finalized);
        if !finalized {
            self.log.set_abort_reason("max_rounds_exceeded");
            self.observer
                .on_abort("max_rounds_exceeded", final_round);
        }
        Ok(RuntimeOutcome {
            log: self.log,
            finalized,
            final_round,
        })
    }

    /// Run one bounded coordination session and return both the outcome and
    /// the underlying transport so callers can reuse a live transport across
    /// multiple sessions.
    pub fn run_with_transport(
        mut self,
        run_id: impl Into<String>,
    ) -> Result<(RuntimeOutcome, T), RuntimeError> {
        self.log = CoordinationLog::new(run_id);

        let mut finalized = false;
        let mut final_round = RoundId::new(0);
        for _ in 0..self.max_rounds {
            let round = self.round_machine.current_round();
            final_round = round;
            let outcome = self.run_one_round(round)?;
            self.observer
                .on_round_committed(round, matches!(outcome, RoundOutcome::Finalized));
            match outcome {
                RoundOutcome::Finalized => {
                    finalized = true;
                    break;
                }
                RoundOutcome::Fallback => {
                    self.round_machine.advance_fallback()?;
                }
            }
        }

        self.log.set_final_round(final_round, finalized);
        if !finalized {
            self.log.set_abort_reason("max_rounds_exceeded");
            self.observer
                .on_abort("max_rounds_exceeded", final_round);
        }
        let outcome = RuntimeOutcome {
            log: self.log,
            finalized,
            final_round,
        };
        Ok((outcome, self.transport))
    }

    /// Consume the runtime and return the underlying transport.
    ///
    /// This is used by long-lived drivers that want to execute multiple
    /// bounded coordination sessions over the same transport instance.
    pub fn into_transport(self) -> T {
        self.transport
    }

    fn run_one_round(&mut self, round: RoundId) -> Result<RoundOutcome, RuntimeError> {
        // Step 1: each non-dropped agent broadcasts its commitment (and any
        // adversarial variations scheduled for this round).
        self.broadcast_commitments(round)?;
        self.transport.flush();
        self.drain_commitments(round);

        // If no requester commitment was accepted (e.g. requester dropped),
        // the round cannot form a candidate and must fall back.
        let requester_id = self.topology.requester().id;
        if !self
            .round_machine
            .commitments()
            .contains_key(&requester_id)
        {
            self.log.append_rejection(RejectionRecord {
                round,
                node_id: requester_id,
                kind: "round".into(),
                reason_code: "requester_missing".into(),
            });
            return Ok(RoundOutcome::Fallback);
        }

        // Step 2: proposer derives a candidate from the accepted commitments
        // and broadcasts a proposal.
        if !self.broadcast_proposal(round)? {
            return Ok(RoundOutcome::Fallback);
        }
        self.transport.flush();
        self.drain_proposal(round);

        // If no proposal landed on the log in this round, fall back.
        let proposal = match self
            .log
            .proposals
            .iter()
            .rev()
            .find(|p| p.round == round)
            .cloned()
        {
            Some(p) => p,
            None => {
                return Ok(RoundOutcome::Fallback);
            }
        };

        // Step 3: matched requester + provider produce proof artifacts.
        self.broadcast_proofs(round, &proposal)?;
        self.transport.flush();
        self.drain_proofs(round);

        // All participants in the proposal must have an accepted proof.
        let has_req_proof = self.log.proofs.iter().any(|p| {
            p.round == round && p.node_id == proposal.candidate_requester
        });
        let has_prov_proof = self.log.proofs.iter().any(|p| {
            p.round == round && p.node_id == proposal.candidate_provider
        });
        if !(has_req_proof && has_prov_proof) {
            return Ok(RoundOutcome::Fallback);
        }

        // Step 4: provider emits a completion receipt; we use a synthetic
        // deterministic signature derived from the public proof artifacts so
        // the verifier can re-check it without holding private keys. A real
        // deployment plugs in ed25519 here.
        self.broadcast_receipt(round, &proposal)?;
        self.transport.flush();
        self.drain_receipts(round);

        Ok(RoundOutcome::Finalized)
    }

    fn broadcast_commitments(&mut self, round: RoundId) -> Result<(), RuntimeError> {
        // Snapshot: stable-order iteration over agents. Adversarial events run
        // first so they are ordered ahead of the legitimate broadcast — this
        // mirrors a real attacker who races the honest node, and exercises
        // the replay check before the duplicate-commitment check fires.
        let node_ids: Vec<NodeId> = self.agents.keys().copied().collect();
        for node_id in node_ids {
            // Adversarial events broadcast even when the node is silent on
            // the legitimate path, since a real attacker can still forge a
            // message claiming to be from that key.
            for ev in self
                .scenario
                .events_for_round(node_id, round.value())
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
            {
                if let ScenarioEvent::ReplayPriorCommitment {
                    node,
                    round: _,
                    from_round,
                } = ev
                {
                    // Resurrect an older commitment hex and re-label it for
                    // the current round. The replay check rejects it.
                    if let Some(prior) = self
                        .historical_commitments
                        .get(&(node, RoundId::new(from_round)))
                        .cloned()
                    {
                        let replayed = CommitmentRecord {
                            node_id: prior.node_id,
                            round,
                            commitment_hex: prior.commitment_hex.clone(),
                            public_intent: update_intent_round(
                                prior.public_intent.clone(),
                                round,
                            ),
                        };
                        self.transport.broadcast(CoordinationMessage {
                            origin: node,
                            payload: MessagePayload::Commitment(replayed),
                        })?;
                    }
                }
            }

            if self.scenario.has_dropped(node_id, round.value()) {
                // Dropped nodes contribute nothing on the legitimate path.
                // Record the silent-node case on the log so the verifier
                // can reconstruct the topology participation trace.
                self.log.append_rejection(RejectionRecord {
                    round,
                    node_id,
                    kind: "commitment".into(),
                    reason_code: "node_dropped".into(),
                });
                continue;
            }

            let base_record = self.build_commitment(node_id, round)?;
            self.historical_commitments
                .insert((node_id, round), base_record.clone());
            self.transport.broadcast(CoordinationMessage {
                origin: node_id,
                payload: MessagePayload::Commitment(base_record),
            })?;

            // Double-commit is broadcast after the legitimate commit so the
            // runtime sees the duplicate as a second attempt — the round
            // machine rejects the second one with `DuplicateCommitment`.
            for ev in self
                .scenario
                .events_for_round(node_id, round.value())
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
            {
                if let ScenarioEvent::DoubleCommit { node, round: r } = ev {
                    let rec = self
                        .historical_commitments
                        .get(&(node, RoundId::new(r)))
                        .cloned();
                    if let Some(rec) = rec {
                        self.transport.broadcast(CoordinationMessage {
                            origin: node,
                            payload: MessagePayload::Commitment(rec),
                        })?;
                    }
                }
            }
        }
        Ok(())
    }

    fn drain_commitments(&mut self, round: RoundId) {
        while let Some(msg) = self.transport.next_ordered() {
            let MessagePayload::Commitment(rec) = msg.payload else {
                // Out-of-phase message; push back by re-broadcasting is not
                // supported on our minimal transport, so just log it as a
                // rejection so the trace stays complete.
                self.log.append_rejection(RejectionRecord {
                    round,
                    node_id: msg.origin,
                    kind: payload_label(&msg.payload).into(),
                    reason_code: "out_of_phase".into(),
                });
                continue;
            };

            if msg.origin != rec.node_id {
                self.log.append_rejection(RejectionRecord {
                    round,
                    node_id: msg.origin,
                    kind: "commitment".into(),
                    reason_code: "origin_mismatch".into(),
                });
                continue;
            }

            match self.round_machine.accept_commitment(rec.clone()) {
                Ok(()) => {
                    // Persist to the coordination log. We ignore the dup
                    // error here because the round machine already rejected
                    // the dup above; but in the off-chance the log has a
                    // separate disagreement, we log the rejection.
                    if let Err(_e) = self.log.append_commitment(rec.clone()) {
                        self.log.append_rejection(RejectionRecord {
                            round,
                            node_id: rec.node_id,
                            kind: "commitment".into(),
                            reason_code: "log_duplicate".into(),
                        });
                    } else {
                        self.observer.on_commitment(rec.node_id, round);
                    }
                }
                Err(err) => {
                    self.log.append_rejection(RejectionRecord {
                        round,
                        node_id: rec.node_id,
                        kind: "commitment".into(),
                        reason_code: round_error_code(&err).into(),
                    });
                }
            }
        }
    }

    fn broadcast_proposal(&mut self, round: RoundId) -> Result<bool, RuntimeError> {
        let proposer = self.round_machine.current_proposer()?;
        // In a multi-process run each node only speaks for itself. Only the
        // process that actually *is* the proposer emits the proposal message.
        // A single-process demo with all agents in `self.agents` emits it
        // from whichever agent happens to be elected — same behaviour as
        // before.
        if !self.agents.contains_key(&proposer) {
            return Ok(true);
        }
        let req_id = self.topology.requester().id;
        let req_commit = match self
            .round_machine
            .commitments()
            .get(&req_id)
            .cloned()
        {
            Some(c) => c,
            None => {
                self.log.append_rejection(RejectionRecord {
                    round,
                    node_id: proposer,
                    kind: "proposal".into(),
                    reason_code: "requester_missing".into(),
                });
                return Ok(false);
            }
        };
        let providers_snapshot: Vec<CommitmentRecord> = self
            .round_machine
            .commitments()
            .iter()
            .filter_map(|(nid, rec)| {
                if *nid == req_id {
                    None
                } else {
                    Some(rec.clone())
                }
            })
            .collect();

        let candidate = match derive_candidate(round, &req_commit, &providers_snapshot) {
            Ok(Some(c)) => c,
            Ok(None) => {
                // No feasible provider in this round. Log and fall back.
                self.log.append_rejection(RejectionRecord {
                    round,
                    node_id: proposer,
                    kind: "proposal".into(),
                    reason_code: "no_feasible_provider".into(),
                });
                return Ok(false);
            }
            Err(e) => {
                self.log.append_rejection(RejectionRecord {
                    round,
                    node_id: proposer,
                    kind: "proposal".into(),
                    reason_code: format!("candidate_error:{:?}", e),
                });
                return Ok(false);
            }
        };

        let proposal = ProposalRecord {
            proposer,
            round,
            candidate_requester: candidate.requester,
            candidate_provider: candidate.provider,
            matched_capability: candidate.matched_capability,
        };
        self.transport.broadcast(CoordinationMessage {
            origin: proposer,
            payload: MessagePayload::Proposal(proposal),
        })?;
        Ok(true)
    }

    fn drain_proposal(&mut self, round: RoundId) {
        while let Some(msg) = self.transport.next_ordered() {
            let MessagePayload::Proposal(p) = msg.payload else {
                self.log.append_rejection(RejectionRecord {
                    round,
                    node_id: msg.origin,
                    kind: payload_label(&msg.payload).into(),
                    reason_code: "out_of_phase".into(),
                });
                continue;
            };
            if msg.origin != p.proposer {
                self.log.append_rejection(RejectionRecord {
                    round,
                    node_id: msg.origin,
                    kind: "proposal".into(),
                    reason_code: "origin_mismatch".into(),
                });
                continue;
            }
            match self.round_machine.accept_proposal(p.clone()) {
                Ok(()) => {
                    let cap = p.matched_capability.to_string();
                    let proposer = p.proposer;
                    self.log.append_proposal(p);
                    self.observer.on_proposal(proposer, round, &cap);
                }
                Err(err) => self.log.append_rejection(RejectionRecord {
                    round,
                    node_id: p.proposer,
                    kind: "proposal".into(),
                    reason_code: round_error_code(&err).into(),
                }),
            }
        }
    }

    fn broadcast_proofs(
        &mut self,
        round: RoundId,
        proposal: &ProposalRecord,
    ) -> Result<(), RuntimeError> {
        for participant in [proposal.candidate_requester, proposal.candidate_provider] {
            // Multi-process guard: only the node whose agent state this
            // process holds emits its own proof.
            if !self.agents.contains_key(&participant) {
                continue;
            }
            let commitment = self
                .round_machine
                .commitments()
                .get(&participant)
                .cloned()
                .expect("proposal validated against commitment set");

            // Build the public proof artifact.
            let mut artifact =
                build_proof_artifact(&commitment, participant, round);

            // Adversarial injection: corrupt the public inputs so their
            // embedded commitment hash no longer matches the logged one.
            if self
                .invalid_proof_schedule
                .contains(&(participant, round))
            {
                artifact.public_inputs_hex = tamper_public_inputs(&artifact.public_inputs_hex);
            }

            self.transport.broadcast(CoordinationMessage {
                origin: participant,
                payload: MessagePayload::Proof(artifact),
            })?;
        }
        Ok(())
    }

    fn drain_proofs(&mut self, round: RoundId) {
        while let Some(msg) = self.transport.next_ordered() {
            let MessagePayload::Proof(rec) = msg.payload else {
                self.log.append_rejection(RejectionRecord {
                    round,
                    node_id: msg.origin,
                    kind: payload_label(&msg.payload).into(),
                    reason_code: "out_of_phase".into(),
                });
                continue;
            };
            if msg.origin != rec.node_id {
                self.log.append_rejection(RejectionRecord {
                    round,
                    node_id: msg.origin,
                    kind: "proof".into(),
                    reason_code: "origin_mismatch".into(),
                });
                continue;
            }
            if rec.round != round {
                self.log.append_rejection(RejectionRecord {
                    round,
                    node_id: rec.node_id,
                    kind: "proof".into(),
                    reason_code: "wrong_round".into(),
                });
                continue;
            }
            // Must reference an accepted commitment for (node_id, round).
            let expected_commit = self
                .round_machine
                .commitments()
                .get(&rec.node_id)
                .map(|c| c.commitment_hex.clone());
            let embedded_commit = extract_commitment_hash_hex(&rec.public_inputs_hex);
            if expected_commit.is_none() {
                self.log.append_rejection(RejectionRecord {
                    round,
                    node_id: rec.node_id,
                    kind: "proof".into(),
                    reason_code: "unknown_commitment".into(),
                });
                continue;
            }
            if embedded_commit != expected_commit {
                self.log.append_rejection(RejectionRecord {
                    round,
                    node_id: rec.node_id,
                    kind: "proof".into(),
                    reason_code: "public_inputs_mismatch".into(),
                });
                continue;
            }
            let node_id = rec.node_id;
            self.log.append_proof(rec);
            self.observer.on_proof_verified(node_id, round);
        }
    }

    fn broadcast_receipt(
        &mut self,
        round: RoundId,
        proposal: &ProposalRecord,
    ) -> Result<(), RuntimeError> {
        let provider = proposal.candidate_provider;
        // Multi-process guard: only the matched-provider's own process
        // signs and broadcasts the completion receipt.
        if !self.agents.contains_key(&provider) {
            return Ok(());
        }
        let capability = proposal.matched_capability.to_string();

        // Prefer ed25519 when the provider has a signing seed configured.
        // Fall back to the Phase 3 deterministic blake2s tag so legacy
        // fixtures that predate Phase 4 keep working.
        let signature_hex = match self
            .agents
            .get(&provider)
            .and_then(|a| a.provider.as_ref())
            .and_then(|i| i.signing_secret_key.as_ref())
        {
            Some(seed_secret) => {
                let sig = crate::signing::sign_receipt_ed25519(
                    seed_secret.expose(),
                    provider,
                    round,
                    &capability,
                );
                hex::encode(sig)
            }
            None => hex::encode(crate::signing::legacy_signature(
                provider,
                round,
                &capability,
            )),
        };

        let receipt = CompletionReceiptRecord {
            provider,
            round,
            signature_hex,
        };
        self.transport.broadcast(CoordinationMessage {
            origin: provider,
            payload: MessagePayload::Receipt(receipt),
        })?;
        Ok(())
    }

    fn drain_receipts(&mut self, round: RoundId) {
        while let Some(msg) = self.transport.next_ordered() {
            let MessagePayload::Receipt(r) = msg.payload else {
                self.log.append_rejection(RejectionRecord {
                    round,
                    node_id: msg.origin,
                    kind: payload_label(&msg.payload).into(),
                    reason_code: "out_of_phase".into(),
                });
                continue;
            };
            if msg.origin != r.provider {
                self.log.append_rejection(RejectionRecord {
                    round,
                    node_id: msg.origin,
                    kind: "receipt".into(),
                    reason_code: "origin_mismatch".into(),
                });
                continue;
            }
            if r.round != round {
                self.log.append_rejection(RejectionRecord {
                    round,
                    node_id: r.provider,
                    kind: "receipt".into(),
                    reason_code: "wrong_round".into(),
                });
                continue;
            }
            if r.signature_hex.is_empty() {
                self.log.append_rejection(RejectionRecord {
                    round,
                    node_id: r.provider,
                    kind: "receipt".into(),
                    reason_code: "missing_signature".into(),
                });
                continue;
            }
            let provider = r.provider;
            self.log.append_receipt(r);
            self.observer.on_receipt(provider, round);
        }
    }

    fn build_commitment(
        &self,
        node_id: NodeId,
        round: RoundId,
    ) -> Result<CommitmentRecord, RuntimeError> {
        let agent = self
            .agents
            .get(&node_id)
            .ok_or_else(|| RuntimeError::MissingPrivateIntent(node_id.to_hex()))?;

        let nonce = derive_test_nonce(node_id, round, &self.run_salt);

        let node_cfg = self
            .topology
            .nodes
            .iter()
            .find(|n| n.id == node_id)
            .expect("agents validated against topology");

        let (commitment_hex, public_intent) = match agent.role {
            Role::Requester => {
                let intent = agent.requester.as_ref().ok_or_else(|| {
                    RuntimeError::MissingPrivateIntent(format!(
                        "requester-private for {}",
                        node_id.to_hex()
                    ))
                })?;
                let commit = commit_requester(intent, &nonce, round)?;
                let required = node_cfg
                    .required_capability
                    .clone()
                    .expect("topology requester has required_capability");
                let public = PublicIntent::Requester {
                    node_id,
                    round,
                    required_capability: required,
                };
                (commit.to_hex(), public)
            }
            Role::Provider => {
                let intent = agent.provider.as_ref().ok_or_else(|| {
                    RuntimeError::MissingPrivateIntent(format!(
                        "provider-private for {}",
                        node_id.to_hex()
                    ))
                })?;
                let commit = commit_provider(intent, &nonce, round)?;
                let public = PublicIntent::Provider {
                    node_id,
                    round,
                    capability_claims: node_cfg.capability_claims.clone(),
                };
                (commit.to_hex(), public)
            }
        };

        Ok(CommitmentRecord {
            node_id,
            round,
            commitment_hex,
            public_intent,
        })
    }
}

fn update_intent_round(intent: PublicIntent, new_round: RoundId) -> PublicIntent {
    match intent {
        PublicIntent::Requester {
            node_id,
            required_capability,
            ..
        } => PublicIntent::Requester {
            node_id,
            round: new_round,
            required_capability,
        },
        PublicIntent::Provider {
            node_id,
            capability_claims,
            ..
        } => PublicIntent::Provider {
            node_id,
            round: new_round,
            capability_claims,
        },
    }
}

enum RoundOutcome {
    Finalized,
    Fallback,
}

/// Canonical public-input byte layout embedded inside [`ProofArtifactRecord::public_inputs_hex`].
///
/// ```text
/// [0..8)    u64 BE round
/// [8..40)   [u8;32] node_id
/// [40..72)  [u8;32] commitment_hash
/// [72]      u8      role (0 = requester, 1 = provider)
/// ```
///
/// The verifier and the runtime share this layout. It carries only public
/// inputs; there is no private witness material anywhere in the artifact.
pub const PROOF_PUBLIC_INPUTS_LEN: usize = 8 + 32 + 32 + 1;

fn build_proof_artifact(
    commitment: &CommitmentRecord,
    node_id: NodeId,
    round: RoundId,
) -> ProofArtifactRecord {
    let commit_bytes = hex::decode(&commitment.commitment_hex).unwrap_or_else(|_| vec![0u8; 32]);
    let mut buf = [0u8; PROOF_PUBLIC_INPUTS_LEN];
    buf[0..8].copy_from_slice(&round.value().to_be_bytes());
    buf[8..40].copy_from_slice(node_id.as_bytes());
    // commitment_hash; pad with zeros if decode was short
    let commit_len = commit_bytes.len().min(32);
    buf[40..40 + commit_len].copy_from_slice(&commit_bytes[..commit_len]);
    buf[72] = match commitment.public_intent {
        PublicIntent::Requester { .. } => 0,
        PublicIntent::Provider { .. } => 1,
    };

    // The default (execute-only) path uses a local constraint-validation
    // attestation as the "proof". The attestation is a blake2s-256 tag over
    // the public inputs plus a domain separator. The real UltraHonk bytes
    // replace this when the `barretenberg` feature is enabled on
    // `vertex-veil-noir`.
    let mut h = Blake2s256::new();
    h.update(b"vertex-veil/v1/proof-attest");
    h.update(buf);
    let attest = h.finalize_fixed();
    let mut proof = Vec::with_capacity(1 + 32);
    proof.push(1u8); // "ACIR-execute-ok" marker
    proof.extend_from_slice(attest.as_slice());

    ProofArtifactRecord {
        node_id,
        round,
        public_inputs_hex: hex::encode(buf),
        proof_hex: hex::encode(proof),
    }
}

fn extract_commitment_hash_hex(public_inputs_hex: &str) -> Option<String> {
    let bytes = hex::decode(public_inputs_hex).ok()?;
    if bytes.len() < 72 {
        return None;
    }
    Some(hex::encode(&bytes[40..72]))
}

fn tamper_public_inputs(hex: &str) -> String {
    // Flip the top byte of the commitment-hash region so the verifier's
    // equality check against the logged commitment fails. Stays public.
    let mut bytes = match hex::decode(hex) {
        Ok(v) => v,
        Err(_) => return hex.to_string(),
    };
    if bytes.len() < 41 {
        return hex.to_string();
    }
    bytes[40] ^= 0x01;
    hex::encode(bytes)
}

/// Verifier-facing: recompute the expected legacy (Phase 3) receipt
/// signature tag. Only used when a topology has no `signing_public_key`;
/// Phase 4 fixtures populate that field and the ed25519 path kicks in.
pub fn expected_signature_hex(provider: NodeId, round: RoundId, capability: &str) -> String {
    hex::encode(crate::signing::legacy_signature(provider, round, capability))
}

fn round_error_code(err: &RoundError) -> &'static str {
    match err {
        RoundError::RoundMismatch { .. } => "round_mismatch",
        RoundError::DuplicateCommitment { .. } => "duplicate_commitment",
        RoundError::ReplayDetected => "replay_detected",
        RoundError::UnknownNode => "unknown_node",
        RoundError::RoleMismatch => "role_mismatch",
        RoundError::BackwardTransition => "backward_transition",
        RoundError::Proposer(_) => "proposer_error",
        RoundError::WrongProposer => "wrong_proposer",
        RoundError::UnknownRequesterCommitment => "unknown_requester_commitment",
        RoundError::UnknownProviderCommitment => "unknown_provider_commitment",
        RoundError::TamperedProposal(_) => "tampered_proposal",
        RoundError::AlreadyFinalized => "already_finalized",
    }
}

fn payload_label(p: &MessagePayload) -> &'static str {
    match p {
        MessagePayload::Commitment(_) => "commitment",
        MessagePayload::Proposal(_) => "proposal",
        MessagePayload::Proof(_) => "proof",
        MessagePayload::Receipt(_) => "receipt",
    }
}

/// Return the deterministic proposer rotation for the topology as a provider
/// id vector in stable-key order. Verifiers use this to re-check proposer
/// identity for each round in the log.
pub fn deterministic_proposer(
    topology: &TopologyConfig,
    round: RoundId,
) -> Result<NodeId, RuntimeError> {
    let providers: Vec<NodeId> = topology
        .providers_stable_order()
        .into_iter()
        .map(|n| n.id)
        .collect();
    let p = proposer_for_round(round, &providers)?;
    Ok(p)
}

// Re-export a few helpers that the verifier module needs to share the
// canonical public-input layout.
pub(crate) fn parse_public_inputs(
    public_inputs_hex: &str,
) -> Option<(RoundId, NodeId, [u8; 32], u8)> {
    let bytes = hex::decode(public_inputs_hex).ok()?;
    if bytes.len() != PROOF_PUBLIC_INPUTS_LEN {
        return None;
    }
    let mut round_bytes = [0u8; 8];
    round_bytes.copy_from_slice(&bytes[0..8]);
    let round = RoundId::new(u64::from_be_bytes(round_bytes));
    let mut node_bytes = [0u8; 32];
    node_bytes.copy_from_slice(&bytes[8..40]);
    let mut commit_bytes = [0u8; 32];
    commit_bytes.copy_from_slice(&bytes[40..72]);
    let role = bytes[72];
    Some((round, NodeId::from_bytes(node_bytes), commit_bytes, role))
}

pub(crate) fn predicate_holds_for_log(
    log: &CoordinationLog,
    round: RoundId,
    requester: NodeId,
    provider: NodeId,
) -> Result<(), &'static str> {
    let req = log
        .commitments
        .iter()
        .find(|c| c.node_id == requester && c.round == round)
        .ok_or("requester_commitment_missing")?;
    let prov = log
        .commitments
        .iter()
        .find(|c| c.node_id == provider && c.round == round)
        .ok_or("provider_commitment_missing")?;
    match_predicate(&req.public_intent, &prov.public_intent, round)
        .map_err(|d| match d {
            crate::predicate::PredicateDenial::ProviderLacksCapability => {
                "provider_lacks_capability"
            }
            crate::predicate::PredicateDenial::WrongRequesterRole => "wrong_requester_role",
            crate::predicate::PredicateDenial::WrongProviderRole => "wrong_provider_role",
            crate::predicate::PredicateDenial::RequesterRoundMismatch => "requester_round_mismatch",
            crate::predicate::PredicateDenial::ProviderRoundMismatch => "provider_round_mismatch",
            crate::predicate::PredicateDenial::RequesterProviderRoundMismatch => {
                "requester_provider_round_mismatch"
            }
            crate::predicate::PredicateDenial::RequesterIdentityMismatch => {
                "requester_identity_mismatch"
            }
            crate::predicate::PredicateDenial::ProviderIdentityMismatch => {
                "provider_identity_mismatch"
            }
            crate::predicate::PredicateDenial::CapabilityAnnotationMismatch => {
                "capability_annotation_mismatch"
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::CapabilityTag;
    use crate::private_intent::{PrivateProviderIntent, PrivateRequesterIntent, Secret};

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

    fn sample_agents() -> BTreeMap<NodeId, AgentState> {
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
        for (byte, claims, price) in [
            (0x22u8, vec!["GPU", "LLM"], 500u64),
            (0x33u8, vec!["GPU"], 450u64),
            (0x44u8, vec!["CPU"], 200u64),
        ] {
            m.insert(
                NodeId::from_bytes([byte; 32]),
                AgentState::provider(PrivateProviderIntent {
                    node_id: NodeId::from_bytes([byte; 32]),
                    capability_claims: claims
                        .into_iter()
                        .map(|c| CapabilityTag::parse_shape(c).unwrap())
                        .collect(),
                    reservation_cents: Secret::new(price),
                    signing_secret_key: None,
                }),
            );
        }
        m
    }

    #[test]
    fn happy_path_round_finalizes() {
        let topology = TopologyConfig::from_toml_str(&baseline_topology_text()).unwrap();
        let rt = CoordinationRuntime::new(
            topology,
            OrderedBus::new(),
            sample_agents(),
            Scenario::empty(),
            4,
        )
        .unwrap();
        let outcome = rt.run("run-happy").unwrap();
        assert!(outcome.finalized);
        assert_eq!(outcome.final_round, RoundId::new(0));
        assert!(!outcome.log.receipts.is_empty());
    }

    #[test]
    fn rejections_are_empty_on_clean_run() {
        let topology = TopologyConfig::from_toml_str(&baseline_topology_text()).unwrap();
        let rt = CoordinationRuntime::new(
            topology,
            OrderedBus::new(),
            sample_agents(),
            Scenario::empty(),
            4,
        )
        .unwrap();
        let outcome = rt.run("run-clean").unwrap();
        assert!(outcome.log.rejections.is_empty());
    }
}
