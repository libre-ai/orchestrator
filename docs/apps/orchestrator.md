# Orchestrator — Governed agent run control

**Reading this spec: two layers, not one.** `crates/agent-orchestrator` (this repository, crate `libre-ai-agent-orchestrator`) is a pure decision core. Its `0.2.0` surface adds the Phase 4A native authorized-execution semantics approved by ADR-0037 / D43 / WP-G3-O02 to the existing control and budget functions. It owns no run register, worker, Biscuit verification, PostgreSQL transaction or real effect. Sections marked **Target** describe `crates/agent-orchestrator-run`, a separate and still-blocked runtime; they are not claims about this crate.

- **Path:** `crates/agent-orchestrator` (effect-free decision core, `WP-G2-A01` plus `WP-G3-O02`) and `crates/agent-orchestrator-run` (target runtime, not implemented). The split keeps capability-bearing persistence and execution out of the accepted pure core.
- **Owner:** Polaris / Orchestrator (run control for agent fleets)
- **Runtime:** Rust control core; no network, no secret, no provider at this stage (ADR-0018 D2)
- **Tenant model:** organization; every run carries the tenant of the authorization that opened it

## Phase 4A — native authorized-execution core

**Implemented; immutable role review pending.** The crate now provides strict
contract validation followed by private typed normalization, deterministic
graph validation and routing, causal replay, human-decision evaluation,
generation transfer and effect-attestation continuity decisions. Its returned
applications describe what an authorized caller may persist or execute; the
crate itself performs neither action.

**Proven in Phase 4A:**

- a strict validated input boundary using the Contracts registry pinned at
  `5b9b6668909119b670e0db62174419ab04e5b402` through SDK Rust
  `ac9f2020425733183839a58fc2c3928a4de5c066`;
- deterministic graph, causal, decision, transfer and effect decisions with
  stable refusal precedence and checked arithmetic;
- independent replay of all 54 locked semantic vectors across graph, causal,
  decision, effect, authority and transfer domains;
- five fake-journal/fake-executor crash scenarios covering every cut around
  invocation, effect commitment, terminal persistence and result recording;
- no runtime capability in `src/`, enforced mechanically by the capability
  boundary gate.

**Not proven and blocking real execution:**

- transactional PostgreSQL serialization under the organization barrier;
- executor-enforced point-of-effect idempotency and fencing;
- allow-listed, zero-PII runtime logs;
- retention, deletion tombstones and restore replay.

The payoff is a small, independently replayable semantic authority: callers
can test ordering, refusal and recovery decisions without booting a runtime,
and a future execution worker can be replaced without changing Missions or
the canonical wire contracts. The limit matters equally: fake-harness crash
proofs establish the protocol obligations, not the atomicity or confinement
of a real executor.

LangGraph remains an optional, non-normative source of questions and failure
scenarios. It is absent from the dependency graph and is neither an authority
nor an implicit specification; a LangGraph worker may later be connected or
removed without changing Missions or these contracts.

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

1. **Open a run.** The orchestrator receives an `execution-authorization.v2` naming tenant, mission, mission revision, mission record digest, plan and plan digest. It re-computes the plan digest from `execution-plan-body.v2` and refuses on mismatch. No authorization, no run.

2. **Apply a control document.** A control document sets budget ceilings and liveness limits for the run. It is parsed strictly, its command fingerprint recorded, and its limits bound to the run for its whole life; a control document cannot be widened mid-run.

3. **Sequence a step.** The orchestrator selects the next step of the plan, checks the agent's `capability_scope` covers it, requests execution under a named harness profile, and waits for the worker's result plus the harness attestation.

4. **Record an event.** Every decision, refusal, budget movement and step result is appended to the run's event chain as `orchestrator-event.v3`, each entry linked to its predecessor. Nothing is edited; a correction is a new event.

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

**Implemented (covered by tests) — legacy control and budget surface:**

