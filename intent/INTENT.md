# Vertex Veil Intent

> Build a Track 3 hackathon submission that proves leaderless, privacy-preserving agent coordination on Tashi by combining Vertex consensus with Noir-based intent validation.

Status: draft
Last updated: 2026-04-19

## Vision

`vertex-veil/` is the execution surface for a fresh Praxis-powered Intent-Driven Development project based on the Secured Intention Coordination seed bundle in `vertex-swarm.praxis/pages/seed___*.md`.

The project goal is to demonstrate that a swarm of at least three agents can negotiate, commit, execute, and verify a shared outcome without a master orchestrator and without exposing private intent data in plaintext. This directly targets Vertex Swarm Challenge 2026 Track 3, which requires a leaderless agent coordination layer with a full negotiate -> commit -> execute -> verify loop.

The thesis we are carrying forward from the seed is:

- Tashi/Vertex provides leaderless ordering, finality, and resilient peer-to-peer coordination.
- Noir-based zero-knowledge proofs provide intent confidentiality and verifiable constraint satisfaction.
- Together they enable private coordination correctness without falling back to a centralized orchestrator.

The initial v1 scenario is private compute task matching:

- One requester agent publishes a private budget and private capability requirement via a commitment.
- Two or more provider agents publish private reservation prices with public capability tags.
- A deterministic proposer suggests a match.
- Each participating agent proves the proposed match satisfies its committed constraints.
- The swarm finalizes a public Proof of Coordination that is auditable without revealing raw private intent fields.

## Why This Project

This project is shaped by two concrete inputs:

- `references/vertex-swarm-challenge/track3.md`: the hackathon requires agent discovery, leaderless agreement, execution, and a verifiable record of who did what and when.
- `references/vertex-hackathon-guide/README.md`: FoxMQ is the low-friction path for decentralized messaging, while Vertex remains the underlying consensus primitive for deterministic ordering and resilience. We favor using Vertex and building upon our experience implementing `references/warmup-vertex-rust/` for the hackathon warmup challenge.

For the hackathon submission, the demo must visibly satisfy these judging dimensions:

- Coordination correctness: no double assignment and deterministic resolution from a shared public record.
- Resilience: the swarm continues or fails cleanly when agents drop or messages are delayed.
- Auditability: a verifier can validate the coordination record independently.
- Security posture: replay resistance, invalid proof rejection, and no plaintext intent leakage.
- Developer clarity: the repo must be runnable and the demo flow easy to follow.

## Architecture Overview

```text
private intents
    |
    v
commitments + round binding
    |
    v
FoxMQ / Vertex ordered coordination log
    |
    +--> deterministic proposer selects candidate match
    |
    +--> agents generate Noir proofs against proposed match
    |
    +--> valid proofs + signatures finalize Proof of Coordination
    |
    v
third-party verifier checks public record without private inputs
```

Working architectural boundaries, inherited from the seed:

- Tashi owns message ordering, finality, BFT coordination, and peer-to-peer transport semantics.
- Noir owns commitment validation, round binding, and proof of match validity against private constraints.
- Application code owns the intent schema, proposer rotation, round lifecycle, and demo orchestration.

Current v1 architectural lean:

- Use FoxMQ for the coordination message surface because it is the fastest path to a runnable Track 3 demo.
- Keep matching computation off-circuit and use a propose-and-verify pattern to reduce proving complexity.
- Treat the public coordination record as the canonical artifact for auditability.

## Initial Scope

The first implementation pass should establish these foundations:

- Intent schema for requester and provider roles
- Commitment function with round-number binding
- Deterministic proposer and match predicate
- Noir circuits that verify committed intents against a proposed match
- Agent runtime skeleton that can commit, observe, prove, sign, and react to finality
- FoxMQ/Vertex integration for ordered coordination messages
- Standalone verifier for the public coordination record
- End-to-end demo command for the happy path

## Non-Goals

- Network-layer anonymity or metadata privacy
- Arc or blockchain settlement integration
- General-purpose intent language design
- Production-grade proving performance optimization
- Autonomous agent intelligence beyond scripted coordination behavior
- Broad multi-scenario support before the v1 compute-matching demo works

## Constraints

- The repo starts from an empty project area and should stay lightweight until the core intent is validated.
- The submission must remain hackathon-scoped and runnable on a laptop.
- The minimum credible demo is at least three agents completing a full coordination loop.
- No plaintext private intent fields should appear in coordination messages.
- Proofs must be bound to the round number to make replay across rounds invalid by construction.
- The coordination outcome must be independently verifiable from the public record alone.

## Success Criteria

- At least three agents complete discover -> commit -> match -> execute -> verify in one runnable demo.
- One malicious behavior path is demonstrably rejected by the system.
- One dropout path is handled by either clean completion or explicit abort with a verifiable reason.
- A third-party verifier confirms the public coordination record without access to private intents.
- The full demo completes in under 10 seconds on a laptop, or any gap is explicitly documented.

## Source Seeds

- `vertex-swarm.praxis/pages/seed___intent.md`
- `vertex-swarm.praxis/pages/seed___glossary.md`
- `vertex-swarm.praxis/pages/seed___architecture.md`
- `vertex-swarm.praxis/pages/seed___scenario.md`
- `vertex-swarm.praxis/pages/seed___decomposition.md`
- `references/vertex-swarm-challenge/details.md`
- `references/vertex-swarm-challenge/track3.md`
- `references/vertex-hackathon-guide/README.md`

## Next IDD Steps

- Use `/intent-interview` or direct editing to refine the root intent into explicit project decisions.
- Split the seed decomposition into phased implementation work once stack choices are pinned.
- Add project-local guidance in `vertex-veil/AGENTS.md` when the implementation conventions become clear.
