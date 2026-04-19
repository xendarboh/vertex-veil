# Execution Plan: Vertex Veil

## Overview

Implement a Rust + Vertex + Noir system that lets agents coordinate over private intent, finalize a public Proof of Coordination, and verify the public record without revealing private price constraints.

The first execution target is a runtime-configurable compute-task matching protocol with a validated 4-node baseline, real per-agent Noir proofs, deterministic fallback rounds, and saved coordination artifacts.

## Prerequisites

- Rust toolchain installed and working in `vertex-veil/`
- Noir toolchain installed and available for circuit compile/test/prove flows
- `tashi-vertex` integration available to the Rust workspace
- Local environment able to run a 4-node Vertex baseline
- Intent decisions remain governed by `INTENT.md`

## Phase 0: Workspace Skeleton And Shared Types

### Description

Create the Rust workspace and baseline project layout for protocol code, agent CLIs, fixtures, artifact output, and Noir circuits. Define the shared intent, match, round, and artifact types without implementing network or proof behavior yet.

### Tests

#### Happy Path

- [ ] Rust workspace builds with library crate, CLI crate, and circuit directory wiring in place
- [ ] Shared intent and match types serialize and deserialize deterministically
- [ ] Runtime configuration loads a 4-node baseline topology from fixture files

#### Bad Path

- [ ] Invalid topology config is rejected with a structured error
- [ ] Unknown capability tag input is rejected or normalized by the configured parser
- [ ] Missing required fixture fields fail config loading cleanly
- [ ] Invalid stable key ordering input fails validation before runtime start

#### Edge Cases

- [ ] Empty provider list config is rejected explicitly
- [ ] Single illustrative capability tag config still loads successfully
- [ ] Duplicate node identifiers in topology config are rejected

#### Security

- [ ] Config parsing rejects malformed message paths and unsafe relative traversal inputs
- [ ] Artifact output paths are validated before file creation
- [ ] Stable public key parsing rejects malformed keys without panicking

#### Data Leak

- [ ] Config errors do not print private fixture values that are meant to simulate secret inputs
- [ ] Default logs do not dump full private intent bodies
- [ ] Debug formatting for shared types redacts private fields by default

#### Data Damage

- [ ] Artifact directory initialization does not clobber existing run data unintentionally
- [ ] Partial config load failure does not leave corrupted generated state files
- [ ] Shared serialization roundtrip preserves field integrity exactly

### E2E Gate

```bash
cargo test -p vertex-veil-core config shared_types && cargo test -p vertex-veil-agents cli_bootstrap
```

### Acceptance Criteria

- [ ] All 6 test categories pass
- [ ] Rust workspace and crate layout are in place
- [ ] Shared protocol types exist for intents, matches, rounds, proofs, and run artifacts
- [ ] E2E Gate passes

---

## Phase 1: Commitments And Deterministic Match Logic

### Description

Implement the commitment model, public/private intent split, capability-tag handling, stable public key ordering, candidate formation, and deterministic proposer / fallback-round logic as pure library code.

### Tests

#### Happy Path

- [ ] Commitment generation is deterministic for the same intent, nonce, and round
- [ ] Commitment changes when round number changes
- [ ] Candidate formation finds a feasible provider from public capability claims
- [ ] Stable public key ordering selects the same winner across repeated runs
- [ ] Fallback round selection advances to the next proposer deterministically

#### Bad Path

- [ ] Provider with incompatible public capability claim is excluded from candidate formation
- [ ] Proposal referencing unknown commitment is rejected
- [ ] Duplicate provider key in ordering input fails deterministically
- [ ] Missing requester coarse capability tag fails proposal construction
- [ ] Invalid round transition is rejected by the round-state machine

#### Edge Cases

- [ ] No feasible providers returns no proposal without panicking
- [ ] Multiple equally feasible providers still resolve to one deterministic winner
- [ ] Runtime-configurable capability tags work with a custom label set beyond the illustrative defaults

#### Security

- [ ] Round binding prevents commitment reuse across rounds in library validation
- [ ] Proposal validation rejects tampered public metadata
- [ ] Double-commit attempt by the same agent key is detected by state logic

#### Data Leak

- [ ] Proposal logs do not print requester budget or provider reservation price
- [ ] Candidate formation traces include only public claims and identifiers
- [ ] Commitment helper errors do not expose private witness inputs

