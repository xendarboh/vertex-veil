# Execution Plan: Vertex Veil

## Overview

Implement a Rust + Vertex + Noir system that lets agents coordinate over private intent, finalize a public Proof of Coordination, and verify the public record without revealing private price constraints.

The first execution target is a runtime-configurable compute-task matching protocol with a validated 4-node baseline, real per-agent Noir proofs, deterministic fallback rounds, explicit replay and double-commit rejection, third-party verifiability from the public record alone, and saved coordination artifacts.

## Prerequisites

- Rust toolchain installed and working in `vertex-veil/`
- Noir toolchain installed and available for circuit compile/test/prove flows
- `tashi-vertex` integration available to the Rust workspace
- Local environment able to run a 4-node Vertex baseline
- Intent decisions remain governed by `INTENT.md`

## Phase 0: Workspace Skeleton And Shared Types

### Description

Create the Rust workspace and baseline project layout for protocol code, agent CLIs, fixtures, artifact output, and Noir circuits. Define the shared intent, match, round, and artifact types without implementing network or proof behavior yet. Establish artifact schemas and interfaces so third-party verification can be built entirely from the public coordination record.

### Tests

#### Happy Path

- [x] Rust workspace builds with library crate, CLI crate, and circuit directory wiring in place
- [x] Shared intent, match, round, and artifact types serialize and deserialize deterministically
- [x] Runtime configuration loads a 4-node baseline topology from fixture files
- [x] Public coordination artifact schema is sufficient to represent verifier inputs without any private witness fields

#### Bad Path

- [x] Invalid topology config is rejected with a structured error
- [x] Unknown capability tag input is rejected or normalized by the configured parser
- [x] Missing required fixture fields fail config loading cleanly
- [x] Invalid stable key ordering input fails validation before runtime start
- [x] Artifact schema creation rejects attempts to include private witness fields in verifier-facing records

#### Edge Cases

- [x] Empty provider list config is rejected explicitly
- [x] Single illustrative capability tag config still loads successfully
- [x] Duplicate node identifiers in topology config are rejected
- [x] Artifact schema remains valid when optional public metadata is absent

#### Security

- [x] Config parsing rejects malformed message paths and unsafe relative traversal inputs
- [x] Artifact output paths are validated before file creation
- [x] Stable public key parsing rejects malformed keys without panicking
- [x] Shared artifact types reject duplicate commitment entries for the same agent key in a single round

#### Data Leak

- [x] Config errors do not print private fixture values that are meant to simulate secret inputs
- [x] Default logs do not dump full private intent bodies
- [x] Debug formatting for shared types redacts private fields by default
- [x] Public artifact schema contains no private witness material by construction

#### Data Damage

- [x] Artifact directory initialization does not clobber existing run data unintentionally
- [x] Partial config load failure does not leave corrupted generated state files
- [x] Shared serialization roundtrip preserves field integrity exactly
- [x] Artifact schema roundtrip preserves verifier-relevant public fields exactly

### E2E Gate

```bash
cargo test -p vertex-veil-core -- config shared_types artifacts && cargo test -p vertex-veil-agents cli_bootstrap
```

### Acceptance Criteria

- [x] All 6 test categories pass
- [x] Rust workspace and crate layout are in place
- [x] Shared protocol types exist for intents, matches, rounds, proofs, and run artifacts
- [x] Public coordination artifact schema is sufficient for third-party verification inputs
- [x] E2E Gate passes (using the `--` form; see note above)

---

## Phase 1: Commitments, Predicate Logic, And Deterministic Round State

### Description

Implement the commitment model, public/private intent split, capability-tag handling, stable public key ordering, candidate formation, deterministic proposer / fallback-round logic, replay rejection, and double-commit rejection as pure library code. Define the runtime-side match predicate and the parity fixtures that the Noir implementation must match.

### Tests

#### Happy Path

- [x] Commitment generation is deterministic for the same intent, nonce, and round
- [x] Commitment changes when round number changes
- [x] Candidate formation finds a feasible provider from public capability claims
- [x] Stable public key ordering selects the same winner across repeated runs
- [x] Fallback round selection advances to the next proposer deterministically
- [x] Runtime-side match predicate accepts a valid requester/provider pair with consistent public metadata

#### Bad Path

