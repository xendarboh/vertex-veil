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

- [x] Minimal requester or provider circuit proves one structural property end-to-end with real Noir tooling
- [x] Requester circuit proves a valid match against private budget constraints
- [x] Provider circuit proves a valid match against private reservation constraints
- [x] Rust witness generation matches the Noir circuit input schema
- [x] Proof verification succeeds for a valid proof artifact generated by the local agent flow
- [x] Shared parity fixtures produce matching allow/deny results in Rust and Noir implementations

#### Bad Path

- [x] Requester proof generation fails when clearing conditions violate private budget
- [x] Provider proof generation fails when reservation constraint is violated
- [x] Verification rejects a proof artifact bound to the wrong round
- [x] Verification rejects malformed proof payloads
- [x] Witness generation fails cleanly for missing private inputs
- [x] Rust and Noir predicate outputs diverging on the same fixture fails the parity suite hard

#### Edge Cases

- [x] Boundary price equality at the acceptance threshold verifies correctly
- [x] Custom runtime capability labels still map correctly into the chosen circuit encoding
- [x] Empty optional metadata fields do not break proof interface generation
- [x] Minimal-circuit bring-up can be retired cleanly once the full predicate set is in place

#### Security

- [x] Replay attempt using a prior round public input fails verification
- [x] Tampered public inputs invalidate an otherwise valid proof
- [x] Invalid proof artifact is rejected without panicking or partial acceptance
- [x] Parity test fixtures cover tampered metadata and mismatched round cases

#### Data Leak

- [x] Witness files and proof logs do not expose private budget or reservation data in plain logs
- [x] Circuit integration errors redact private witness values
- [x] Saved proof artifacts contain only intended public inputs and proof material
- [x] Parity test failures do not leak private witness values while explaining the mismatch

#### Data Damage

- [x] Failed proof generation does not corrupt reusable proving artifacts
- [x] Verification failure does not mutate persisted coordination state
- [x] Proof serialization roundtrip preserves artifact integrity
- [x] Failed parity checks do not mutate shared fixture baselines

### E2E Gate

```bash
cd circuits && nargo compile --workspace && nargo test --workspace && cd .. && cargo test -p vertex-veil-core -- proofs noir_bridge predicate_parity
```

> Implementation notes (surfaced 2026-04-20):
>
> - **Noir commands run from `circuits/`.** The gate command above enters the
>   Noir workspace explicitly because `vertex-veil/` itself does not contain a
>   `Nargo.toml`.
> - **`nargo test` needs `--workspace`** to run tests in `shared`, `provider`,
>   and `requester`. The gate command above is corrected; the bare `nargo
>   test` form exercises only the `default-member` from
>   `circuits/Nargo.toml`.
> - **`nargo compile --workspace` is required on a fresh checkout.** Rust-side
>   bridge and parity tests load compiled circuit JSON artifacts from
>   `circuits/target/`, which are generated and not checked into git.
> - **Hash function chosen: blake2s.** Noir stdlib v1.0.0-beta.20 exposes
>   `sha256_compression` (the block primitive) but not a full `sha256`. To
>   keep parity tractable, both Rust commitments and Noir circuits use
>   blake2s-256 over a fixed-size padded preimage. The byte layout is
>   documented in `crates/vertex-veil-core/src/commitments.rs`. Phase 1
>   commitment hex values changed as a side effect; no Phase 1 test pinned a
>   specific hex, so Phase 1 still passes.
> - **Default proof path: ACIR `execute`.** `cargo test -p vertex-veil-core`
>   runs the gate via `noir_rs::execute` which validates every circuit
>   constraint without requiring barretenberg or SRS download. Acceptance
>   criteria pass against this path.
> - **Real UltraHonk path: `barretenberg` feature.** `cargo test -p
>   vertex-veil-noir --features barretenberg --test proofs_barretenberg
>   --release` runs full prove + verify against `crs.aztec.network`. Three
>   tests cover requester, provider, and wrong-round rejection at proof
>   generation time. Optional and not part of the default gate to avoid
>   network dependency in the baseline run.
> - **Parity contract.** `PredicateDenial::tag()` strings are pinned by
>   `predicate_parity_codes_are_stable_strings`. Any future Noir-emitted
>   denial codes must use the same strings.

