# Orchestration pattern catalogue — Security review

- **reviewPassId:** `langgraph-pattern-catalog-security-a3c77b7-20260909`
- **Role:** Security
- **Mode:** specialized role, dedicated review-only pass
- **Reviewed commit:** `a3c77b7ea67c86786028e9ca544f9f545e44b9c5`
- **Review worktree:** detached, clean before and after
- **Agent:** Codex; session, provider and model identifiers are not exposed by the current harness

## Subject integrity

No contract or vector is changed. File SHA-256 values are README
`dfccb49f5dbd68d6d04df108dcd5755b5e532b2fffebea85c83b86c24774a99e`,
catalogue `609f95fe3fa3353473d05bcd04b4c103da0e15fd138f1dac421ce8b57e4b06c7`,
manifest `46aca05956b56fbec7140a30865cf549a6bb8544e682962e778fdeefc749113f`,
tests `c2b1a7cca07263f0bc6d1beab96e8b23fe0c332a3128b1dc31c86e7215a3ad84`,
validator `2b682da631960e4926f82a149b54656981cc74105a3c9f9452ceb83b5d3dc4b3`
and coverage config `aa70c7ba62cadbca0397f4142c3a3ea96a8f6f0648043ac4f7377f6da60eb67a`.

## Security assessment

- Findings expose only closed codes and paths built from fixed field names and numeric array indexes; rejected strings are never interpolated (`tools/quality/orchestration-pattern-catalog.ts:75`).
- A sentinel probe produced only `catalog.*` codes and fixed paths, with no sentinel reflection. The committed non-reflection test enforces the same property (`tools/quality/orchestration-pattern-catalog.test.ts:208`).
- Non-JSON objects and hostile prototype inspection fail closed (`tools/quality/orchestration-pattern-catalog.ts:55`).
- Source provenance requires HTTPS GitHub coordinates, a full lowercase commit SHA, an ISO date and MIT declaration; the documentation states that structure does not prove the declaration true.
- LangGraph, LangChain and LangSmith identifiers are rejected from candidate contract names; refused and worker-internal dispositions cannot nominate contracts (`tools/quality/orchestration-pattern-catalog.ts:204`).
- Upstream content is documented as untrusted read-only data that is neither executed nor installed and receives no secret, PII or private context (`docs/research/orchestration-patterns/README.md:30`).
- The increment adds no dependency, network call, managed service, runtime capability or lockfile delta.

## Reproduced evidence

- Sentinel non-reflection probe: exit 0; output contained four fixed finding code/path pairs only.
- Catalogue CLI: exit 0 on the committed catalogue.
- Full `bun run check`: secret scan, personal-data boundary, capability boundary, specification lock, coverage, lint and typecheck all exit 0.
- `cargo test --locked --all-features`: 19 tests, 0 failures.
- Lockfile diff and final detached-worktree status are empty.

## Findings

- Blocking: none.
- Major: none.
- Minor: none.
- Non-blocking: none.

## Residual risks

The structural gate cannot prove upstream truth or detect every semantic alias of
a framework name. The local, review-mediated catalogue has no automated remote
ingestion; any future remote or high-volume ingestion must add hard byte/item
limits, guarded untrusted-content handling and a separate security review.

## Verdict

`approve`
