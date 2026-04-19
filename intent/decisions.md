# Interview Decisions: Vertex Veil

> Anchor: Enable leaderless agents to coordinate on private intent and produce an auditable Proof of Coordination without relying on a central orchestrator.

## Decisions

### 1. Project anchor

- **Question**: In one sentence, what is `vertex-veil/`'s reason to exist for this hackathon submission?
- **Decision**: The project exists to enable private coordination in a leaderless swarm and should stand on its own beyond the hackathon.
- **Rationale**: The user wants `vertex-veil/` to be publishable as an enduring standalone project rather than a repo whose identity is only a hackathon submission.

### 2. Thesis framing

- **Question**: How should the project's deeper thesis be framed so it stays durable beyond the current event?
- **Decision**: Frame the thesis as leaderless coordination over private intent: Tashi/Vertex provides deterministic decentralized ordering and finality, while Noir provides private constraint validation and public verifiability.
- **Rationale**: This captures the long-term project idea without tying its identity to a single event, while still preserving the current technical direction.

### 3. Delivery context

- **Question**: How should hackathon context be represented in the intent workflow?
- **Decision**: Treat the Track 3 hackathon as the current delivery context and scope constraint, not as the core project identity.
- **Rationale**: The hackathon is high-signal guidance for what to build now, but the published project intent should remain durable and standalone.

### 4. Primary audience

- **Question**: Who is the primary audience we should optimize the project for in v1?
- **Decision**: Optimize first for hackathon judges.
- **Rationale**: The implementation should visibly map to Track 3 judging criteria: correctness, resilience, auditability, security posture, and developer clarity.

### 5. Artifact shape

- **Question**: What shape should the v1 artifact take?
- **Decision**: Build a reusable reference framework, not only a one-off scripted demo.
- **Rationale**: The user explicitly wants the project to reflect the hackathon's "Systems Over Demos" emphasis and values reusable system depth alongside a runnable demonstration.

### 6. Stack bias

- **Question**: Which implementation bias should we lock in early?
- **Decision**: Favor Rust and Vertex directly.
- **Rationale**: The user wants to build on the `warmup-vertex-rust` experience, prefers the control and visibility of native Vertex integration, and does not currently see a compelling FoxMQ advantage for this use case.

### 7. Minimum coordination scenario

- **Question**: For the minimum demo loop, what should the agents actually coordinate on?
- **Decision**: Use compute task matching.
- **Rationale**: This aligns with the seed scenario and cleanly demonstrates negotiation, commitment, proof-based validity, and execution receipts in a Track 3 framing.

### 8. Privacy boundary for v1

- **Question**: What should stay private in v1, and what can be public to keep the demo tractable?
- **Decision**: Keep prices private and capability tags public.
- **Rationale**: This preserves the core privacy claim while keeping proposer logic and matching feasibility tractable for a first real ZK implementation.

### 9. Failure path required in the first demo

- **Question**: What failure path must be in the first demo, not deferred?
- **Decision**: Include visible invalid-proof rejection.
- **Rationale**: This gives the clearest early proof of adversarial handling and security posture using the public coordination record.

### 10. Match selection rule

- **Question**: Which deterministic selection rule should v1 use after providers are known to be feasible?
- **Decision**: Choose the winning provider by stable public key order.
- **Rationale**: The user explicitly pushed back on hash-based selection because it invites gaming concerns. Stable key order is simple, deterministic, and easy to reason about, while the writeup should claim validity over optimality rather than market fairness.

### 11. Fairness framing

- **Question**: How strong should the fairness claim be in the v1 writeup?
- **Decision**: Claim validity over optimality.
- **Rationale**: The system's first job is to prove private constraints are satisfied under leaderless coordination, not to prove the best economic outcome.

### 12. Capability surface

- **Question**: What kind of capability tags should the demo use so the scenario feels concrete?
- **Decision**: Use compute-oriented provider capability tags with an extensible shape. The initial tag set should include `GPU`, `CPU`, `LLM`, and `ZK`, even if the first demo run uses only a subset.
- **Rationale**: The user wants a compute marketplace flavor now while keeping a clear path toward richer cryptographic and intelligence service providers later.

### 13. Future provider attributes

- **Question**: How should richer provider attributes like PIR, LLM, storage, and latency be treated in v1?
- **Decision**: Build v1 with expansion in mind, but keep the first private constraint model centered on public capability claims plus private price constraints.
- **Rationale**: The user wants the design to support future richer attributes, but not at the cost of derailing the first implementation with too many simultaneous proof dimensions.