#### Data Damage

- [ ] Round-state updates remain atomic under proposal rejection paths
- [ ] Deterministic ordering remains stable after serialization roundtrip
- [ ] Proposal rejection does not corrupt the active commitment set

### E2E Gate

```bash
cargo test -p vertex-veil-core commitments proposer round_state capability_tags
```

### Acceptance Criteria

- [ ] All 6 test categories pass
- [ ] Commitments and round binding are implemented as shared library logic
- [ ] Deterministic proposer and stable key winner selection are implemented
- [ ] Runtime-configurable coarse capability tags are supported
- [ ] E2E Gate passes

---

## Phase 2: Noir Circuits And Proof Interfaces

### Description

Implement the Noir circuits and Rust integration boundaries required for agents to prove local validity against private constraints using real proof artifacts.

### Tests

#### Happy Path

- [ ] Requester circuit proves a valid match against private budget constraints
- [ ] Provider circuit proves a valid match against private reservation constraints
- [ ] Rust witness generation matches the Noir circuit input schema
- [ ] Proof verification succeeds for a valid proof artifact generated by the local agent flow

#### Bad Path

- [ ] Requester proof generation fails when clearing conditions violate private budget
- [ ] Provider proof generation fails when reservation constraint is violated
- [ ] Verification rejects a proof artifact bound to the wrong round
- [ ] Verification rejects malformed proof payloads
- [ ] Witness generation fails cleanly for missing private inputs

#### Edge Cases

- [ ] Boundary price equality at the acceptance threshold verifies correctly
- [ ] Custom runtime capability labels still map correctly into the chosen circuit encoding
- [ ] Empty optional metadata fields do not break proof interface generation

#### Security

- [ ] Replay attempt using a prior round public input fails verification
- [ ] Tampered public inputs invalidate an otherwise valid proof
- [ ] Invalid proof artifact is rejected without panicking or partial acceptance

#### Data Leak

- [ ] Witness files and proof logs do not expose private budget or reservation data in plain logs
- [ ] Circuit integration errors redact private witness values
- [ ] Saved proof artifacts contain only intended public inputs and proof material

#### Data Damage

- [ ] Failed proof generation does not corrupt reusable proving artifacts
- [ ] Verification failure does not mutate persisted coordination state
- [ ] Proof serialization roundtrip preserves artifact integrity

### E2E Gate

```bash
nargo test && cargo test -p vertex-veil-core proofs noir_bridge
```

### Acceptance Criteria

- [ ] All 6 test categories pass
- [ ] Real Noir circuits exist for requester and provider validation
- [ ] Rust proof integration can generate and verify local proofs
- [ ] Round-bound replay rejection is enforced by proof validation
- [ ] E2E Gate passes

---

## Phase 3: Vertex Agent Runtime And Coordination Record

### Description

Build the CLI agents and Vertex-backed runtime that publish commitments, derive proposals, submit proof artifacts and signatures, persist the coordination log, and emit verifier-ready artifacts.

### Tests

#### Happy Path

- [ ] Four configured agents start and exchange ordered coordination messages over Vertex
- [ ] Requester and selected provider complete one valid round and persist a coordination log
- [ ] Matched provider publishes signed completion receipt and requester acknowledgement
- [ ] Standalone verifier reads the saved coordination log and reports valid

#### Bad Path

- [ ] Invalid proof artifact from one provider is rejected and the round does not finalize
- [ ] Unknown message type in the coordination log is rejected by the verifier
- [ ] Missing signature on a finalization path fails verification
- [ ] Commitment message from an unconfigured key is rejected or ignored cleanly
- [ ] Corrupted artifact file causes verifier failure with a precise error

#### Edge Cases

- [ ] Runtime handles no-match rounds without crashing
- [ ] Fallback round after failed proof selects the next proposer correctly
- [ ] Runtime supports a custom capability-label config while preserving the same protocol flow

#### Security

- [ ] Replay of a prior-round proof into the runtime is rejected
- [ ] Double-commit from a single agent key is rejected in the ordered state view
- [ ] Verifier detects tampering of proposal or proof records in persisted artifacts

#### Data Leak

- [ ] Runtime logs do not print private price constraints during happy or failure flows
- [ ] Saved coordination log excludes private witness material
- [ ] Verifier output remains public-record-only and never requests private inputs

#### Data Damage

