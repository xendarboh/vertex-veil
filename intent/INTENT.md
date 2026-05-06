# Vertex Veil Intent

> Enable leaderless agents to coordinate on private intent and produce an auditable Proof of Coordination without relying on a central orchestrator.

Status: synced
Last updated: 2026-05-05

::: locked {reason="core project identity"}

## Vision

Vertex Veil is a system for leaderless coordination over private intent.

Its purpose is to let autonomous agents reach a valid shared outcome without revealing sensitive constraints in plaintext and without deferring trust to a master orchestrator. The project combines two complementary guarantees:

- Vertex provides decentralized ordering, finality, and resilient coordination between peers.
- Noir provides private constraint validation with proofs that can be checked from the public coordination record.

The project is successful when agents can coordinate on a real shared decision, publish only the public information the protocol requires, and leave behind an auditable Proof of Coordination that a third party can verify independently.

:::

::: reviewed {by="Xen" date="2026-04-19"}

## Current Delivery Context

The current milestone is shaped by the Vertex Swarm Challenge 2026 Track 3 constraints.

That context matters because it gives the first implementation a sharp target:

- prove leaderless multi-agent coordination
- show deterministic resolution without a central orchestrator
- handle at least one adversarial or invalid behavior path visibly
- leave behind a clear public record that demonstrates correctness, resilience, and auditability

This context drives the scope of `v1`, but it is not the project's core identity.

:::

::: locked {reason="core project responsibilities"}

## Responsibilities

- Define a coordination protocol where agents commit to private intent and participate in public consensus without exposing sensitive fields.
- Use real Noir proofs in each agent so proposed outcomes can be validated against private constraints locally.
- Use Vertex directly from Rust as the primary coordination transport and ordering substrate, with a deterministic local mirror for testing and local development.
- Complete coordination correctly when up to `f` of `3f+1` nodes are adversarial, silent, or drop mid-round within the validated `v1` baseline.
- Produce a public coordination record and verifier report for every run.

:::

::: locked {reason="v1 scope boundaries"}

## Non-Goals

- Full privacy for all public capability information in `v1`
- Economic optimality or market-fairness claims beyond deterministic validity
- Centralized proving helpers or mock-proof substitutes as the main architecture
- Blockchain settlement, token economics, or Arc integration
- Production-grade proving optimization before correctness is established
- A fully general marketplace schema for every future provider attribute
- Real downstream provider task execution such as compute jobs, model inference, or circuit-development work in the first delivery slice

:::

::: locked {reason="initial implementation slice and configurable capability framing"}

## V1 Scope

The first delivery slice is a compute-task matching protocol with these boundaries:

- Roles: one requester and one or more providers
- Primary validated topology: runtime-configurable system with a 4-node baseline of 1 requester plus 3 providers
- Matching model: requester publishes a coarse public capability need, providers publish public capability claims, and price constraints remain private
- Capability surface: runtime-configurable coarse capability tags, with `GPU`, `CPU`, `LLM`, and `ZK_DEV` as illustrative examples for the first delivery context
- Execution model: requester-side proof plus the matched provider's signed completion receipt form the finalized public execution evidence for `v1`; a distinct persisted requester acknowledgement artifact is deferred
- Round model: fallback rounds are required when proposals or proofs fail
- Noir scope: `v1` is satisfied by real per-agent Noir proofs over the committed private constraints needed for the requester and provider acceptance checks; additional circuit complexity is not required unless a private constraint depends on it

`ZK_DEV` refers to agents offering zero-knowledge circuit engineering or proof-workflow services. It does not refer to the protocol outsourcing its own proof generation or verification.

`v1` is allowed to use only a subset of illustrative capability tags in the first demo run.

A staged bring-up is allowed while building `v1`: a minimal viable circuit may prove one structural property end-to-end before the full requester/provider predicate set lands, but `v1` is not complete until the full private constraint predicate set is implemented.

:::

::: locked {reason="core architecture and protocol semantics"}

## Core Model

### Roles

- **Requester**: owns a task, a coarse public required capability tag, and private economic or policy constraints
- **Provider**: advertises public capability claims and holds private reservation constraints
- **Proposer**: derives a candidate match from public information and the current round state
- **Verifier**: reads the public record and validates that the finalized coordination outcome is structurally sound

### Public vs Private

Public data in `v1`:

- agent identity or stable public key
- round number
- requester coarse required capability tag
- provider capability claims
- proposal metadata
- proof artifacts, signatures, execution receipts, and verifier-facing bundle metadata

Private data in `v1`:

- requester budget and finer preferences
- provider reservation price and finer constraints
- private witness material required to generate Noir proofs

### Match Rule

`v1` does not attempt to prove optimal market clearing. It proves valid private coordination.