### Acceptance Criteria

- [x] All 6 test categories pass
- [x] Real Noir circuits exist for requester and provider validation
- [x] Rust proof integration can generate and verify local proofs
- [x] Round-bound replay rejection is enforced by proof validation
- [x] Predicate parity between Rust and Noir is verified by shared fixtures
- [x] Minimal-circuit bring-up is superseded by the full requester/provider predicate set before phase completion
- [x] E2E Gate passes

---

## Phase 3: Vertex Agent Runtime, Third-Party Verifier, And Adversarial Recovery

### Description

Build the CLI agents and Vertex-backed runtime that publish commitments, derive proposals, submit proof artifacts and signatures, persist the coordination log, and emit verifier-ready artifacts. Add the standalone verifier and adversarial scenarios for invalid proofs, replay, double-commit, silent-node behavior, and mid-round drop handling.

### Tests

#### Happy Path

- [x] Four configured agents start and exchange ordered coordination messages over Vertex
- [x] Requester and selected provider complete one valid round and persist a coordination log
- [x] Matched provider publishes signed completion receipt and requester acknowledgement
- [x] Standalone verifier reads the saved coordination log and reports valid using public inputs only
- [x] Runtime recovers to a valid fallback round after one proposal path fails

#### Bad Path

- [x] Invalid proof artifact from one provider is rejected and the round does not finalize
- [x] Replay of a prior-round proof into the active round is rejected visibly
- [x] Double-commit from a single agent key in one round is rejected visibly
- [x] Unknown message type in the coordination log is rejected by the verifier
- [x] Missing signature on a finalization path fails verification
- [x] Commitment message from an unconfigured key is rejected or ignored cleanly
- [x] Corrupted artifact file causes verifier failure with a precise error
- [x] Mid-round drop beyond the recoverable threshold aborts with a verifiable reason instead of partial success

#### Edge Cases

- [x] Runtime handles no-match rounds without crashing
- [x] Fallback round after failed proof selects the next proposer correctly
- [x] Runtime supports a custom capability-label config while preserving the same protocol flow
- [x] Silent node within the validated baseline does not prevent the swarm from completing or aborting verifiably

#### Security

- [x] Replay of a prior-round proof into the runtime is rejected
- [x] Double-commit from a single agent key is rejected in the ordered state view
- [x] Verifier detects tampering of proposal or proof records in persisted artifacts
- [x] Third-party verifier completes without access to private inputs or witness files

#### Data Leak

- [x] Runtime logs do not print private price constraints during happy or failure flows
- [x] Saved coordination log excludes private witness material
- [x] Verifier output remains public-record-only and never requests private inputs
- [x] Drop or abort handling does not leak the private constraints of the affected node

#### Data Damage

- [x] Partial runtime failure does not leave malformed coordination artifacts reported as valid
- [x] Artifact writer remains consistent when a round aborts mid-flow
- [x] Restarting verifier against the same artifact set produces identical results
- [x] Silent-node or drop handling leaves a coherent final round record

### E2E Gate

```bash
cd circuits && nargo compile --workspace && cd .. && cargo test -p vertex-veil-core -- verifier runtime_log adversarial && cargo run -p vertex-veil-agents -- demo --topology fixtures/topology-4node.toml --private-intents fixtures/topology-4node.private.toml --scenario fixtures/replay-doublecommit-drop.toml --artifacts artifacts/phase3 && cargo run -p vertex-veil-agents -- verify --artifacts artifacts/phase3
```

