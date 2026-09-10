# Integration review — native authorized execution

- Reviewed implementation SHA: `1ae6b881f39bd731be2cc5326ba9de982afbd431`
- Base: `origin/main` at `2295060afa03d13f0e953d82a82c2039ccdfe6a2`
- Governance authority: `25778623d10150fe1ffb6ab8806e35fba7af5245`
- Recorded at (UTC): `2026-09-10T16:08:55Z`
- Verdict: `approve`

## Reproducible proof

Every command below ran against the reviewed implementation SHA and exited 0:

- `cargo fmt --all -- --check`
- `cargo test --locked --all-features` — 52 tests plus one README doctest;
  includes one Rust test that executes all 54 locked semantic cases and five
  fake-journal/fake-executor crash tests.
- `cargo clippy --locked --all-targets --all-features -- -D warnings`
- `cargo bench --bench authorized_execution_replay` — raw metrics are recorded
  in `benchmark.txt`.
- `bun run check` — review tooling, capability, secret, personal-data,
  specification, authority, coverage configuration, pattern catalogue, lint and
  strict TypeScript gates all green.
- `cargo deny check licenses` — `licenses ok`.
- `reuse lint` — 74/74 files carry copyright and licence information.
- `cargo llvm-cov --locked --all-features --lcov --output-path coverage/lcov.info
  --fail-under-lines 87 --fail-under-functions 90` — report generated; 87.91%
  lines and 90.20% functions. A deliberate 88% line threshold exits 1, proving
  the gate is active.

The CI installs `cargo-llvm-cov 0.9.1` from the official immutable release with
SHA-256 `3fca950394a3c49457657c158b1619cec8dfd2647ae5b48746734c0ab969a522`.
The repository test pins the version, digest, report path and both thresholds.

## Findings and disposition

- Blocking: none.
- Major: none open. Two review findings were remediated before this SHA: stale
  contract-version documentation and absence of blocking Rust coverage.
- Minor: none.

## Rollback and non-claims

Before a consumer pins Phase 4A, rollback is a revert of its additive commits.
After a consumer exists, pin that consumer to the prior Orchestrator revision
before reverting. No canonical event migration exists because Phase 4A adds no
contract or stored-state schema.

This approval covers the pure native core only. Transactional PostgreSQL
serialization, executor point-of-effect idempotency/fencing, allow-listed
zero-PII runtime logs, and retention/deletion/restore remain blocking. It does
not claim WP-G3-O01, a run boundary, real effects or deployment.