- [ ] Partial runtime failure does not leave malformed coordination artifacts reported as valid
- [ ] Artifact writer remains consistent when a round aborts mid-flow
- [ ] Restarting verifier against the same artifact set produces identical results

### E2E Gate

```bash
cargo test -p vertex-veil-core verifier runtime_log && cargo run -p vertex-veil-agents -- demo --topology fixtures/topology-4node.toml --scenario fixtures/happy-path.toml --artifacts artifacts/phase3
```

### Acceptance Criteria

- [ ] All 6 test categories pass
- [ ] Vertex-backed agent runtime can execute a valid round and persist artifacts
- [ ] Invalid-proof rejection is visible in the saved coordination record
- [ ] Standalone verifier validates a good log and rejects a tampered one
- [ ] E2E Gate passes

---

## Phase 4: End-To-End Demo Hardening And Multi-Round Recovery

### Description

Harden the full demo flow around the validated 4-node baseline, fallback rounds, artifact packaging, and judge-facing reproducibility so the system is ready for execution and presentation.

### Tests

#### Happy Path

- [ ] Single command runs the baseline 4-node demo and produces a valid verifier report
- [ ] Demo artifacts include coordination log, verifier report, and completion receipt in a predictable layout
- [ ] Multi-round run completes successfully when the first proposal path fails and fallback recovers

#### Bad Path

- [ ] Demo command fails clearly when required toolchain dependencies are missing
- [ ] Demo command fails clearly when one node config is malformed
- [ ] Verifier report marks invalid when final artifact bundle is incomplete
- [ ] Demo run exits non-zero when the fallback round cannot recover to a valid match

#### Edge Cases

- [ ] Baseline demo still works when using only a subset of illustrative capability tags
- [ ] Demo supports a larger runtime-configured topology without breaking the baseline profile
- [ ] Artifact packaging remains deterministic across repeated runs with the same fixtures

#### Security

- [ ] Packaged demo artifacts do not include private witness or secret fixture material
- [ ] Judge-facing logs remain free of plaintext private price data
- [ ] Replay or tamper attempts in the packaged artifact set are detected by the verifier script

#### Data Leak

- [ ] README/demo script examples do not instruct users to expose private constraints
- [ ] Final report summaries remain public-only and redact internal witness paths where needed
- [ ] Failure output remains informative without leaking secret fixture values

#### Data Damage

- [ ] Demo command cleans or versions artifact directories without deleting unrelated files
- [ ] Re-running the demo does not corrupt prior saved reports
- [ ] Multi-round failure handling leaves a coherent final artifact bundle

### E2E Gate

```bash
cargo test && nargo test && cargo run -p vertex-veil-agents -- demo --topology fixtures/topology-4node.toml --scenario fixtures/fallback-invalid-proof.toml --artifacts artifacts/final && cargo run -p vertex-veil-agents -- verify --artifacts artifacts/final
```

### Acceptance Criteria

- [ ] All 6 test categories pass
- [ ] Single-command demo is reproducible on the validated 4-node baseline
- [ ] Fallback-round behavior is demonstrated end-to-end
- [ ] Final artifact bundle is judge-friendly and verifier-backed
- [ ] E2E Gate passes

---

## Final E2E Verification

```bash
cargo test && nargo test && cargo run -p vertex-veil-agents -- demo --topology fixtures/topology-4node.toml --scenario fixtures/fallback-invalid-proof.toml --artifacts artifacts/final && cargo run -p vertex-veil-agents -- verify --artifacts artifacts/final
```

## Risk Mitigation

| Risk | Mitigation | Contingency |
| --- | --- | --- |
| Noir proving integration takes longer than expected | Keep proof interface narrow and test witness generation before runtime integration | Reduce `v1` to the minimum requester/provider proof shape needed for the baseline scenario |
| Vertex runtime recovery behavior is harder than expected | Validate the 4-node baseline early and keep fallback logic deterministic in library code | Prioritize invalid-proof fallback over node restart complexity in the first delivery slice |
| Capability-tag generality expands scope | Treat tags as runtime-configurable coarse labels only | Defer richer attribute matching to a later intent update |
| Artifact format drifts between runtime and verifier | Define artifact schemas in shared library types first | Block runtime changes until verifier fixtures are updated |

## References

- [Intent](./INTENT.md)
- [Interview Decisions](./decisions.md)
