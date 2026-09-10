# Architecture review — native authorized execution

- Reviewed implementation SHA: `1ae6b881f39bd731be2cc5326ba9de982afbd431`
- Recorded at (UTC): `2026-09-10T16:08:55Z`
- Role pass: architecture
- Verdict: `approve`

## Scope and authority

The implementation is an additive `0.2.0` effect-free decision surface under
ADR-0037 / D43 / WP-G3-O02. Contract authority remains external and immutable:
Contracts is pinned at `5b9b6668909119b670e0db62174419ab04e5b402`
and SDK Rust at `ac9f2020425733183839a58fc2c3928a4de5c066`.
Wire documents are validated by the registry before private normalization; no
contract shape is redefined as a public Orchestrator authority.

Graph validation, route selection, causal replay, human decision, generation
transfer and effect continuity remain pure functions. Deterministic ordering is
implemented with ordered collections or exact precedence, while returned
applications are values only and perform no I/O. The complete public surface and
stable codes are pinned by compatibility snapshots with the `0.2.0` version and
break journal.

LangGraph is absent from Cargo and Bun dependency graphs. Its catalogue is
non-normative and can only supply questions or failure scenarios; neither
Missions nor the locked wire contracts depend on it.

No Phase 4B capability entered `src/`: there is no store, worker invocation,
network, filesystem, process, environment, secret or clock access. The design
does not claim WP-G3-O01.

## Findings

- Blocking: none.
- Major: none. The stale v1/v2 contract documentation found during the pass was
  corrected before this reviewed SHA.
- Minor: none.

## Residual boundary

Transactional PostgreSQL serialization, a real executor boundary, runtime logs
and retention/deletion/restore remain separate blocking proofs. This approval
does not authorize them.
