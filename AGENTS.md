# Vertex Veil

Vertex Veil should remain understandable as a standalone, publishable project.

## Purpose

- Build leaderless coordination over private intent.
- Use Vertex for decentralized ordering and finality.
- Use Noir for private constraint validation.
- Produce a public Proof of Coordination and verifier-facing artifacts without exposing private inputs in plaintext.

## IDD Contract

- Treat `intent/INTENT.md` as the primary contract.
- Treat `intent/decisions.md` as rationale and decision history.
- Treat `intent/plan.md` as the current execution contract.
- Treat `intent/TASK.yaml` as status only, not architecture.
- If code and intent diverge, surface it explicitly; do not silently normalize the mismatch.

## Workflow

- Follow the IDD loop: `Intent -> Review -> Plan -> Test -> Code -> Sync`.
- Prefer updating intent artifacts before changing architecture, protocol shape, proof flow, persistence format, or public artifact schemas.
- Use human review to lock intent quality; use implementation work to satisfy the approved intent, not replace it.
- After implementation and passing tests, sync confirmed details back into the intent artifacts.

## Loading Policy

- Load only the files needed for the current task.
- Start in `intent/` for scope, architecture, sequencing, acceptance criteria, and non-goals.
- Start in implementation files for localized fixes when the relevant intent is already clear.
- Read intent artifacts before making changes that affect durable public behavior or verifier-facing outputs.

## Publication Guardrails

- Keep repository language publishable and self-contained.
- Do not rely on unpublished workspace context as project authority.
- Prefer durable names, explicit artifacts, and verifier-friendly behavior over demo shortcuts.
