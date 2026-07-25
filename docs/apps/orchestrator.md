# Orchestrator — Governed agent run control

- **Path:** `crates/agent-orchestrator` (control core, already locked by `WP-G2-A01`, simulation-only) and `crates/agent-orchestrator-run` (runtime, written by this specification's package). The split is deliberate: ADR-0004 §8 bounds the core to simulation against a fake harness, so capabilities land in a separate crate with its own review rather than widening an accepted one.
- **Owner:** Polaris / Orchestrator (run control for agent fleets)
- **Runtime:** Rust control core; no network, no secret, no provider at this stage (ADR-0018 D2)
- **Tenant model:** organization; every run carries the tenant of the authorization that opened it

## Purpose and actors

The orchestrator turns an authorized execution plan into a bounded, observable run, and refuses everything it was not authorized to do. It decides sequencing, applies budgets and control documents, records an append-only event chain, and halts. It never grants itself the right to act.

**Actors:**

- **Missions** (authority): owns workflow, quorum, execution authorization and the validation projection. It issues `execution-authorization.v1`; the orchestrator consumes it and cannot mint it (ADR-0004 §2).
- **Workers** (executors): replaceable RPC processes that perform steps under a harness profile. Their permissions, sessions and internal types are never a Libre AI security boundary (ADR-0004 §3).
- **Operators** (auditors): read run events, budgets consumed, refusals and attestation references; cannot mutate a closed run.
- **Owner** (nominative acts): pronounces the bootstrap hard stop of each new security-critical pattern in this layer (ADR-0011 D4).

**Doctrine constraints.** Deny-by-default authorization with mandatory agent facts — `agent_fleet`, `mission_agent`, `capability_scope` — and per-agent revocation that fails closed (K1). Tool output and worker stdout enter as `operational` and never justify a write to a source of truth (K2). Any untrusted payload reaching a model is wrapped in a signed envelope (K3). The run register is append-only (K5).

## Journeys

1. **Open a run.** The orchestrator receives an `execution-authorization.v1` naming tenant, mission, mission revision, mission record digest, plan and plan digest. It re-computes the plan digest from `execution-plan-body.v1` and refuses on mismatch. No authorization, no run.

2. **Apply a control document.** A control document sets budget ceilings and liveness limits for the run. It is parsed strictly, its command fingerprint recorded, and its limits bound to the run for its whole life; a control document cannot be widened mid-run.

3. **Sequence a step.** The orchestrator selects the next step of the plan, checks the agent's `capability_scope` covers it, requests execution under a named harness profile, and waits for the worker's result plus the harness attestation.

4. **Record an event.** Every decision, refusal, budget movement and step result is appended to the run's event chain as `orchestrator-event.v2`, each entry linked to its predecessor. Nothing is edited; a correction is a new event.

5. **Halt.** A run halts on plan completion, on a budget ceiling reached, on a liveness limit reached, or on a refusal. A halt produces a decision dossier — what was done, what remains, the cause — and never a silent kill (ADR-0011 D6).

6. **Inspect a closed run.** An operator retrieves the full event chain, the control document applied, the budgets consumed and every harness attestation referenced, and can re-verify each digest independently.

## Non-goals

- **Self-authorization.** The orchestrator never issues, widens or infers an execution authorization. Missions is the only authority.
- **Being a security boundary for the worker.** Pi's internal permission model is not trusted; confinement is the harness's job, not the worker's promise.
- **Executing without a harness.** A step with no harness profile and no attestation is refused, not run unconfined.
- **Network, secrets, providers, real tenant data.** Closed at this stage by ADR-0018 D2; each requires its own package and review.
- **Writing to CI or gates.** No agent token grants gate or workflow write by default (K1).
- **Editing history.** The event chain is append-only; a closed run is immutable.

## Domain protocol

**Commands:** `OpenRun`, `ApplyControlDocument`, `RequestStep`, `RecordStepResult`, `RecordRefusal`, `HaltRun`.

**Queries:** `GetRunState`, `ListRunEvents`, `GetBudgetLedger`, `ExplainRefusal`, `GetAttestationReferences`.

**Events:** `RunOpened`, `ControlApplied`, `StepRequested`, `StepCompleted`, `StepRefused`, `BudgetConsumed`, `LimitReached`, `RunHalted`.

Every event carries the run id, its predecessor digest, the tenant, and a monotonically increasing sequence. Replay of an event chain reconstructs run state exactly; divergence is a defect, not a tolerated drift.

## Refusal matrix

Refusals are closed and stable. Each names the failing invariant, never the payload.

- `orchestrator.authorization_missing` — a run was requested without an execution authorization.
- `orchestrator.plan_digest_mismatch` — the plan body does not hash to the authorized plan digest.
- `orchestrator.mission_revision_stale` — the authorization references a mission revision that is no longer current.
- `orchestrator.self_authorization_denied` — an attempt to open or widen a run without Missions.
- `orchestrator.capability_out_of_scope` — the step requires a capability outside the agent's `capability_scope`.
- `orchestrator.cross_mission_denied` — the agent's `mission_agent` fact does not match the run's mission.
- `orchestrator.agent_revoked` — the agent identity is revoked, or the revocation store is unavailable (fail closed).
- `orchestrator.control_document_invalid` — the control document is not strict-parseable or widens a bound limit.
- `orchestrator.budget_exceeded` — a budget ceiling of the control document is reached.
- `orchestrator.liveness_limit_reached` — a non-progress or retry ceiling is reached.
- `orchestrator.harness_attestation_missing` — a step result arrived without its harness attestation.
- `orchestrator.harness_profile_mismatch` — the attestation does not bind the profile that was requested.
- `orchestrator.event_chain_broken` — the predecessor digest of an event does not match the chain head.
- `orchestrator.run_closed` — a mutation was attempted on a halted run.
- `orchestrator.capability_not_enabled` — the step requires a capability closed at this stage (network, secret, provider, real tenant data).

## Data

PostgreSQL is authoritative, under the shared tenant barrier: `ENABLE` and `FORCE ROW LEVEL SECURITY`, policy on `current_setting('app.tenant_id')`, access only through `withTenantDbTransaction`.

Tables: runs (one row per run, closed state immutable), run_events (append-only, `GRANT SELECT, INSERT` only, primary key `(tenant_id, run_id, sequence)`), budget_ledger (append-only movements, never a mutable balance), attestation_refs (digest and profile id of each harness attestation, never the attested content).

Retention follows the executable policy: a closed run's events expire under `runRetentionSweep` using the shared two-phase sweep with in-transaction re-check. Evidence rows are unsweepable by grant. No raw worker output is stored — only its digest, its byte count and its refusal code if any.

## Authentication and authorization

Biscuit, deny-by-default, with the three agent facts mandatory in every token and per-agent revocation checked fail-closed. An orchestrator token authorizes run control only; it never carries write capability on contracts, gates, workflows or another product's database.

The authorizer refuses cross-mission operation by `check if`, and refuses any capability outside the token's `capability_scope`. The execution authorization consumed at `OpenRun` is verified against the mission record digest it names — an authorization that does not bind its mission is refused, not trusted.

## Runtime boundaries

**Rust (control core):** plan digest verification, control document parsing, budget and liveness evaluation, capability scope checks, event chain construction and verification, refusal decision. Pure and deterministic — same inputs, same events.

**Rust (run boundary):** worker invocation under a harness profile, attestation collection, persistence adapters. This is the only part holding capabilities, and it holds exactly those opened by ADR-0018 D2: spawn a local process, nothing else.

**Worker (external, replaceable):** performs a step. Receives a bounded, opaque payload and returns a bounded, opaque result. It never receives a repository path, a git capability, a shell handle or an executor secret.

**Closed at this stage:** outbound network, secret material, model providers, real tenant data, deployment. Each is a separate package with its own review.

## Accessibility and degraded mode

The orchestrator has no human interface of its own; Missions carries the human surface of this layer, and its accessibility obligations apply there.

Degraded modes are explicit and fail closed. Revocation store unavailable: deny every new token, let running steps finish under their existing authorization, halt at the next step boundary. Persistence unavailable: refuse to open a run rather than run unrecorded — an unrecorded run cannot be audited and therefore must not exist. Worker unreachable: the step is refused, the run halts with its dossier, and no retry is attempted beyond the control document's ceiling.

## Contracts

- `contracts/schemas/orchestrator-control.v1.schema.json`
- `contracts/schemas/orchestrator-event.v2.schema.json`
- `contracts/schemas/execution-plan-body.v1.schema.json`
- `contracts/schemas/execution-authorization.v1.schema.json`
- `contracts/schemas/agent-contributor-lineage.v1.schema.json`
- `contracts/authz/authority-v1.datalog` (agent facts, per-agent revocation)
- `contracts/fixtures/agent-orchestration-v1/event-chain-vectors.v1.json`
- `contracts/fixtures/agent-orchestration-v1/mission-transition-vectors.v1.json`
- `contracts/fixtures/agent-orchestration-v1/authz-vectors.v2.json`

These are locked by ADR-0004 and are not amended by any implementation package.

## Evidence

Golden vectors of the existing control core stay green: event chain, mission transition, authorization and digest fixtures. Added by the realization packages: a two-tenant integration test proving cross-tenant run access is denied; a replay test proving an event chain reconstructs run state byte-identically; a refusal test for every code of the matrix above; a proof that a step without attestation is refused.

Evidence is published under `distribution/evidence/` per I-20, with the coverage metrics that drive the growth law.

## Work packages

Realization is split so that each package opens exactly one surface and can be reviewed against it:

1. **Run control persistence** — runs, event chain, budget ledger, attestation references, under the shared tenant barrier with its two-tenant deny proof. First security-critical merge of layer 2: bootstrap hard stop (ADR-0011 D4).
2. **Authorization consumption** — execution authorization verification, agent facts enforcement, per-agent revocation fail-closed, refusal matrix.
3. **Bounded local execution** — worker invocation under a harness profile, attestation collection, halt dossier. Depends on the harness packages.

Each package declares its exclusive write paths and carries the mandatory criteria of ADR-0004 §6: store concurrency, RLS, need-to-know exports, retention, deletion and restore.

## Release and rollback

**Release gates.** Every refusal code reachable by a test. Cross-tenant denial proven at the storage layer. Event chain replay byte-identical. No step executed without a bound attestation. Coverage metrics published. Independent review by reviewers distinct from the implementer (K4).

**Rollback.** A run control release rolls back by reverting the deployable and replaying the event chain — no data migration is required, since events are append-only and closed runs are immutable. A rollback never rewrites history: a superseded run stays readable with its original events, marked by a new event rather than edited.