- `parse_control_document(registry, document) -> Result<ControlCommand, ControlRefusal>` — validates a JSON document against the locked `orchestrator-control.v1` schema and deserializes it; never reflects a rejected value back (`ControlRefusal::SchemaInvalid` on any failure).
- `command_fingerprint(command) -> Result<String, ControlRefusal>` — SHA-256 of the command's canonical JSON (JCS) serialization, used for idempotent-replay detection.
- `evaluate_control(state, command, evaluation_time, preflight, collision) -> ControlDecision` — the state machine: `Start` requires `StartPreflight` and allocates a run only when every preflight fact is ready; `Pause`/`Resume`/`Cancel` require an existing `RunControlState` and a matching `expected_revision`. Returns `ControlDecision::Apply`, `::Idempotent`, or `::Refuse(ControlRefusal)`.
- `evaluate_simulated_effect(state: &RunControlState) -> SimulatedEffectDecision` — **not documented anywhere else in this spec, despite being in the crate's public scope (`project.v1.yaml`).** Allows a simulated effect only when the run's phase is `Running` and its caller-declared authorization is both present and active; every other phase (`Blocked`, `Paused`, `Cancelled`, `ResultSubmitted`, `Failed`) or an inactive/unavailable authorization refuses (`tests/control_core.rs::pause_block_and_terminal_states_refuse_every_new_simulated_effect`).
- `evaluate_budget_event(observation, current, limits) -> BudgetDecision` — validates one causal event's arithmetic against `PlanBudgetLimits` and the caller-supplied event chain; refuses closed when the causal store is declared unavailable.

**Implemented in Phase 4A — native authorized-execution surface:**

- `parse_authorized_graph`, `evaluate_graph`, `select_graph_transition` and `evaluate_graph_authority` validate and evaluate the locked graph and authority semantics.
- `parse_authorized_execution_event`, `evaluate_causal_transition` and `replay_authorized_execution` validate event digests and replay accepted state deterministically.
- `evaluate_human_decision` and `evaluate_execution_transfer` decide exact request/response and generation-transfer observations without consuming or persisting them.
- `evaluate_effect_attestation` applies the continuity barrier across invocation, fencing, attestation and terminal observations without calling an executor.

All associated decision, refusal, state, transition, observation and
application types are re-exported by `src/lib.rs` and pinned by the `0.2.0`
compatibility snapshots.

**Target — closed by ADR-0018 D2 until a WP opens it:**

**Commands:** `OpenRun`, `ApplyControlDocument`, `RequestStep`, `RecordStepResult`, `RecordRefusal`, `HaltRun`.

**Queries:** `GetRunState`, `ListRunEvents`, `GetBudgetLedger`, `ExplainRefusal`, `GetAttestationReferences`.

**Events:** `RunOpened`, `ControlApplied`, `StepRequested`, `StepCompleted`, `StepRefused`, `BudgetConsumed`, `LimitReached`, `RunHalted`.

Every event carries the run id, its predecessor digest, the tenant, and a monotonically increasing sequence. Replay of an event chain reconstructs run state exactly; divergence is a defect, not a tolerated drift.

## Refusal matrix

**Status: Mixed.** The 24 legacy codes listed below remain real and stable. Phase 4A adds the graph, authority, causal, decision, transfer, effect and authorized-execution boundary codes; their exhaustive values are mechanically pinned alongside the legacy set in `tests/compat/stable_codes.snapshot` and exercised by the focused and 54-vector suites. The 15 `orchestrator.snake_case` codes further down are the target runtime's and match none of these; no consumer of this crate today can observe them.

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

**Status: Target — closed by ADR-0018 D2 until a WP opens it.** This crate owns no store: its control, budget and authorized-execution evaluators are pure functions over caller-supplied state or observations and return decisions, never persist them. `check-capabilities.ts` fails the build on any filesystem, network or process capability in `src/`, so a PostgreSQL adapter cannot land inside this crate — it belongs to `crates/agent-orchestrator-run`.

PostgreSQL is authoritative, under the shared tenant barrier: `ENABLE` and `FORCE ROW LEVEL SECURITY`, policy on `current_setting('app.tenant_id')`, access only through `withTenantDbTransaction`.

Tables: runs (one row per run, closed state immutable), run_events (append-only, `GRANT SELECT, INSERT` only, primary key `(tenant_id, run_id, sequence)`), budget_ledger (append-only movements, never a mutable balance), attestation_refs (digest and profile id of each harness attestation, never the attested content).

