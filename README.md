# Vertex Veil

<div align="center">
  <img src="./img/vertex-veil-002--720x720.png" alt="VV-Logo" width="720">
</div>

**Private-intent coordination with ZK proofs, anchored by Tashi Vertex BFT
consensus.** Agents publish commitments, negotiate a match, and settle a
signed completion receipt — without ever putting their private price
constraints on the wire. Every run produces a third-party-verifiable
public record.

## Overview

Say you have a requester who needs a GPU job run under a private budget,
and three providers each with a private reservation price. You want them
to reach a deal that satisfies both sides' constraints — without either
side revealing their number. And you want anyone — a judge, a regulator,
a counterparty — to later confirm the deal was valid, from a public log
alone.

That's Vertex Veil:

- Each agent commits to its private intent with a hash (Blake2s over a
  canonical preimage).
- A Noir ZK circuit proves the match predicate (capability +
  budget ≥ price) holds, without revealing either private value.
- Consensus ordering — Tashi Vertex in production, an equivalent
  deterministic in-process `OrderedBus` for demos — keeps every agent
  on the same view of the round.
- Fallback rounds handle dropped nodes, tampered proofs, replays, and
  double-commits — each visibly recorded as a rejection in the public
  log.
- The matched provider signs a completion receipt with ed25519. A
  standalone verifier re-checks everything from public inputs alone.

## Architecture

```
┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
│ agent n1 │   │ agent n2 │   │ agent n3 │   │ agent n4 │
│ requester│   │ provider │   │ provider │   │ provider │
└────┬─────┘   └────┬─────┘   └────┬─────┘   └────┬─────┘
     │              │              │              │
     └──────────────┴──────┬───────┴──────────────┘
                           ▼
              ┌──────────────────────────┐
              │       Vertex Swarm       │
              └──────────────┬───────────┘
                             ▼
   commitments ─► proposal ─► proofs ─► receipt
                             │
                             ▼
             ┌─────────────────────────────────┐
             │  coordination_log.json          │
             │  verifier_report.json           │
             │  completion_receipt.json        │
             │  run_status.json                │
             │  bundle_README.md               │
             └─────────────────────────────────┘
                             │
                             ▼
               cargo run … verify --artifacts …
                             │
                             ▼
                       valid = true
```

## Quick Start

```bash
# Build the circuits (Noir) and the workspace (Rust)
cd circuits && nargo compile --workspace && cd ..
cargo build -p vertex-veil-agents --release

# Run the narratable demo (one command)
cargo run --release -p vertex-veil-agents -- \
  demo --topology fixtures/topology-4node.toml \
       --scenario fixtures/replay-doublecommit-drop.toml \
       --artifacts artifacts/demo \
       --narrate

# Independently verify the bundle
cargo run --release -p vertex-veil-agents -- verify --artifacts artifacts/demo
```

## What you'll see

- `[COORD] commitment from nX round=0` — four hashes, one per agent.
- `[COORD] proposal by n2 (GPU) round=0` — proposer elects a match.
- `[COORD] proof verified for n1 round=0` — Noir ZK proof accepted.
- `[VERTEX] round 0 committed (finalized=false)` — round 0 falls back
  because the scenario injected a tampered proof for n2.
- `[VERTEX] round 1 committed (finalized=true)` — fallback round
  finalizes with a clean provider.
- `verify … valid=true` — public-only third-party verification.

## Capability criteria → where it lives

| Criterion                                    | Evidence in this repo                                                                                                                   |
| -------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| Coordination works                           | `demo … --narrate` produces a signed receipt and a `valid=true` verifier report from public inputs.                                     |
| Coordination works (Vertex handles failures) | `demo-bft` subcommand + `VertexTransport` (feature-gated). Adversarial rejection visible in `coordination_log.json` under `rejections`. |
| Auditability                                 | `coordination_log.json` + standalone `verify`.                                                                                          |
| ZK correctness                               | `circuits/requester/`, `circuits/provider/`, `circuits/shared/`. Parity tests in `crates/vertex-veil-core/tests/parity*.rs`.            |
| Privacy posture                              | `Secret<T>` wrapper, redaction in logs, public-only artifact schema.                                                                    |

## Repo map

- `crates/vertex-veil-core/` — protocol, commitments, round machine,
  standalone verifier, public artifact schema.
- `crates/vertex-veil-noir/` — Rust ↔ Noir bridge, proof
  generation/verification, UltraHonk feature gate.
- `crates/vertex-veil-agents/` — CLI binary: `demo`, `verify`, `node`
  (feature-gated), `demo-bft` (feature-gated).
- `circuits/` — Noir workspace: `requester`, `provider`, `shared`.
- `fixtures/` — topology + private-intent TOML, adversarial scenarios.
- `intent/` — Intent-Driven-Development artifacts (INTENT.md,
  decisions.md, plan.md, TASK.yaml).
- `docs/DEMO.md` — two-minute narration script.

## Demonstrate on the real Vertex substrate

The primary demo path uses an in-process `OrderedBus` that mirrors
Vertex's consensus ordering deterministically. The workspace also ships
a feature-gated real-Vertex path that compiles against
`tashi-vertex` and exposes two subcommands:

```bash
cargo build -p vertex-veil-agents --features vertex-transport
cargo run -p vertex-veil-agents --features vertex-transport -- node --help
cargo run -p vertex-veil-agents --features vertex-transport -- demo-bft --help
```

`VertexTransport` implements the same `CoordinationTransport` trait the
in-process demo uses — the protocol logic is transport-agnostic. See
`crates/vertex-veil-agents/src/node.rs` and
`crates/vertex-veil-agents/src/orchestrate.rs` for the
single-node + multi-process orchestrator implementations.

## Reproducibility

Every run produces:

- `coordination_log.json` — complete ordered public record.
- `verifier_report.json` — verifier decision.
- `run_status.json` — judge-facing summary (finalized, abort reason,
  file manifest).
- `completion_receipt.json` — ed25519-signed receipt (if finalized).
- `topology.toml` + `scenario.toml` — input configuration snapshot.
- `bundle_README.md` — human-readable walkthrough.

Repeated runs with the same fixtures produce byte-identical logs (tests:
`edge_artifact_packaging_deterministic`,
`edge_replay_doublecommit_reproducible`).

Prior bundles are rotated to `<artifacts>.prev-<N>` instead of clobbered.