### 14. Execution semantics

- **Question**: How concrete should the post-match `execute` step be in the first demo?
- **Decision**: Use a simulated execution receipt.
- **Rationale**: The swarm still proves the full discover -> commit -> match -> execute -> verify loop without having to integrate a real downstream compute protocol in the first pass.

### 15. Round behavior

- **Question**: What multi-round behavior should v1 prove?
- **Decision**: Prove fallback rounds.
- **Rationale**: If a proposal or proof fails, the next proposer should advance the round deterministically. This is the clearest multi-round resilience behavior for the first system implementation.

### 16. Node topology

- **Question**: How should topology be framed for v1?
- **Decision**: The system should be runtime-configurable, with a primary validated baseline of 1 requester + 3 providers to align with `n=4, f=1` BFT testing.
- **Rationale**: The user wants dynamic versatility, but also wants the first E2E validation profile to reflect the minimum topology that actually exercises meaningful BFT resilience from the warmup learnings.

### 17. Delivery target

- **Question**: What delivery target should the Rust implementation favor first?
- **Decision**: Use a hybrid shape: reusable shared library plus runnable CLI agents.
- **Rationale**: This balances system depth and hackathon usability, matching the user's desire to build a real coordination framework with a clear demonstration surface.

### 18. Coordination transport

- **Question**: Which coordination transport should v1 treat as the primary implementation target?
- **Decision**: Use Vertex directly.
- **Rationale**: The user prefers native Rust, wants direct protocol control, and does not want to optimize around a FoxMQ adapter before there is a concrete need.

### 19. ZK runtime model

- **Question**: Where should Noir proving and verification live in the first architecture?
- **Decision**: Each agent proves locally and publishes real proof artifacts into the coordination flow.
- **Rationale**: The user wants a fully decentralized proof path with no central proving helper and no mock-proof shortcut.

### 20. ZK depth

- **Question**: How strict should we be about implementing real ZK in the first phase?
- **Decision**: Real Noir integration is required in v1.
- **Rationale**: The privacy claim is core to the project. The user explicitly rejected mock proofs and wants actual Noir-generated and verified proofs from the start.

### 21. Persistent artifacts

- **Question**: What should be the main persistent artifact from each run?
- **Decision**: Persist the coordination log and a verifier report.
- **Rationale**: These are the key audit artifacts for proving correctness, resilience, and judge-facing clarity.

### 22. Public/private wording

- **Question**: How should the public capability tags versus private prices tradeoff be described?
- **Decision**: Providers make public capability claims while requester requirements and pricing remain protected, with the requester's coarse required capability tag public when needed for deterministic candidate formation.
- **Rationale**: This keeps the story concrete and impactful without overclaiming fully private matching semantics that v1 does not yet implement.

### 23. Requester visibility

- **Question**: What should be public about the requester's need in v1 so the proposer can form candidate matches deterministically?
- **Decision**: Publish a coarse requester capability tag publicly, while keeping budget and any finer constraints private.
- **Rationale**: The proposer needs a deterministic public signal for candidate formation, but the sensitive part of requester intent remains protected.

### 24. Execution record shape

- **Question**: How much of the execution phase should be represented in the public record?
- **Decision**: Record a signed completion receipt from the matched provider plus requester acknowledgement.
- **Rationale**: This is enough to make the full coordination loop auditable without prematurely expanding into full result-publication semantics.

## Open Items

- Decide the exact commitment construction shared between Rust and Noir, likely Poseidon-based.
- Decide the concrete proof artifact format and verification flow for coordination messages.
- Decide whether non-winning providers emit explicit no-objection attestations in v1.
- Decide whether the provider key order is static config order, lexical pubkey order, or another canonical public ordering.
- Decide the initial capability representation in code: enum, bitflags, or field-friendly bitmask shared with Noir.
- Decide how much of future attributes like latency, storage, PIR, and richer service qualities should appear in the first schema versus a later schema revision.
- Decide the exact run artifact layout for saved coordination logs and verifier reports.

## Out of Scope

- Full privacy for all capability information in v1.
- Economic optimality or market fairness claims beyond deterministic validity.
- Central proving helpers or any mock-proof fallback as the main architecture.
- Blockchain settlement, Arc integration, or token economics.
- Real downstream PIR, storage, or inference execution in the first demo loop.
- A generalized marketplace schema covering all future provider attributes from day one.