- [x] Provider with incompatible public capability claim is excluded from candidate formation
- [x] Proposal referencing unknown commitment is rejected
- [x] Duplicate provider key in ordering input fails deterministically
- [x] Missing requester coarse capability tag fails proposal construction
- [x] Invalid round transition is rejected by the round-state machine
- [x] Same agent key attempting to commit twice in one round is rejected
- [x] Prior-round proposal or proof metadata is rejected by round-bound validation

#### Edge Cases

- [x] No feasible providers returns no proposal without panicking
- [x] Multiple equally feasible providers still resolve to one deterministic winner
- [x] Runtime-configurable capability tags work with a custom label set beyond the illustrative defaults
- [x] Silent provider does not corrupt round-state advancement when the protocol moves to the next proposer

#### Security

- [x] Round binding prevents commitment reuse across rounds in library validation
- [x] Proposal validation rejects tampered public metadata
- [x] Double-commit attempt by the same agent key is detected by state logic
- [x] Replay attempt using prior-round identifiers is rejected before finalization

#### Data Leak

- [x] Proposal logs do not print requester budget or provider reservation price
- [x] Candidate formation traces include only public claims and identifiers
- [x] Commitment helper errors do not expose private witness inputs
- [x] Predicate parity fixtures use redacted or synthetic private values in failure output

#### Data Damage

- [x] Round-state updates remain atomic under proposal rejection paths
- [x] Deterministic ordering remains stable after serialization roundtrip
- [x] Proposal rejection does not corrupt the active commitment set
- [x] Double-commit rejection does not mutate the accepted commitment set incorrectly

### E2E Gate

```bash
cargo test -p vertex-veil-core -- commitments proposer round_state capability_tags predicate_runtime
```

### Acceptance Criteria

- [x] All 6 test categories pass
- [x] Commitments and round binding are implemented as shared library logic
- [x] Deterministic proposer and stable key winner selection are implemented
- [x] Runtime-configurable coarse capability tags are supported
- [x] Replay and double-commit rejection are enforced in round-state logic
- [x] Runtime-side predicate fixtures exist for Noir parity testing
- [x] E2E Gate passes (using the `--` form; see note above)

---

## Phase 2: Noir Bring-Up, Full Predicate Coverage, And Parity Testing

### Description

Implement the Noir circuits and Rust integration boundaries required for agents to prove local validity against private constraints using real proof artifacts. Use a staged bring-up: first prove one structural property end-to-end with a minimal viable circuit, then land the full requester/provider predicate set required for `v1`. Establish predicate parity tests so runtime and circuit implementations are treated as one logical function.

### Tests

#### Happy Path

- [ ] Minimal requester or provider circuit proves one structural property end-to-end with real Noir tooling
- [ ] Requester circuit proves a valid match against private budget constraints
- [ ] Provider circuit proves a valid match against private reservation constraints
- [ ] Rust witness generation matches the Noir circuit input schema
- [ ] Proof verification succeeds for a valid proof artifact generated by the local agent flow
- [ ] Shared parity fixtures produce matching allow/deny results in Rust and Noir implementations

#### Bad Path

- [ ] Requester proof generation fails when clearing conditions violate private budget
- [ ] Provider proof generation fails when reservation constraint is violated
- [ ] Verification rejects a proof artifact bound to the wrong round
- [ ] Verification rejects malformed proof payloads
- [ ] Witness generation fails cleanly for missing private inputs
- [ ] Rust and Noir predicate outputs diverging on the same fixture fails the parity suite hard

#### Edge Cases

- [ ] Boundary price equality at the acceptance threshold verifies correctly
- [ ] Custom runtime capability labels still map correctly into the chosen circuit encoding
- [ ] Empty optional metadata fields do not break proof interface generation
- [ ] Minimal-circuit bring-up can be retired cleanly once the full predicate set is in place

#### Security

- [ ] Replay attempt using a prior round public input fails verification
- [ ] Tampered public inputs invalidate an otherwise valid proof
- [ ] Invalid proof artifact is rejected without panicking or partial acceptance
- [ ] Parity test fixtures cover tampered metadata and mismatched round cases

#### Data Leak

- [ ] Witness files and proof logs do not expose private budget or reservation data in plain logs
- [ ] Circuit integration errors redact private witness values
- [ ] Saved proof artifacts contain only intended public inputs and proof material
- [ ] Parity test failures do not leak private witness values while explaining the mismatch

#### Data Damage

- [ ] Failed proof generation does not corrupt reusable proving artifacts
- [ ] Verification failure does not mutate persisted coordination state
- [ ] Proof serialization roundtrip preserves artifact integrity
- [ ] Failed parity checks do not mutate shared fixture baselines

