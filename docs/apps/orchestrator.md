# Orchestrator — Governed agent run control

**Reading this spec: two layers, not one.** `crates/agent-orchestrator` (this repository, crate `libre-ai-agent-orchestrator`) is a pure decision core: parse a control command, evaluate one action against caller-supplied state, evaluate one budget event against caller-supplied causal facts — no run register, no worker, no Biscuit verification, no PostgreSQL. Every section below carries an explicit status: **Implemented (covered by tests)** where it describes that core (real kebab-case refusal codes cited from `src/control.rs` and `src/budget.rs`), or **Target — closed by ADR-0018 D2 until a WP opens it** where it describes `crates/agent-orchestrator-run`, the not-yet-started runtime this specification also covers (the 15 snake_case codes below are that runtime's, and match no code this crate renders).

- **Path:** `crates/agent-orchestrator` (control core, already locked by `WP-G2-A01`, simulation-only) and `crates/agent-orchestrator-run` (runtime, written by this specification's package). The split is deliberate: ADR-0004 §8 bounds the core to simulation against a fake harness, so capabilities land in a separate crate with its own review rather than widening an accepted one.
- **Owner:** Polaris / Orchestrator (run control for agent fleets)
- **Runtime:** Rust control core; no network, no secret, no provider at this stage (ADR-0018 D2)
- **Tenant model:** organization; every run carries the tenant of the authorization that opened it

## Purpose and actors

**Status: Target — closed by ADR-0018 D2 until a WP opens it.** The actor model below (Missions, Workers, Operators, Owner) is the full run-control runtime's. The crate that exists today has no notion of any of them: callers pass it opaque, already-authenticated facts (`RunControlState`, `StartPreflight`, causal event facts) and get back a decision. K1 (deny-by-default, per-agent revocation) and K3 (signed envelope) are not implemented here — the crate consumes caller-declared booleans (`authorization_active`, `authorization_store_available`, `biscuit_allowed`, `harness_attestation_valid`, …) rather than verifying a Biscuit token or an envelope itself; a caller that lies about those booleans is not something this crate can detect.

The orchestrator turns an authorized execution plan into a bounded, observable run, and refuses everything it was not authorized to do. It decides sequencing, applies budgets and control documents, records an append-only event chain, and halts. It never grants itself the right to act.

**Actors:**

- **Missions** (authority): owns workflow, quorum, execution authorization and the validation projection. It issues `execution-authorization.v1`; the orchestrator consumes it and cannot mint it (ADR-0004 §2).
- **Workers** (executors): replaceable RPC processes that perform steps under a harness profile. Their permissions, sessions and internal types are never a Libre AI security boundary (ADR-0004 §3).
- **Operators** (auditors): read run events, budgets consumed, refusals and attestation references; cannot mutate a closed run.
- **Owner** (nominative acts): pronounces the bootstrap hard stop of each new security-critical pattern in this layer (ADR-0011 D4).

**Doctrine constraints.** Deny-by-default authorization with mandatory agent facts — `agent_fleet`, `mission_agent`, `capability_scope` — and per-agent revocation that fails closed (K1). Tool output and worker stdout enter as `operational` and never justify a write to a source of truth (K2). Any untrusted payload reaching a model is wrapped in a signed envelope (K3). The run register is append-only (K5).

## Journeys

**Status: Target — closed by ADR-0018 D2 until a WP opens it.** None of the six journeys below exist end-to-end: there is no run register, no worker invocation, no harness, no event persistence in this repository (`project.v1.yaml`'s "hors périmètre" scope). What exists today is narrower and pure: `evaluate_control` decides one `ControlAction` (`start`/`pause`/`resume`/`cancel`) against a caller-supplied `RunControlState` and `StartPreflight`; `evaluate_simulated_effect` decides whether a simulated effect is allowed for the current phase and authorization; `evaluate_budget_event` decides one causal event against caller-supplied budget limits. Journey 2 ("Apply a control document") is the closest existing match — schema validation and idempotent replay are real (`parse_control_document`, `command_fingerprint`) — but budget ceilings are evaluated by a separate function (`evaluate_budget_event`), not bound to a control document the way this journey implies.

1. **Open a run.** The orchestrator receives an `execution-authorization.v1` naming tenant, mission, mission revision, mission record digest, plan and plan digest. It re-computes the plan digest from `execution-plan-body.v1` and refuses on mismatch. No authorization, no run.

2. **Apply a control document.** A control document sets budget ceilings and liveness limits for the run. It is parsed strictly, its command fingerprint recorded, and its limits bound to the run for its whole life; a control document cannot be widened mid-run.

3. **Sequence a step.** The orchestrator selects the next step of the plan, checks the agent's `capability_scope` covers it, requests execution under a named harness profile, and waits for the worker's result plus the harness attestation.

4. **Record an event.** Every decision, refusal, budget movement and step result is appended to the run's event chain as `orchestrator-event.v2`, each entry linked to its predecessor. Nothing is edited; a correction is a new event.

5. **Halt.** A run halts on plan completion, on a budget ceiling reached, on a liveness limit reached, or on a refusal. A halt produces a decision dossier — what was done, what remains, the cause — and never a silent kill (ADR-0011 D6).

6. **Inspect a closed run.** An operator retrieves the full event chain, the control document applied, the budgets consumed and every harness attestation referenced, and can re-verify each digest independently.

## Non-goals

**Status: Implemented (covered by tests).** `verification/agent-orchestrator/check-capabilities.ts` (13 tests, `bun run check:capabilities`) mechanically enforces the zero-effect boundary this list describes: it fails the build on `std::process`/`std::fs`/`std::net`/`std::env`/`std::thread`/`std::time`, on any dependency outside an explicit allow-list, and on any `unsafe` line not covered by `#![forbid(unsafe_code)]` (`src/lib.rs:1`). The non-goals below hold for the decision core today by construction, not by discipline alone, and are restated here as the invariant every future work package inherits.

- **Self-authorization.** The orchestrator never issues, widens or infers an execution authorization. Missions is the only authority.
- **Being a security boundary for the worker.** Pi's internal permission model is not trusted; confinement is the harness's job, not the worker's promise.
- **Executing without a harness.** A step with no harness profile and no attestation is refused, not run unconfined.
- **Network, secrets, providers, real tenant data.** Closed at this stage by ADR-0018 D2; each requires its own package and review.
- **Writing to CI or gates.** No agent token grants gate or workflow write by default (K1).
- **Editing history.** The event chain is append-only; a closed run is immutable.

## Domain protocol

**Status: Mixed.** The functions below are the crate's real, tested public API (`src/lib.rs`'s `pub use`); the Commands/Queries/Events below them describe the target runtime's protocol and match none of these signatures — there is no run register to open, no event to list, nothing to halt.

**Implemented (covered by tests) — the decision core's actual protocol:**

- `parse_control_document(registry, document) -> Result<ControlCommand, ControlRefusal>` — validates a JSON document against the locked `orchestrator-control.v1` schema and deserializes it; never reflects a rejected value back (`ControlRefusal::SchemaInvalid` on any failure).
- `command_fingerprint(command) -> Result<String, ControlRefusal>` — SHA-256 of the command's canonical JSON (JCS) serialization, used for idempotent-replay detection.
- `evaluate_control(state, command, evaluation_time, preflight, collision) -> ControlDecision` — the state machine: `Start` requires `StartPreflight` and allocates a run only when every preflight fact is ready; `Pause`/`Resume`/`Cancel` require an existing `RunControlState` and a matching `expected_revision`. Returns `ControlDecision::Apply`, `::Idempotent`, or `::Refuse(ControlRefusal)`.
- `evaluate_simulated_effect(state: &RunControlState) -> SimulatedEffectDecision` — **not documented anywhere else in this spec, despite being in the crate's public scope (`project.v1.yaml`).** Allows a simulated effect only when the run's phase is `Running` and its caller-declared authorization is both present and active; every other phase (`Blocked`, `Paused`, `Cancelled`, `ResultSubmitted`, `Failed`) or an inactive/unavailable authorization refuses (`tests/control_core.rs::pause_block_and_terminal_states_refuse_every_new_simulated_effect`).
- `evaluate_budget_event(observation, current, limits) -> BudgetDecision` — validates one causal event's arithmetic against `PlanBudgetLimits` and the caller-supplied event chain; refuses closed when the causal store is declared unavailable.

**Target — closed by ADR-0018 D2 until a WP opens it:**

**Commands:** `OpenRun`, `ApplyControlDocument`, `RequestStep`, `RecordStepResult`, `RecordRefusal`, `HaltRun`.

**Queries:** `GetRunState`, `ListRunEvents`, `GetBudgetLedger`, `ExplainRefusal`, `GetAttestationReferences`.

**Events:** `RunOpened`, `ControlApplied`, `StepRequested`, `StepCompleted`, `StepRefused`, `BudgetConsumed`, `LimitReached`, `RunHalted`.

Every event carries the run id, its predecessor digest, the tenant, and a monotonically increasing sequence. Replay of an event chain reconstructs run state exactly; divergence is a defect, not a tolerated drift.

## Refusal matrix

**Status: Mixed.** The 24 codes below are real — rendered by this crate today, reached by `tests/control_core.rs` and `tests/budget_core.rs`, and pinned exhaustively by the compat-policy snapshot (`project.v1.yaml`'s `compat-policy` exit criterion) — and follow a `orchestrator.control.kebab-case` / `orchestrator.effect.kebab-case` / bare-`kebab-case` shape. The 15 `orchestrator.snake_case` codes further down are the target runtime's and match none of these; no consumer of this crate today can observe them.

**Implemented (covered by tests) — `ControlRefusal::code` (17):**

- `orchestrator.control.schema-invalid`, `orchestrator.control.fingerprint-invalid`, `orchestrator.control.time-invalid`, `orchestrator.control.expired`, `orchestrator.control.idempotency-store-invalid`, `orchestrator.control.idempotency-store-unavailable`, `orchestrator.control.idempotency-conflict`, `orchestrator.control.authorization-store-unavailable`, `orchestrator.control.authorization-revoked`, `orchestrator.control.run-id-invalid`, `orchestrator.control.state-missing`, `orchestrator.control.identity-mismatch`, `orchestrator.control.stale-revision`, `orchestrator.control.preflight-missing`, `orchestrator.control.preflight-failed`, `orchestrator.control.transition-forbidden`, `orchestrator.control.revision-overflow`.

**Implemented (covered by tests) — `ControlDecision::code`'s own codes (2; `::Refuse` delegates to `ControlRefusal::code` above, not counted twice):**

- `orchestrator.control.apply`, `orchestrator.control.idempotent`.

**Implemented (covered by tests) — `SimulatedEffectDecision::code` (2):**

- `orchestrator.effect.allow`, `orchestrator.effect.refuse`.

**Implemented (covered by tests) — `BudgetDecision::code`'s own codes (3; `::Chain(_)` delegates to `OrchestratorEventChainResult::code()` from `libre-ai/sdk-rs`, out of this crate's compat scope):**

- `causal-store-unavailable`, `plan-identity-mismatch`, `plan-budget-exceeded`.

**Target — closed by ADR-0018 D2 until a WP opens it:**

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

**Status: Target — closed by ADR-0018 D2 until a WP opens it.** This crate owns no store: `evaluate_control` and `evaluate_budget_event` are pure functions over caller-supplied state (`RunControlState`, `EventStoreObservation`) and return a decision, never persist one. `check-capabilities.ts` fails the build on any filesystem, network or process capability in `src/`, so a PostgreSQL adapter cannot land inside this crate — it belongs to `crates/agent-orchestrator-run`.

PostgreSQL is authoritative, under the shared tenant barrier: `ENABLE` and `FORCE ROW LEVEL SECURITY`, policy on `current_setting('app.tenant_id')`, access only through `withTenantDbTransaction`.

Tables: runs (one row per run, closed state immutable), run_events (append-only, `GRANT SELECT, INSERT` only, primary key `(tenant_id, run_id, sequence)`), budget_ledger (append-only movements, never a mutable balance), attestation_refs (digest and profile id of each harness attestation, never the attested content).

Retention follows the executable policy: a closed run's events expire under `runRetentionSweep` using the shared two-phase sweep with in-transaction re-check. Evidence rows are unsweepable by grant. No raw worker output is stored — only its digest, its byte count and its refusal code if any.

## Authentication and authorization

**Status: Target — closed by ADR-0018 D2 until a WP opens it.** No Biscuit token is parsed or verified in this crate. `StartPreflight` and `RunControlState` carry caller-declared booleans (`biscuit_allowed`, `authorization_active`, `authorization_store_available`, `quorum_valid`, `harness_attestation_valid`, `required_controls_effective`, `key_registry_available`) that `evaluate_control` treats as facts, refusing closed the moment any is false or absent (`ControlRefusal::PreflightFailed`, `::AuthorizationRevoked`, `::AuthorizationStoreUnavailable`) — but verifying those facts in the first place is the future runtime's job, not this crate's.

Biscuit, deny-by-default, with the three agent facts mandatory in every token and per-agent revocation checked fail-closed. An orchestrator token authorizes run control only; it never carries write capability on contracts, gates, workflows or another product's database.

The authorizer refuses cross-mission operation by `check if`, and refuses any capability outside the token's `capability_scope`. The execution authorization consumed at `OpenRun` is verified against the mission record digest it names — an authorization that does not bind its mission is refused, not trusted.

## Runtime boundaries

**Status: Mixed.**

**Rust (control core) — Implemented (covered by tests), with two overstated specifics:** control document parsing (`parse_control_document`), budget evaluation (`evaluate_budget_event`) and the refusal decision itself are real and pure — same inputs, same `ControlDecision`/`BudgetDecision`, verified by `#[forbid(unsafe_code)]` plus `check-capabilities.ts`. "Plan digest verification" and "capability scope checks" are not: neither `execution-plan-body.v1` nor `capability_scope` appears anywhere in `src/`. "Event chain construction" is also imprecise — `evaluate_budget_event` _validates_ a caller-supplied event against caller-supplied prior state via `evaluate_orchestrator_event_chain` (an `sdk-rs` function); it constructs nothing.

**Rust (run boundary) — Target — closed by ADR-0018 D2 until a WP opens it:** worker invocation under a harness profile, attestation collection, persistence adapters. This is the only part holding capabilities, and it holds exactly those opened by ADR-0018 D2: spawn a local process, nothing else.

**Worker (external, replaceable) — Target — closed by ADR-0018 D2 until a WP opens it:** performs a step. Receives a bounded, opaque payload and returns a bounded, opaque result. It never receives a repository path, a git capability, a shell handle or an executor secret.

**Closed at this stage:** outbound network, secret material, model providers, real tenant data, deployment. Each is a separate package with its own review.

## Accessibility and degraded mode

**Status: Mixed.** No human interface exists in either layer, so the accessibility deferral holds unconditionally. The fail-closed _principle_ below is real and tested at the decision-core level today; the specific degraded modes described (revocation store, persistence, worker) are the target runtime's.

**Implemented (covered by tests) — fail closed on a caller-declared unavailable store:** `CommandCollisionObservation::StoreUnavailable` refuses `orchestrator.control.idempotency-store-unavailable`; `RunControlState.authorization_store_available: false` refuses `orchestrator.control.authorization-store-unavailable`; `EventStoreObservation::Unavailable` refuses `causal-store-unavailable`. In every case the crate refuses rather than assuming availability — it never observes a store itself, only the caller's declaration of one.

**Target — closed by ADR-0018 D2 until a WP opens it:** The orchestrator has no human interface of its own; Missions carries the human surface of this layer, and its accessibility obligations apply there.

Degraded modes are explicit and fail closed. Revocation store unavailable: deny every new token, let running steps finish under their existing authorization, halt at the next step boundary. Persistence unavailable: refuse to open a run rather than run unrecorded — an unrecorded run cannot be audited and therefore must not exist. Worker unreachable: the step is refused, the run halts with its dossier, and no retry is attempted beyond the control document's ceiling.

## Contracts

**Status: Mixed.** Two of the nine entries below are read by this crate's source today; the other seven are the target runtime's, referenced by no code in `src/` or `tests/`. `event-chain-vectors.v1.json` — real and load-bearing — was missing from this list; added below rather than silently left out.

**Implemented (covered by tests):**

- `contracts/schemas/orchestrator-control.v1.schema.json` — `src/control.rs`'s `CONTROL_SCHEMA`, validated in `parse_control_document`.
- `contracts/fixtures/agent-orchestration-v1/event-chain-vectors.v1.json` — replayed by `tests/locked_event_vectors.rs` through `evaluate_budget_event`.

**Target — closed by ADR-0018 D2 until a WP opens it:**

- `contracts/schemas/orchestrator-event.v2.schema.json`
- `contracts/schemas/execution-plan-body.v1.schema.json`
- `contracts/schemas/execution-authorization.v1.schema.json`
- `contracts/schemas/agent-contributor-lineage.v1.schema.json`
- `contracts/authz/authority-v1.datalog` (agent facts, per-agent revocation)
- `contracts/fixtures/agent-orchestration-v1/mission-transition-vectors.v1.json`
- `contracts/fixtures/agent-orchestration-v1/authz-vectors.v2.json`

These are locked by ADR-0004 and are not amended by any implementation package.

## Evidence

**Status: Mixed.**

**Implemented (covered by tests) today:** 9 tests in `tests/control_core.rs` (schema validation, preflight-gated `Start`, `Pause`/`Resume`/`Cancel` transitions, idempotent replay vs. divergence, revision overflow, simulated-effect refusal by phase); 7 tests in `tests/budget_core.rs` (plan-limit arithmetic, causal-store outage, exact-duplicate idempotency, plan-identity substitution); 1 test in `tests/locked_event_vectors.rs` replaying the locked event-chain vectors; 13 tests in `verification/agent-orchestrator/capability-boundary.test.ts` proving the zero-effect boundary. 21 of the 24 real codes in the Refusal matrix above have their exact string asserted by at least one of these tests; every `ControlDecision` and `SimulatedEffectDecision` variant is reached at the decision-logic level even where its code string isn't separately asserted. **Gap, not a claim to paper over:** `orchestrator.control.fingerprint-invalid`, `orchestrator.control.state-missing` and `orchestrator.control.preflight-missing` are reached by no test in this suite today — `command_fingerprint` is only ever exercised on its success path, no test omits `preflight` on a `Start` command, and no test omits `state` on a non-`Start` command.

**Target — closed by ADR-0018 D2 until a WP opens it:** "mission transition" and "authorization" fixtures (no such vectors are consumed by this crate — see Contracts above). Golden vectors of the existing control core stay green: event chain, mission transition, authorization and digest fixtures. Added by the realization packages: a two-tenant integration test proving cross-tenant run access is denied; a replay test proving an event chain reconstructs run state byte-identically; a refusal test for every code of the target matrix above; a proof that a step without attestation is refused.

Evidence is published under `distribution/evidence/` per I-20, with the coverage metrics that drive the growth law.

## Work packages

**Status: Target — closed by ADR-0018 D2 until a WP opens it.** The three work packages below are the not-yet-started runtime's. The decision core they would sit on top of corresponds to the already-closed `WP-G2-A01` (see the Path line at the top of this spec) — it is not one of the three below, and needs no further work package to be "done"; its own release gate is the compat-policy mechanism (`project.v1.yaml`, `tests/compat_surface.rs`), not this section.

Realization is split so that each package opens exactly one surface and can be reviewed against it:

1. **Run control persistence** — runs, event chain, budget ledger, attestation references, under the shared tenant barrier with its two-tenant deny proof. First security-critical merge of layer 2: bootstrap hard stop (ADR-0011 D4).
2. **Authorization consumption** — execution authorization verification, agent facts enforcement, per-agent revocation fail-closed, refusal matrix.
3. **Bounded local execution** — worker invocation under a harness profile, attestation collection, halt dossier. Depends on the harness packages.

Each package declares its exclusive write paths and carries the mandatory criteria of ADR-0004 §6: store concurrency, RLS, need-to-know exports, retention, deletion and restore.

## Release and rollback

**Status: Target — closed by ADR-0018 D2 until a WP opens it.** The gates below govern the runtime's release, not this crate's. The decision core's own release discipline is `cargo test --locked` plus the compat-policy snapshot (`project.v1.yaml`); it has no cross-tenant storage, no attestation and no rollback of its own to describe, since it persists nothing.

**Release gates.** Every refusal code reachable by a test. Cross-tenant denial proven at the storage layer. Event chain replay byte-identical. No step executed without a bound attestation. Coverage metrics published. Independent review by reviewers distinct from the implementer (K4).

**Rollback.** A run control release rolls back by reverting the deployable and replaying the event chain — no data migration is required, since events are append-only and closed runs are immutable. A rollback never rewrites history: a superseded run stays readable with its original events, marked by a new event rather than edited.