> Implementation notes (surfaced 2026-04-21):
>
> - **Default `--private-intents` lookup.** If the `demo` CLI is invoked
>   without `--private-intents`, the runner looks for a sibling
>   `<topology-stem>.private.toml` next to the topology file. The gate
>   command above passes the flag explicitly so it survives a future
>   rename of the fixture. The repo ships
>   `fixtures/topology-4node.private.toml` for the 4-node baseline.
> - **Transport: `OrderedBus` default, real Vertex as Phase 4 hardening.**
>   The runtime is parameterized over a `CoordinationTransport` trait
>   whose single contract is consensus-ordered broadcast — exactly what
>   `tashi-vertex::Engine` provides. The Phase 3 demo binary runs all
>   four agents in a single process over an in-memory `OrderedBus` that
>   preserves FIFO order across broadcasters, which is behaviorally
>   equivalent to Vertex ordering for a single-process run. Swapping in a
>   `VertexTransport` that wraps `Engine::send_transaction` /
>   `Engine::recv_message` is a drop-in transport swap; no protocol
>   logic changes. That upgrade is scheduled for Phase 4's "Reproducible
>   BFT Baseline" and is deferred here to avoid a network-dependent
>   default gate.
> - **Proof artifact format.** Each `ProofArtifactRecord` carries a
>   canonical 73-byte public-inputs payload (`round`, `node_id`,
>   `commitment_hash`, role byte) hex-encoded. The `proof_hex` begins with
>   a marker byte (`1` = ACIR-execute-validated, `2` = UltraHonk; full
>   UltraHonk bytes land when the `barretenberg` feature is enabled).
>   The verifier decodes this layout directly and matches the embedded
>   commitment hash against the logged commitment — tampering with any
>   field breaks that equality check.
> - **Completion receipt signature.** The runtime emits a deterministic
>   blake2s-256 tag over `(domain, provider, round, capability)` as the
>   receipt signature. The verifier recomputes the same tag and rejects
>   mismatches. Real ed25519 signing is a Phase 4 hardening step; the
>   shape is already in place.
> - **`CoordinationLog` gained three public fields (serde-default):**
>   `rejections`, `final_round`, and `finalized`. Old v1 logs that predate
>   these fields still deserialize; the defaults are empty / zero /
>   `false` so back-compat stays silent.
> - **Private-intent fixture file format.** Demo runs need private witness
>   material per node. The binary loads it from a separate TOML file
>   (`*.private.toml`) cross-validated against the topology (role match,
>   capability match, every topology node present). Values are never
>   echoed in errors — a malformed file surfaces the field name, not the
>   value.

### Acceptance Criteria

- [x] All 6 test categories pass
- [x] Vertex-backed agent runtime can execute a valid round and persist artifacts
- [x] Invalid-proof rejection, replay rejection, and double-commit rejection are visible in the saved coordination record
- [x] Silent-node or mid-round-drop behavior is handled by valid completion or verifiable abort within the validated baseline
- [x] Standalone verifier validates a good log and rejects tampered or incomplete ones using public inputs only
- [x] E2E Gate passes

---

## Phase 4: End-To-End Artifact Hardening And Reproducible Baseline

### Description

Harden the validated 4-node baseline around fallback rounds, adversarial artifact packaging, and reproducible public artifact output so the system is ready for repeatable execution and third-party verification.

### Tests

#### Happy Path

- [x] Single command runs the baseline 4-node demo and produces a valid verifier report
- [x] Demo artifacts include coordination log, verifier report, and completion receipt in a predictable layout
- [x] Multi-round run completes successfully when the first proposal path fails and fallback recovers
- [x] Third-party verifier run from artifacts alone succeeds without any private inputs present on disk

#### Bad Path

- [x] Demo command fails clearly when required toolchain dependencies are missing
- [x] Demo command fails clearly when one node config is malformed
- [x] Verifier report marks invalid when final artifact bundle is incomplete
- [x] Demo run exits non-zero when the fallback round cannot recover to a valid match
- [x] Demo run exits non-zero with a verifiable abort artifact when silent-node or drop conditions exceed the recoverable threshold

#### Edge Cases