- Candidate formation uses public compatibility signals.
- Agents validate candidate outcomes against their private constraints locally.
- When more than one feasible provider exists, the deterministic winner is selected by stable public key order.
- Any invalid proposal or invalid proof advances the protocol into a fallback round with the next proposer.

### Invariants

- The public coordination record is sufficient for third-party outcome verification. No private input is required at any point in the verifier path.
- The match predicate is a single logical function with two implementations: Rust runtime logic and Noir circuit logic. Divergence between them is a correctness bug, not a performance tradeoff.

:::

::: reviewed {by="Xen" date="2026-04-19"}

## Structure

```text
private requester/provider intent
            |
            v
commitment + round binding + public capability claims
            |
            v
Vertex-ordered coordination log
            |
            +--> proposer derives candidate match from public state
            |
            +--> each relevant agent proves local validity with Noir
            |
            +--> proofs and signatures finalize Proof of Coordination
            |
            +--> matched provider publishes signed completion receipt
            |
            v
verifier checks coordination log and reports validity
```

:::

::: reviewed {by="Xen" date="2026-04-19"}

## Implementation Shape

The project should be built as a hybrid Rust system:

- a reusable library containing protocol types, round logic, commitment rules, verifier logic, and shared coordination behavior
- CLI agents that run requester and provider processes against Vertex
- Noir circuits that each agent can invoke locally to prove match validity against private constraints

The coordination transport for `v1` is Vertex directly. A deterministic local mirror of the same protocol loop is part of the implementation shape for tests and local development. FoxMQ is not a primary requirement for the first implementation path.

The proof model for `v1` has two implementation paths over the same Noir circuits:

- Default path: Noir ACIR `execute` validates the requester and provider constraints locally and emits verifier-facing proof artifacts bound to the public round inputs.
- Optional backend path: a full proof backend can replace the default attestation path when enabled, without changing the public protocol flow.

Receipt signing in the current implementation is Ed25519-primary. The durable `v1` contract is a provider-signed completion receipt that a third-party verifier can check from public data alone.

:::

::: reviewed {by="Xen" date="2026-04-19"}

## Coordination Flow

1. Agents start with private intents and stable identities.
2. Each agent publishes a commitment and the public capability information required for candidate formation.
3. Vertex finalizes the ordered round state.
4. The current proposer derives a candidate match from public state.
5. Relevant agents generate real Noir proofs locally and publish proof artifacts plus signatures.
6. If proofs and signatures validate, the match becomes the Proof of Coordination.
7. The matched provider publishes a signed completion receipt bound to the finalized round and matched capability.
8. A verifier reads the public record and produces a report from public inputs alone.
9. If a proposal or proof fails, the system advances deterministically into a fallback round.
10. A distinct requester acknowledgement artifact may be added later as a protocol enhancement, but it is not required for the current `v1` finalized record.

:::

::: reviewed {by="Xen" date="2026-04-19"}

## Examples

### Happy path

- Requester publishes public need `GPU` with private budget.
- Three providers publish capability claims; two claim `GPU`, one claims `CPU`.
- The proposer selects a `GPU`-capable candidate by stable public key order.
- The requester and selected provider each prove the candidate satisfies their private constraints.
- Vertex finalizes the proofs and signatures.
- The winning provider emits a signed completion receipt.
- The verifier report marks the coordination log valid.

### Invalid proof path

- A provider publishes a proof artifact that does not verify against the public round inputs.
- The proof is rejected visibly in the coordination record.
- The current round does not finalize.
- The protocol advances to the next proposer and retries deterministically.

:::

::: reviewed {by="Xen" date="2026-04-19"}

## Success Criteria

- Agents coordinate without exposing private price constraints in plaintext.
- The first serious end-to-end run works with a validated 4-node baseline and a runtime-configurable topology.
- Real Noir proofs are generated and verified as part of the coordination flow.
- Invalid-proof rejection is visible in the public record.
- Prior-round proofs cannot be replayed into the active round.
- No agent key can commit twice in a single round.
- Coordination completes correctly or aborts verifiably when a node is silent or drops mid-round within the validated `v1` baseline.
- Every run produces a public artifact bundle sufficient for third-party verification from public inputs alone.
- The system remains understandable as a standalone project and not only as an event artifact.

:::

::: reviewed {by="Xen" date="2026-04-19"}

## Synced Implementation Details

> Synced on: 2026-05-05

### Transport Model

- The primary multi-process runtime path is Vertex-backed and runs over a real `tashi-vertex` transport in the agent CLI.
- The same coordination runtime also runs over a deterministic local mirror for repeatable tests, local development, and the default network-free CI path.
- Transport is an implementation boundary, not a protocol fork: both paths drive the same round lifecycle, proposal logic, proof flow, receipt emission, and verifier-facing artifacts.

### Proof Model

