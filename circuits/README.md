# Noir Circuits

This workspace now contains the Phase 2 Noir circuits used by the Rust bridge
and parity tests.

Layout:

```
circuits/
  requester/         # Nargo project: requester acceptance predicate
  provider/          # Nargo project: provider acceptance predicate
  shared/            # Library with commitment helpers and predicate parity hooks
```

The Rust side of the predicate (shared parity fixtures) lives in
`crates/vertex-veil-core`.

Compiled circuit JSON artifacts are generated into `circuits/target/` and are
not checked into git. Recreate them with:

```bash
nargo compile --workspace
```

Run all Noir tests with:

```bash
nargo test --workspace
```