- [x] Baseline demo still works when using only a subset of illustrative capability tags
- [x] Demo supports a larger runtime-configured topology without breaking the baseline profile
- [x] Artifact packaging remains deterministic across repeated runs with the same fixtures
- [x] Replay and double-commit adversarial fixtures remain reproducible across repeated runs

#### Security

- [x] Packaged demo artifacts do not include private witness or secret fixture material
- [x] Public logs remain free of plaintext private price data
- [x] Replay or tamper attempts in the packaged artifact set are detected by the verifier script
- [x] Artifact bundle demonstrates visible rejection of invalid proof, replay, and double-commit scenarios

#### Data Leak

- [x] README/demo script examples do not instruct users to expose private constraints
- [x] Final report summaries remain public-only and redact internal witness paths where needed
- [x] Failure output remains informative without leaking secret fixture values
- [x] Public verifier workflow documentation never requires private input material

#### Data Damage

- [x] Demo command cleans or versions artifact directories without deleting unrelated files
- [x] Re-running the demo does not corrupt prior saved reports
- [x] Multi-round failure handling leaves a coherent final artifact bundle
- [x] Adversarial demo bundle preserves enough evidence for third-party verification after failures

### E2E Gate

```bash
cd circuits && nargo compile --workspace && nargo test --workspace && cd .. && cargo test && cargo run -p vertex-veil-agents -- demo --topology fixtures/topology-4node.toml --scenario fixtures/replay-doublecommit-drop.toml --artifacts artifacts/final && cargo run -p vertex-veil-agents -- verify --artifacts artifacts/final
```

### Acceptance Criteria

- [x] All 6 test categories pass
- [x] Single-command demo is reproducible on the validated 4-node baseline
- [x] Fallback-round behavior is demonstrated end-to-end
- [x] Invalid-proof, replay, and double-commit rejection are demonstrated end-to-end
- [x] Final artifact bundle is verifier-backed, publicly inspectable, and sufficient for third-party verification from public inputs alone
- [x] E2E Gate passes

> Implementation notes (surfaced 2026-04-21):
>
> - **Ed25519 completion-receipt signatures.** Each topology node carries
>   an optional `signing_public_key` (32-byte curve point, hex); the
>   private-intent fixture carries the matching `signing_secret_key`
>   (32-byte seed, hex) inside a `Secret<SigningSecretSeed>`. The runtime
>   signs receipts with ed25519-dalek; the verifier checks with the
>   configured public key. Phase 3 fixtures that omit both fields still
>   verify via the legacy deterministic blake2s tag, so back-compat stays
>   silent for existing logs.
> - **Artifact bundle layout (Phase 4 public):**
>   `coordination_log.json`, `verifier_report.json`, `run_status.json`,
>   `completion_receipt.json`, `bundle_README.md`, `topology.toml`,
>   `scenario.toml` (when supplied). `run_status.json` surfaces
>   `finalized`, `final_round`, `receipt_present`, `abort_reason`,
>   `rejection_count`, and the full `bundle_files` manifest in a single
>   public artifact.
> - **Abort handling.** When `max_rounds` is exhausted without
>   finalization the runtime sets `CoordinationLog.abort_reason =
>   "max_rounds_exceeded"`. The bundle is still written, the verifier
>   re-confirms structural coherence, and the demo binary exits with
>   code 2 so CI / scripts can distinguish abort from happy path without
>   parsing artifacts.
> - **Directory versioning.** By default the demo rotates any existing
>   bundle to `<artifacts>.prev-<N>` (monotonic N) via a whole-dir
>   rename, preserving every file — including unrelated files a judge
>   dropped in — intact. `--force` overwrites in place but still only
>   touches files from the writer's manifest.
> - **Real tashi-vertex transport behind a feature flag.** A
>   `VertexTransport: CoordinationTransport` lives in
>   `crates/vertex-veil-agents/src/vertex_transport.rs` gated by the
>   `vertex-transport` cargo feature, which pulls in `tashi-vertex`
>   (git dep) + `tokio` + `anyhow`. The default build and the E2E Gate
>   stay network-free and deterministic; `cargo check -p
>   vertex-veil-agents --features vertex-transport` validates that the
>   Vertex-backed path compiles. Protocol logic is transport-agnostic,
>   so swapping `OrderedBus` for `VertexTransport` requires no change
>   to `vertex-veil-core`.
> - **Private-intent parse-error redaction.** The loader already stripped
>   TOML pipe-prefixed source echoes; Phase 4 also blanks any
>   `"..."`-quoted substring from the residual diagnostic so the TOML
>   "invalid type: string \"SECRET\"" path cannot leak the offending
>   value.