### E2E Gate

```bash
nargo test && cargo test -p vertex-veil-core -- proofs noir_bridge predicate_parity
```

### Acceptance Criteria

- [ ] All 6 test categories pass
- [ ] Real Noir circuits exist for requester and provider validation
- [ ] Rust proof integration can generate and verify local proofs
- [ ] Round-bound replay rejection is enforced by proof validation
- [ ] Predicate parity between Rust and Noir is verified by shared fixtures
- [ ] Minimal-circuit bring-up is superseded by the full requester/provider predicate set before phase completion
- [ ] E2E Gate passes

---

## Phase 3: Vertex Agent Runtime, Third-Party Verifier, And Adversarial Recovery

### Description

Build the CLI agents and Vertex-backed runtime that publish commitments, derive proposals, submit proof artifacts and signatures, persist the coordination log, and emit verifier-ready artifacts. Add the standalone verifier and adversarial scenarios for invalid proofs, replay, double-commit, silent-node behavior, and mid-round drop handling.

### Tests

#### Happy Path

- [ ] Four configured agents start and exchange ordered coordination messages over Vertex
- [ ] Requester and selected provider complete one valid round and persist a coordination log
- [ ] Matched provider publishes signed completion receipt and requester acknowledgement
- [ ] Standalone verifier reads the saved coordination log and reports valid using public inputs only
- [ ] Runtime recovers to a valid fallback round after one proposal path fails

#### Bad Path

- [ ] Invalid proof artifact from one provider is rejected and the round does not finalize
- [ ] Replay of a prior-round proof into the active round is rejected visibly
- [ ] Double-commit from a single agent key in one round is rejected visibly
- [ ] Unknown message type in the coordination log is rejected by the verifier
- [ ] Missing signature on a finalization path fails verification
- [ ] Commitment message from an unconfigured key is rejected or ignored cleanly
- [ ] Corrupted artifact file causes verifier failure with a precise error
- [ ] Mid-round drop beyond the recoverable threshold aborts with a verifiable reason instead of partial success

#### Edge Cases

- [ ] Runtime handles no-match rounds without crashing
- [ ] Fallback round after failed proof selects the next proposer correctly
- [ ] Runtime supports a custom capability-label config while preserving the same protocol flow
- [ ] Silent node within the validated baseline does not prevent the swarm from completing or aborting verifiably

#### Security

- [ ] Replay of a prior-round proof into the runtime is rejected
- [ ] Double-commit from a single agent key is rejected in the ordered state view
- [ ] Verifier detects tampering of proposal or proof records in persisted artifacts
- [ ] Third-party verifier completes without access to private inputs or witness files

#### Data Leak

- [ ] Runtime logs do not print private price constraints during happy or failure flows
- [ ] Saved coordination log excludes private witness material
- [ ] Verifier output remains public-record-only and never requests private inputs
- [ ] Drop or abort handling does not leak the private constraints of the affected node

#### Data Damage

- [ ] Partial runtime failure does not leave malformed coordination artifacts reported as valid
- [ ] Artifact writer remains consistent when a round aborts mid-flow
- [ ] Restarting verifier against the same artifact set produces identical results
- [ ] Silent-node or drop handling leaves a coherent final round record

### E2E Gate

```bash
cargo test -p vertex-veil-core -- verifier runtime_log adversarial && cargo run -p vertex-veil-agents -- demo --topology fixtures/topology-4node.toml --scenario fixtures/replay-doublecommit-drop.toml --artifacts artifacts/phase3 && cargo run -p vertex-veil-agents -- verify --artifacts artifacts/phase3
```

### Acceptance Criteria

- [ ] All 6 test categories pass
- [ ] Vertex-backed agent runtime can execute a valid round and persist artifacts
- [ ] Invalid-proof rejection, replay rejection, and double-commit rejection are visible in the saved coordination record
- [ ] Silent-node or mid-round-drop behavior is handled by valid completion or verifiable abort within the validated baseline
- [ ] Standalone verifier validates a good log and rejects tampered or incomplete ones using public inputs only
- [ ] E2E Gate passes

---

## Phase 4: End-To-End Demo Hardening And Reproducible BFT Baseline

### Description

Harden the full demo flow around the validated 4-node baseline, fallback rounds, adversarial artifact packaging, and judge-facing reproducibility so the system is ready for execution and presentation.

