# WP-G2-A01 verification

This directory proves the simulation-only boundary of `libre-ai-agent-orchestrator`.

The crate is a pure decision core. It validates:

- canonical `OrchestratorControl v1` documents without reflecting rejected values;
- exact run/tenant/mission/plan/authorization identity bindings;
- authority-bound preflight facts before returning permission to allocate a run ID;
- explicit store outages, authorization revocation, stale revisions and monotone cancellation;
- idempotent replay as a receipt-only result that never repeats the recorded effect;
- causal event arithmetic and plan-bound budget ceilings through the locked contract-types validator;
- effect admission only for an active, authorized `Running` simulation state.

It deliberately owns no ledger, process, filesystem, network, environment/provider, secret,
persistence, sandbox, harness or Pi adapter. `check-capabilities.ts` makes that boundary executable by
allowlisting the crate dependency surface and rejecting runtime source imports/entry points that can
perform OS effects.

The tests use synthetic opaque identifiers only. They do not start a mission or claim runtime
conformance. A later harness/worker package and fresh role-separated reviews remain mandatory before
any real effect.