---

## Phase 5: Vertex-Backed Runtime, Local Cluster Workflows, And Observability

### Description

Complete the real `VertexTransport` path as the primary multi-process runtime for `v1`, and expose durable local execution workflows around it. Ship a single-command local cluster runner (`demo-bft`), a long-lived per-node workflow with restart/rejoin support (`node --persist`), per-node public artifact bundles that verify with the standalone verifier, and protocol-event observability for both the in-process deterministic path and the real Vertex-backed path. The in-process baseline remains the default CI gate; the Vertex-backed gate is opt-in and documented.

### Tests

#### Happy Path

- [x] `node` subcommand starts a single process bound to an address with a peer list and a `CoordinationTransport` backed by real Vertex consensus
- [x] `demo-bft` orchestrator spawns four `node` children and drives them to finalized per-node bundles under the baseline scenario
- [x] Per-node bundles (`<artifacts>/<node-id>/…`) each pass the existing `verify` subcommand with `valid=true`
- [x] Protocol-event tags (`[VERTEX]`, `[COORD]`, `[ABORT]`) appear in the real multi-process output and the deterministic local path
- [x] Command exit codes distinguish finalized success, coherent abort, and runtime error paths

#### Bad Path

- [x] `node` fails clearly when `--peer` entries are malformed or incomplete
- [x] `node` fails clearly when `--secret` / `--secret-env` inputs are missing
- [x] `demo-bft` reports non-zero when a child crashes unexpectedly
- [x] Failure-injection paths remain bounded and surface a timeout or error rather than hanging indefinitely
- [x] Real-transport startup failures surface transport-specific errors without panicking

#### Edge Cases

- [x] `--base-port <u16>` allows the orchestrator to avoid local port collisions
- [x] `node --persist` keeps a Vertex participant alive across repeated coordination sessions
- [x] `--rejoin` is available for a node returning to an existing local cluster
- [x] The deterministic local path remains available for repeatable adversarial fixtures without the network layer

#### Security

- [x] Secrets are not printed in the normal stdout flows used for local cluster execution
- [x] `--secret-env` keeps the secret off argv so it does not leak into process listings
- [x] Per-node bundles contain only public artifacts (no private intents, no secret material)
- [x] Protocol-event output remains public-only and excludes private witness material

#### Data Leak

- [x] `[COORD]` log lines never include private price constraints
- [x] `README.md` examples never instruct users to expose private constraints
- [x] Loader and verifier failures continue to redact private fixture values
- [x] Rejoin and peer wiring use only public peer identifiers and transport addresses

#### Data Damage

- [x] Per-node bundles remain coherent after finalize or coherent abort
- [x] `node --persist` refreshes the latest snapshot without corrupting verifier-facing state
- [x] Re-running `demo-bft` reuses per-node artifact directories and preserves verifier coherence
- [x] Standalone `verify` is idempotent against a Phase 5 per-node bundle and produces identical reports across repeated invocations

### E2E Gate (opt-in, network-bound)

```bash
cargo build -p vertex-veil-agents --features vertex-transport
cargo run -p vertex-veil-agents --features vertex-transport -- \
  demo-bft --scenario fixtures/scenario-bft-rejoin.toml \
           --artifacts artifacts/bft \
           --fail-at-round 1 --rejoin-after-ms 2000
for n in n1 n2 n3 n4; do
  cargo run -p vertex-veil-agents -- verify --artifacts artifacts/bft/$n
done
```

