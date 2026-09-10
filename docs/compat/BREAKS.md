# Compat-policy breaks journal

Mechanized by `tests/compat_surface.rs` against `project.v1.yaml`'s
`compat-policy` exit criterion. Two surfaces are pinned: the symbols
`src/lib.rs` re-exports (`tests/compat/public_surface.snapshot`) and the
exhaustive stable code strings (`tests/compat/stable_codes.snapshot`). Both
are consumed as fact by anything that git-deps this crate.

`cargo test --locked` fails the moment either drifts from its snapshot. The
fix is never to only edit the snapshot: a real, intentional break is recorded
here — date, what changed, old value, new value, why — in the same commit
that bumps `version` in `Cargo.toml` and updates the snapshot file.

| Date | Crate version | Surface | Change | Reason |
| ---- | ------------- | ------- | ------ | ------ |
| 2026-09-10 | 0.2.0 | Public API and stable codes | Add the authorized-execution graph, routing and authority surface; retain all 0.1.0 symbols and behavior | Record the first coherent additive authorized-execution API before any consumer can pin it |