Retention follows the executable policy: a closed run's events expire under `runRetentionSweep` using the shared two-phase sweep with in-transaction re-check. Evidence rows are unsweepable by grant. No raw worker output is stored — only its digest, its byte count and its refusal code if any.

## Authentication and authorization

**Status: Target — closed by ADR-0018 D2 until a WP opens it.** No Biscuit token is parsed or verified in this crate. `StartPreflight` and `RunControlState` carry caller-declared booleans (`biscuit_allowed`, `authorization_active`, `authorization_store_available`, `quorum_valid`, `harness_attestation_valid`, `required_controls_effective`, `key_registry_available`) that `evaluate_control` treats as facts, refusing closed the moment any is false or absent (`ControlRefusal::PreflightFailed`, `::AuthorizationRevoked`, `::AuthorizationStoreUnavailable`) — but verifying those facts in the first place is the future runtime's job, not this crate's.

Biscuit, deny-by-default, with the three agent facts mandatory in every token and per-agent revocation checked fail-closed. An orchestrator token authorizes run control only; it never carries write capability on contracts, gates, workflows or another product's database.

The authorizer refuses cross-mission operation by `check if`, and refuses any capability outside the token's `capability_scope`. The execution authorization consumed at `OpenRun` is verified against the mission record digest it names — an authorization that does not bind its mission is refused, not trusted.

## Runtime boundaries

**Status: Mixed.**

**Rust (decision core) — Implemented (covered by tests):** control document parsing, control/budget evaluation and the Phase 4A authorized-execution surface above are real and pure. Graph and event documents are validated against the pinned registry before normalization; decisions consume caller-supplied observations and return values without constructing a store or performing an effect. Capability scope verification, persistence atomicity and executor confinement remain runtime responsibilities.

**Rust (run boundary) — Target — closed by ADR-0018 D2 until a WP opens it:** worker invocation under a harness profile, attestation collection, persistence adapters. This is the only part holding capabilities, and it holds exactly those opened by ADR-0018 D2: spawn a local process, nothing else.

**Worker (external, replaceable) — Target — closed by ADR-0018 D2 until a WP opens it:** performs a step. Receives a bounded, opaque payload and returns a bounded, opaque result. It never receives a repository path, a git capability, a shell handle or an executor secret.

**Closed at this stage:** outbound network, secret material, model providers, real tenant data, deployment. Each is a separate package with its own review.

## Accessibility and degraded mode

**Status: Mixed.** No human interface exists in either layer, so the accessibility deferral holds unconditionally. The fail-closed _principle_ below is real and tested at the decision-core level today; the specific degraded modes described (revocation store, persistence, worker) are the target runtime's.

**Implemented (covered by tests) — fail closed on a caller-declared unavailable store:** `CommandCollisionObservation::StoreUnavailable` refuses `orchestrator.control.idempotency-store-unavailable`; `RunControlState.authorization_store_available: false` refuses `orchestrator.control.authorization-store-unavailable`; `EventStoreObservation::Unavailable` refuses `causal-store-unavailable`. In every case the crate refuses rather than assuming availability — it never observes a store itself, only the caller's declaration of one.

**Target — closed by ADR-0018 D2 until a WP opens it:** The orchestrator has no human interface of its own; Missions carries the human surface of this layer, and its accessibility obligations apply there.

Degraded modes are explicit and fail closed. Revocation store unavailable: deny every new token, let running steps finish under their existing authorization, halt at the next step boundary. Persistence unavailable: refuse to open a run rather than run unrecorded — an unrecorded run cannot be audited and therefore must not exist. Worker unreachable: the step is refused, the run halts with its dossier, and no retry is attempted beyond the control document's ceiling.

## Contracts

**Status: Mixed.** The legacy control schema and event-chain vectors remain load-bearing. Phase 4A additionally resolves the locked authorized-execution schemas through the embedded SDK registry and replays the semantic corpus directly from the pinned Contracts checkout. Runtime-only contracts remain targets, not implemented inputs.

**Implemented (covered by tests):**