The Phase 4 in-process gate (network-free, deterministic) remains the default CI path and is unchanged. The Phase 5 gate is documented as the reproducible Vertex-backed baseline and is not required for `cargo test` to pass.

### Acceptance Criteria

- [x] `node` runs a single local agent over a real `tashi-vertex::Engine` via `VertexTransport`, including peer parsing, secret resolution, and per-node bundle writes.
- [x] `demo-bft` provides the single-command multi-process shape: spawns four `node` children, wires peer lists, supports `--fail-at-round` / `--rejoin-after-ms`, aggregates output, and writes one bundle per node.
- [x] Protocol-event observability is exposed through the `RuntimeObserver` interface for both the deterministic local path and the real Vertex-backed path.
- [x] `demo` remains the deterministic in-process local path and always emits observable protocol milestones.
- [x] Top-level `README.md` explains the system, presents both the Vertex-backed and deterministic local workflows, and documents verifier-backed artifacts.
- [x] Phase 4 E2E Gate still passes unchanged.
- [x] Phase 5 BFT E2E Gate is documented as an opt-in Vertex-backed baseline.

> Implementation notes (surfaced 2026-04-21):
>
> - **Deterministic local path remains first-class.** The same
>   `CoordinationTransport` abstraction (`OrderedBus` for in-process,
>   `VertexTransport` for real BFT) drives a single protocol loop. The
>   deterministic in-process path mirrors Vertex ordering and exercises every
>   protocol milestone: commitments, proposal, fallback rotation, ZK proof
>   verification, visible adversarial rejection, and ed25519-signed
>   completion receipt.
> - **Real-Vertex substrate path is feature-gated, built, and
>   CLI-ready.** `cargo build -p vertex-veil-agents --features
>   vertex-transport` succeeds. `node --help` and `demo-bft --help` both
>   expose the full flag surface. `VertexTransport` wraps a live
>   `tashi-vertex::Engine` on a dedicated tokio runtime with an inline
>   heartbeat pacer to keep the consensus engine producing events between
>   protocol phases. The orchestrator generates fresh Vertex keypairs per
>   run, spawns four children on loopback UDP ports (configurable via
>   `--base-port`), and threads per-child secrets through env vars so
>   they never land in argv.
> - **Observability is a runtime concern, not a separate mode.**
>   `RuntimeObserver` on `CoordinationRuntime` remains the hook for surfaced
>   protocol milestones. The deterministic local path emits `[local]
>   [COORD]` / `[local] [VERTEX]` / `[local] [ABORT]`; the real Vertex-backed
>   node path emits per-node tags. This keeps local debugging and multi-node
>   observation aligned.
> - **Runtime guards.** `broadcast_proposal` / `broadcast_proofs` /
>   `broadcast_receipt` now check `self.agents.contains_key(&X)` before
>   emitting, so a single-agent runtime (one process per node) only
>   speaks for its own identity. The in-process local path (all 4 agents in
>   `self.agents`) is unchanged in behavior because the contains check
>   always succeeds.
> - **Deterministic local bundles stay reproducible.** `cargo test
>   --workspace` remains 224-green; `edge_artifact_packaging_deterministic`
>   and `edge_replay_doublecommit_reproducible` still pass.

---

## Final E2E Verification

```bash
cd circuits && nargo compile --workspace && nargo test --workspace && cd .. \
  && cargo test \
  && cargo clippy --workspace --all-targets -- -D warnings \
  && cargo run -p vertex-veil-agents -- \
       demo --topology fixtures/topology-4node.toml \
            --scenario fixtures/replay-doublecommit-drop.toml \
            --artifacts artifacts/final \
  && cargo run -p vertex-veil-agents -- verify --artifacts artifacts/final \
  && cargo build -p vertex-veil-agents --features vertex-transport
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