### Tests

#### Happy Path

- [ ] Single command runs the baseline 4-node demo and produces a valid verifier report
- [ ] Demo artifacts include coordination log, verifier report, and completion receipt in a predictable layout
- [ ] Multi-round run completes successfully when the first proposal path fails and fallback recovers
- [ ] Third-party verifier run from artifacts alone succeeds without any private inputs present on disk

#### Bad Path

- [ ] Demo command fails clearly when required toolchain dependencies are missing
- [ ] Demo command fails clearly when one node config is malformed
- [ ] Verifier report marks invalid when final artifact bundle is incomplete
- [ ] Demo run exits non-zero when the fallback round cannot recover to a valid match
- [ ] Demo run exits non-zero with a verifiable abort artifact when silent-node or drop conditions exceed the recoverable threshold

#### Edge Cases

- [ ] Baseline demo still works when using only a subset of illustrative capability tags
- [ ] Demo supports a larger runtime-configured topology without breaking the baseline profile
- [ ] Artifact packaging remains deterministic across repeated runs with the same fixtures
- [ ] Replay and double-commit adversarial fixtures remain reproducible across repeated runs

#### Security

- [ ] Packaged demo artifacts do not include private witness or secret fixture material
- [ ] Judge-facing logs remain free of plaintext private price data
- [ ] Replay or tamper attempts in the packaged artifact set are detected by the verifier script
- [ ] Artifact bundle demonstrates visible rejection of invalid proof, replay, and double-commit scenarios

#### Data Leak

- [ ] README/demo script examples do not instruct users to expose private constraints
- [ ] Final report summaries remain public-only and redact internal witness paths where needed
- [ ] Failure output remains informative without leaking secret fixture values
- [ ] Public verifier workflow documentation never requires private input material

#### Data Damage

- [ ] Demo command cleans or versions artifact directories without deleting unrelated files
- [ ] Re-running the demo does not corrupt prior saved reports
- [ ] Multi-round failure handling leaves a coherent final artifact bundle
- [ ] Adversarial demo bundle preserves enough evidence for third-party verification after failures

### E2E Gate

```bash
cargo test && nargo test && cargo run -p vertex-veil-agents -- demo --topology fixtures/topology-4node.toml --scenario fixtures/replay-doublecommit-drop.toml --artifacts artifacts/final && cargo run -p vertex-veil-agents -- verify --artifacts artifacts/final
```

### Acceptance Criteria

- [ ] All 6 test categories pass
- [ ] Single-command demo is reproducible on the validated 4-node baseline
- [ ] Fallback-round behavior is demonstrated end-to-end
- [ ] Invalid-proof, replay, and double-commit rejection are demonstrated end-to-end
- [ ] Final artifact bundle is judge-friendly, verifier-backed, and sufficient for third-party verification from public inputs alone
- [ ] E2E Gate passes

---

## Final E2E Verification

```bash
cargo test && nargo test && cargo run -p vertex-veil-agents -- demo --topology fixtures/topology-4node.toml --scenario fixtures/replay-doublecommit-drop.toml --artifacts artifacts/final && cargo run -p vertex-veil-agents -- verify --artifacts artifacts/final
```

## Risk Mitigation

| Risk                                                     | Mitigation                                                                                                                      | Contingency                                                                                                                                     |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| Noir proving integration takes longer than expected      | Stage Noir bring-up with a minimal viable circuit first, then land the full requester/provider predicate set under parity tests | Keep the minimal-circuit spike only as a build step; `v1` remains incomplete until the full predicate set is present                            |
| Rust and Noir predicate logic drift                      | Treat parity as a structural invariant and maintain shared fixtures across both implementations                                 | Block phase completion until parity failures are resolved                                                                                       |
| Vertex runtime recovery behavior is harder than expected | Validate the 4-node baseline early and keep fallback logic deterministic in library code                                        | Prioritize invalid-proof, replay, double-commit, silent-node, and drop handling within the validated baseline before broader recovery ambitions |
| Capability-tag generality expands scope                  | Treat tags as runtime-configurable coarse labels only                                                                           | Defer richer attribute matching to a later intent update                                                                                        |
| Artifact format drifts between runtime and verifier      | Define artifact schemas in shared library types first                                                                           | Block runtime changes until verifier fixtures are updated                                                                                       |

## References

- [Intent](./INTENT.md)
- [Interview Decisions](./decisions.md)