- `contracts/schemas/orchestrator-control.v1.schema.json` — `src/control.rs`'s `CONTROL_SCHEMA`, validated in `parse_control_document`.
- `contracts/fixtures/agent-orchestration-v1/event-chain-vectors.v1.json` — replayed by `tests/locked_event_vectors.rs` through `evaluate_budget_event`.
- `contracts/schemas/execution-graph.v1.schema.json` and `execution-plan-body.v2.schema.json` — graph validation, routing and graph/plan authority binding.
- `contracts/schemas/orchestrator-event.v3.schema.json` — event validation, JCS digest verification and deterministic replay.
- `contracts/schemas/human-decision-request.v1.schema.json` and `human-decision-response.v1.schema.json` — human-decision evaluation.
- `contracts/schemas/execution-transfer.v1.schema.json` — generation-transfer evaluation.
- `contracts/schemas/effect-attestation.v1.schema.json` — effect continuity and fencing evaluation.
- `contracts/fixtures/authorized-execution-v1/semantic-vectors.v1.json` — all 54 locked cases replayed directly by `tests/authorized_execution_vectors.rs`.

**Target — closed by ADR-0018 D2 until a WP opens it:**

- `contracts/schemas/execution-authorization.v2.schema.json`
- `contracts/schemas/step-invocation.v1.schema.json`
- `contracts/schemas/agent-contributor-lineage.v1.schema.json`
- `contracts/authz/authority-v1.datalog` (agent facts, per-agent revocation)
- `contracts/fixtures/agent-orchestration-v1/mission-transition-vectors.v1.json`
- `contracts/fixtures/agent-orchestration-v1/authz-vectors.v2.json`

These are locked by ADR-0004 and are not amended by any implementation package.

## Evidence

**Status: Mixed.**

**Implemented (covered by tests) today:** the legacy control, budget and locked event-chain suites remain green. Phase 4A adds focused graph, replay, decision, transfer and effect suites; one independent test replays the exact 54-case locked semantic corpus directly from Contracts; and one end-to-end fake-journal/fake-executor suite proves five crash cut points plus identity, fencing, profile and collision substitutions. The capability gate continues to prove that production `src/` cannot gain process, filesystem, network, environment, thread or clock access.

**Target — closed by ADR-0018 D2 until a WP opens it:** mission-transition and authorization fixtures are not consumed by this crate. Runtime realization must add a two-tenant integration test proving cross-tenant run access is denied, transactional replay against the real event store, and proof that no worker invocation occurs without a bound authorization and harness profile.

Evidence is published under `distribution/evidence/` per I-20, with the coverage metrics that drive the growth law.

## Work packages

**Status: Target — closed until separately authorized.** Phase 4A is authorized by WP-G3-O02 and does not complete or claim WP-G3-O01. The three packages below are capability-bearing runtime work and remain unstarted.

Realization is split so that each package opens exactly one surface and can be reviewed against it:

1. **Run control persistence** — runs, event chain, budget ledger, attestation references, under the shared tenant barrier with its two-tenant deny proof. First security-critical merge of layer 2: bootstrap hard stop (ADR-0011 D4).
2. **Authorization consumption** — execution authorization verification, agent facts enforcement, per-agent revocation fail-closed, refusal matrix.
3. **Bounded local execution** — worker invocation under a harness profile, attestation collection, halt dossier. Depends on the harness packages.

Each package declares its exclusive write paths and carries the mandatory criteria of ADR-0004 §6: store concurrency, RLS, need-to-know exports, retention, deletion and restore.

## Release and rollback

**Status: Mixed.** The gates below govern a future runtime, not Phase 4A. The decision core's release discipline includes formatting, all-feature tests, clippy with warnings denied, policy gates, dependency licensing, the 54-vector proof, the five-cut crash proof and immutable role review.

**Release gates.** Every refusal code reachable by a test. Cross-tenant denial proven at the storage layer. Event chain replay byte-identical. No step executed without a bound attestation. Coverage metrics published. Independent review by reviewers distinct from the implementer (K4).

**Phase 4A rollback.** Before any future consumer pins this surface, revert the additive Phase 4A commits. After a consumer exists, pin that consumer to the prior Orchestrator revision first, then revert Phase 4A here. No canonical event migration exists because this pure core adds neither a contract nor a stored-state schema.

**Future runtime rollback.** A run-control release must roll back the deployable and replay the canonical event chain without rewriting accepted history. That mechanism is not implemented or proven by Phase 4A.
