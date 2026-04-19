# Vertex Veil Intent

> Enable leaderless agents to coordinate on private intent and produce an auditable Proof of Coordination without relying on a central orchestrator.

Status: draft
Last updated: 2026-04-19

## Vision

Vertex Veil is a system for leaderless coordination over private intent.

Its purpose is to let autonomous agents reach a valid shared outcome without revealing sensitive constraints in plaintext and without deferring trust to a master orchestrator. The project combines two complementary guarantees:

- Vertex provides decentralized ordering, finality, and resilient coordination between peers.
- Noir provides private constraint validation with proofs that can be checked from the public coordination record.

The project is successful when agents can coordinate on a real shared decision, publish only the public information the protocol requires, and leave behind an auditable Proof of Coordination that a third party can verify independently.

## Current Delivery Context

The current milestone is shaped by the Vertex Swarm Challenge 2026 Track 3 constraints.

That context matters because it gives the first implementation a sharp target:

- prove leaderless multi-agent coordination
- show deterministic resolution without a central orchestrator
- handle at least one adversarial or invalid behavior path visibly
- leave behind a clear public record that demonstrates correctness, resilience, and auditability

This context drives the scope of `v1`, but it is not the project's core identity.

## Responsibilities

- Define a coordination protocol where agents commit to private intent and participate in public consensus without exposing sensitive fields.
- Use real Noir proofs in each agent so proposed outcomes can be validated against private constraints locally.
- Use Vertex directly from Rust as the primary coordination transport and ordering substrate.
- Produce a public coordination record and verifier report for every run.
- Provide a reusable library plus runnable CLI agents, not just a one-off demo script.

## Non-Goals

- Full privacy for all public capability information in `v1`
- Economic optimality or market-fairness claims beyond deterministic validity
- Centralized proving helpers or mock-proof substitutes as the main architecture
- Blockchain settlement, token economics, or Arc integration
- Production-grade proving optimization before correctness is established
- A fully general marketplace schema for every future provider attribute
- Real downstream provider task execution such as compute jobs, model inference, or circuit-development work in the first delivery slice

## V1 Scope

The first delivery slice is a compute-task matching protocol with these boundaries:

- Roles: one requester and one or more providers
- Primary validated topology: runtime-configurable system with a 4-node baseline of 1 requester plus 3 providers
- Matching model: requester publishes a coarse public capability need, providers publish public capability claims, and price constraints remain private
- Capability surface: runtime-configurable coarse capability tags, with `GPU`, `CPU`, `LLM`, and `ZK_DEV` as illustrative examples for the first delivery context
- Execution model: the matched provider emits a signed completion receipt and the requester acknowledges it
- Round model: fallback rounds are required when proposals or proofs fail

`ZK_DEV` refers to agents offering zero-knowledge circuit engineering or proof-workflow services. It does not refer to the protocol outsourcing its own proof generation or verification.

`v1` is allowed to use only a subset of illustrative capability tags in the first demo run, but the protocol shape should support custom coarse capability labels and leave room for richer future provider attributes.

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
- proof artifacts, signatures, and execution receipts

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

## Implementation Shape

The project should be built as a hybrid Rust system:

- a reusable library containing protocol types, round logic, commitment rules, verifier logic, and shared coordination behavior
- CLI agents that run requester and provider processes against Vertex
- Noir circuits that each agent can invoke locally to prove match validity against private constraints

The coordination transport for `v1` is Vertex directly. FoxMQ is not a primary requirement for the first implementation path.

## Coordination Flow

1. Agents start with private intents and stable identities.
2. Each agent publishes a commitment and the public capability information required for candidate formation.
3. Vertex finalizes the ordered round state.
4. The current proposer derives a candidate match from public state.
5. Relevant agents generate real Noir proofs locally and publish proof artifacts plus signatures.
6. If proofs and signatures validate, the match becomes the Proof of Coordination.
7. The matched provider publishes a signed completion receipt and the requester acknowledges it.
8. A verifier reads the public record and produces a report.
9. If a proposal or proof fails, the system advances deterministically into a fallback round.

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

## Success Criteria

- Agents coordinate without exposing private price constraints in plaintext.
- The first serious end-to-end run works with a validated 4-node baseline and a runtime-configurable topology.
- Real Noir proofs are generated and verified as part of the coordination flow.
- Invalid-proof rejection is visible in the public record.
- Every run produces a coordination log and verifier report.
- The system remains understandable as a standalone project and not only as an event artifact.

## Open Decisions

- Exact commitment construction shared between Rust and Noir
- Exact proof artifact format carried in coordination messages
- Canonical definition of stable public key order
- Whether non-winning providers emit explicit no-objection attestations in `v1`
- Exact capability encoding shared between Rust and Noir
- How future attributes like latency, storage, PIR, and richer service qualities enter later schema versions
