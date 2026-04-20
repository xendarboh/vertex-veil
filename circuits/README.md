# Noir Circuits (Phase 2+)

Phase 0 reserves this directory for the per-agent Noir circuits introduced in
Phase 2 of `intent/plan.md`. No circuits live here yet.

Planned layout once Phase 2 lands:

```
circuits/
  requester/         # Nargo project: requester acceptance predicate
  provider/          # Nargo project: provider acceptance predicate
  shared/            # Library with commitment helpers and predicate parity hooks
```

The Rust side of the predicate (shared parity fixtures) lives in
`crates/vertex-veil-core`.