- Requester and provider validation are implemented as real Noir circuits invoked locally by each agent.
- The default execution path validates the Noir constraints through ACIR `execute` and emits proof artifacts whose public inputs are bound to `(round, node_id, commitment_hash, role)`.
- An optional full-proof backend can be enabled without changing the higher-level protocol contract.
- Divergence between Rust runtime predicate logic and Noir circuit logic remains a correctness bug.

### Artifact Schema

Each run writes a verifier-facing public bundle. The implemented bundle schema is:

- `coordination_log.json`
- `verifier_report.json`
- `run_status.json`
- `completion_receipt.json` when finalized
- `bundle_README.md`
- `topology.toml`
- `scenario.toml` when a scenario was supplied

The coordination log includes commitments, proposals, proofs, receipts, rejection traces, final-round metadata, and abort metadata. The verifier report and run status summarize whether the run finalized cleanly or aborted coherently.

- The deterministic `demo` path writes one bundle directly at the requested artifact directory.
- The Vertex-backed `node` path writes the same bundle schema under `<artifacts>/<node-alias>/` for that process.
- The `demo-bft` workflow produces one verifier-facing bundle per spawned node, each independently re-verifiable through the standalone verifier.

### Signing Model

- The matched provider signs the completion receipt with Ed25519 in the current `v1` implementation.
- The verifier recomputes the canonical public receipt message from provider id, round, and matched capability, then verifies the Ed25519 signature using the provider's configured public key.
- The topology file carries the provider's public verification key as `signing_public_key`, while the separate private-intent bundle may carry the matching `signing_secret_key`; private signing material is never persisted into public artifacts.
- A distinct requester acknowledgement artifact is not part of the current finalized public contract.

### Configuration Surfaces

- `topology.toml` is the public runtime configuration surface: `version`, `capability_tags`, and `nodes`, where each node declares its role, public capability data, and optional `signing_public_key`.
- Topology validation is part of the durable runtime contract: the loader enforces exactly one requester, at least one provider, unique 64-character lowercase hex node ids, requester `required_capability`, and provider `capability_claims` drawn from the configured capability-tag set.
- The private-intent bundle is a separate TOML surface cross-validated against the topology and carries requester budget, provider reservation price, and optional `signing_secret_key` values.
- The `demo` path defaults the private-intent input to a sibling `<topology-stem>.private.toml` file when `--private-intents` is omitted.
- The verifier-facing artifact bundle intentionally copies only public configuration surfaces. Private-intent files remain off-bundle and are never needed for verification.

### Observability Model

- Runtime observability is public-only and surfaces protocol milestones for commitments, proposals, proof acceptance, receipt publication, finalized rounds, and coherent aborts.
- The deterministic local path and the Vertex-backed path both expose the same protocol milestones so operators can inspect behavior consistently across local development and multi-process execution.
- Observability is descriptive and behavioral, not a durable dependency on any particular stdout prefix.

### CLI And Runtime Surfaces

- `demo` runs the deterministic local path and writes one public artifact bundle.
- `demo` rotates an existing artifact directory to `<dir>.prev-N` by default and supports in-place overwrite of owned bundle files when explicitly forced.
- `verify` re-verifies a saved artifact bundle from public inputs alone and refreshes the persisted verifier report and run status idempotently.
- `node` runs one agent process on the Vertex-backed transport, writes a per-node public bundle, supports `--persist` for repeated sessions, and supports `--rejoin` when re-entering a live local cluster.
- `node --persist` reuses the same per-node bundle directory as a rolling latest-session snapshot and suffixes each persisted session run id as `<run_id>-rNNN`.
- `node` requires a real Vertex transport secret via `--secret-env` or `--secret`, with `--secret-env` preferred so the transport secret does not land in process listings.
- `demo-bft` launches the validated local multi-process Vertex workflow, generates ephemeral Vertex transport identities for the spawned cluster, supports failure injection and rejoin testing, and writes one bundle per node.

## Resolved Decisions

- Commitment construction shared between Rust and Noir is finalized in implementation.
- Proof artifact format carried in coordination messages is finalized in implementation.
- Stable provider ordering is finalized by stable public key order.
- Capability encoding is finalized as runtime-configurable coarse capability tags shared across runtime and circuit inputs.
- Public bundle packaging is finalized as a single-bundle deterministic local path plus per-node bundles for the Vertex-backed multi-process path.
- Public topology configuration and separate private-intent configuration are finalized as distinct surfaces with signing keys split into public verification material and private signing material.
- Deterministic local runs treat sibling `<topology-stem>.private.toml` discovery as the default private-intent convention.
- Persistent single-node sessions treat the per-node bundle directory as the latest public snapshot and derive session-scoped run ids from a stable `<run_id>-rNNN` pattern.

:::
