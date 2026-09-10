//! Mechanizes the `compat-policy` exit criterion recorded in
//! `project.v1.yaml`: consumers pin two things as fact — which symbols
//! `src/lib.rs` re-exports, and the exact stable code strings the crate ever
//! renders. Neither may drift silently.
//!
//! A legitimate change updates the snapshot files in `tests/compat/`, bumps
//! the crate version in `Cargo.toml`, and adds an entry to
//! `docs/compat/BREAKS.md` — all in the same commit.

use libre_ai_agent_orchestrator::{
    AuthorityDecision, AuthorityRefusal, AuthorizedExecutionRefusal, BudgetDecision,
    CausalDecision, CausalRefusal, ControlApplication, ControlDecision, ControlEffect,
    ControlRefusal, EventCollisionObservation, GraphDecision, GraphRefusal,
    GraphTransitionDecision, SimulatedEffectDecision, parse_authorized_graph,
    select_graph_transition,
};
use libre_ai_contract_types::ContractRegistry;
use serde_json::json;

const LIB_RS_SOURCE: &str = include_str!("../src/lib.rs");
const PUBLIC_SURFACE_SNAPSHOT: &str = include_str!("compat/public_surface.snapshot");
const STABLE_CODES_SNAPSHOT: &str = include_str!("compat/stable_codes.snapshot");

/// Every symbol named inside the crate's `pub use module::{...};` blocks —
/// the only place `src/lib.rs` exposes anything, since `budget` and
/// `control` are private modules. Order is not meaningful: callers sort
/// before comparing.
fn exported_symbol_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    for chunk in source.split("pub use ").skip(1) {
        let end = chunk
            .find(';')
            .expect("pub use statement must terminate with ';'");
        let statement = &chunk[..end];
        let items = match statement.find('{') {
            Some(start) => {
                let close = statement
                    .rfind('}')
                    .expect("pub use brace block must close");
                &statement[start + 1..close]
            }
            None => statement.rsplit("::").next().unwrap_or(statement),
        };
        for item in items.split(',') {
            let item = item.trim();
            if !item.is_empty() {
                names.push(item.to_owned());
            }
        }
    }
    names
}

fn snapshot_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

#[test]
fn public_surface_matches_the_committed_snapshot() {
    let mut actual = exported_symbol_names(LIB_RS_SOURCE);
    actual.sort_unstable();

    let mut expected = snapshot_lines(PUBLIC_SURFACE_SNAPSHOT);
    expected.sort_unstable();

    assert_eq!(
        actual, expected,
        "src/lib.rs re-exports changed (symbol added, removed or renamed) without updating \
         tests/compat/public_surface.snapshot — bump the crate version in Cargo.toml and record \
         the break in docs/compat/BREAKS.md before touching this snapshot"
    );
}

// ---- Exhaustive variant coverage -------------------------------------------
//
// Each function below is a compile-time guard, not a runtime check: it does
// nothing at run time, but a variant added to (or removed from) the enum
// makes the match non-exhaustive, or refer to a variant that no longer
// exists — so `cargo test --locked` fails to *build*, before a single test
// body runs. This is what makes a signature-shaped break (a new refusal
// case, a renamed variant) mechanical rather than reviewer-dependent.

#[allow(dead_code)]
fn control_refusal_variants_are_covered(value: ControlRefusal) {
    match value {
        ControlRefusal::SchemaInvalid
        | ControlRefusal::FingerprintInvalid
        | ControlRefusal::TimeInvalid
        | ControlRefusal::Expired
        | ControlRefusal::IdempotencyStoreInvalid
        | ControlRefusal::IdempotencyStoreUnavailable
        | ControlRefusal::IdempotencyConflict
        | ControlRefusal::AuthorizationStoreUnavailable
        | ControlRefusal::AuthorizationRevoked
        | ControlRefusal::RunIdInvalid
        | ControlRefusal::StateMissing
        | ControlRefusal::IdentityMismatch
        | ControlRefusal::StaleRevision
        | ControlRefusal::PreflightMissing
        | ControlRefusal::PreflightFailed
        | ControlRefusal::TransitionForbidden
        | ControlRefusal::RevisionOverflow => {}
    }
}

#[allow(dead_code)]
fn simulated_effect_decision_variants_are_covered(value: SimulatedEffectDecision) {
    match value {
        SimulatedEffectDecision::Allow | SimulatedEffectDecision::Refuse => {}
    }
}

#[allow(dead_code)]
fn budget_decision_variants_are_covered(value: &BudgetDecision) {
    match value {
        BudgetDecision::Chain(_)
        | BudgetDecision::CausalStoreUnavailable
        | BudgetDecision::PlanIdentityMismatch
        | BudgetDecision::PlanBudgetExceeded => {}
    }
}

#[allow(dead_code)]
fn control_decision_variants_are_covered(value: &ControlDecision) {
    match value {
        ControlDecision::Apply(_)
        | ControlDecision::Idempotent { .. }
        | ControlDecision::Refuse(_) => {}
    }
}

#[allow(dead_code)]
fn authorized_execution_refusal_variants_are_covered(value: AuthorizedExecutionRefusal) {
    match value {
        AuthorizedExecutionRefusal::SchemaInvalid
        | AuthorizedExecutionRefusal::StoreUnavailable
        | AuthorizedExecutionRefusal::TransitionForbidden
        | AuthorizedExecutionRefusal::BudgetExceeded
        | AuthorizedExecutionRefusal::ArithmeticOverflow => {}
    }
}

#[allow(dead_code)]
fn graph_refusal_variants_are_covered(value: GraphRefusal) {
    match value {
        GraphRefusal::DuplicateStep
        | GraphRefusal::DuplicateEdge
        | GraphRefusal::EntryMissing
        | GraphRefusal::DanglingEdge
        | GraphRefusal::TerminalHasOutgoingEdge
        | GraphRefusal::RouteMissing
        | GraphRefusal::RouteAmbiguous
        | GraphRefusal::TerminalUnreachable
        | GraphRefusal::UnreachableStep
        | GraphRefusal::CycleForbidden => {}
    }
}

#[allow(dead_code)]
fn graph_decision_variants_are_covered(value: GraphDecision) {
    match value {
        GraphDecision::Valid | GraphDecision::Refused(_) => {}
    }
}

#[allow(dead_code)]
fn graph_transition_decision_variants_are_covered(value: &GraphTransitionDecision) {
    match value {
        GraphTransitionDecision::Selected(_) | GraphTransitionDecision::Refused(_) => {}
    }
}

#[allow(dead_code)]
fn authority_refusal_variants_are_covered(value: AuthorityRefusal) {
    match value {
        AuthorityRefusal::GraphPolicyInvalid | AuthorityRefusal::AuthorityBindingMismatch => {}
    }
}

#[allow(dead_code)]
fn authority_decision_variants_are_covered(value: AuthorityDecision) {
    match value {
        AuthorityDecision::Valid
        | AuthorityDecision::Refused(_)
        | AuthorityDecision::BoundaryRefused(_) => {}
    }
}

#[allow(dead_code)]
fn event_collision_observation_variants_are_covered(value: EventCollisionObservation<'_>) {
    match value {
        EventCollisionObservation::Absent
        | EventCollisionObservation::Existing { .. }
        | EventCollisionObservation::Unavailable => {}
    }
}

#[allow(dead_code)]
fn causal_refusal_variants_are_covered(value: CausalRefusal) {
    match value {
        CausalRefusal::DuplicateDivergent
        | CausalRefusal::IdentityMismatch
        | CausalRefusal::GenerationStale
        | CausalRefusal::SequenceInvalid
        | CausalRefusal::PreviousDigestMismatch
        | CausalRefusal::BudgetDecreased
        | CausalRefusal::BudgetArithmeticInvalid => {}
    }
}

#[allow(dead_code)]
fn causal_decision_variants_are_covered(value: CausalDecision) {
    match value {
        CausalDecision::Valid
        | CausalDecision::Idempotent
        | CausalDecision::Refused(_)
        | CausalDecision::BoundaryRefused(_) => {}
    }
}

fn selected_route_code() -> &'static str {
    let registry = ContractRegistry::embedded().expect("embedded registry");
    let document = json!({
        "schemaVersion": "libre-ai.execution-graph.v1",
        "id": "urn:libre-ai:graph:compat-snapshot",
        "organizationId": "ten_1234567890abcdef",
        "entryStepId": "urn:libre-ai:step:source",
        "steps": [
            {
                "stepId": "urn:libre-ai:step:source",
                "kind": "calculation",
                "outcomeCodes": ["ready"],
                "retryPolicy": { "maximumAttempts": 1, "retryableOutcomeCodes": [] }
            },
            {
                "stepId": "urn:libre-ai:step:terminal",
                "kind": "terminal",
                "outcomeCodes": []
            }
        ],
        "edges": [{
            "edgeId": "urn:libre-ai:edge:ready",
            "fromStepId": "urn:libre-ai:step:source",
            "outcomeCode": "ready",
            "toStepId": "urn:libre-ai:step:terminal"
        }],
        "createdAt": "2030-01-01T00:00:00Z",
        "graphDigest": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
    });
    let graph = parse_authorized_graph(&registry, &document).expect("valid compat graph");
    select_graph_transition(&graph, "urn:libre-ai:step:source", "ready").code()
}

// ---- Stable code strings ---------------------------------------------------
//
// Scope matches project.v1.yaml's compat-policy criterion exactly: the codes
// this crate itself renders. BudgetDecision::Chain(_) delegates to
// OrchestratorEventChainResult::code(), a libre-ai/sdk-rs type — that crate
// owns its own compat policy, so its codes are out of scope here.

#[test]
fn stable_codes_match_the_committed_snapshot() {
    let all_refusals = [
        ControlRefusal::SchemaInvalid,
        ControlRefusal::FingerprintInvalid,
        ControlRefusal::TimeInvalid,
        ControlRefusal::Expired,
        ControlRefusal::IdempotencyStoreInvalid,
        ControlRefusal::IdempotencyStoreUnavailable,
        ControlRefusal::IdempotencyConflict,
        ControlRefusal::AuthorizationStoreUnavailable,
        ControlRefusal::AuthorizationRevoked,
        ControlRefusal::RunIdInvalid,
        ControlRefusal::StateMissing,
        ControlRefusal::IdentityMismatch,
        ControlRefusal::StaleRevision,
        ControlRefusal::PreflightMissing,
        ControlRefusal::PreflightFailed,
        ControlRefusal::TransitionForbidden,
        ControlRefusal::RevisionOverflow,
    ];
    assert_eq!(
        all_refusals.len(),
        17,
        "ControlRefusal variant count drifted from the compat-policy scope recorded in project.v1.yaml"
    );

    let all_simulated = [
        SimulatedEffectDecision::Allow,
        SimulatedEffectDecision::Refuse,
    ];

    let apply = ControlDecision::Apply(ControlApplication {
        effect: ControlEffect::AllocateRun,
        next_revision: 1,
    });
    let idempotent = ControlDecision::Idempotent {
        recorded_next_revision: 1,
    };

    let mut actual: Vec<String> = Vec::new();
    actual.extend(all_refusals.iter().map(|refusal| refusal.code().to_owned()));
    actual.extend(
        all_simulated
            .iter()
            .map(|decision| decision.code().to_owned()),
    );
    actual.push(apply.code().to_owned());
    actual.push(idempotent.code().to_owned());
    actual.push(BudgetDecision::CausalStoreUnavailable.code().to_owned());
    actual.push(BudgetDecision::PlanIdentityMismatch.code().to_owned());
    actual.push(BudgetDecision::PlanBudgetExceeded.code().to_owned());
    actual.extend(
        [
            AuthorizedExecutionRefusal::SchemaInvalid,
            AuthorizedExecutionRefusal::StoreUnavailable,
            AuthorizedExecutionRefusal::TransitionForbidden,
            AuthorizedExecutionRefusal::BudgetExceeded,
            AuthorizedExecutionRefusal::ArithmeticOverflow,
        ]
        .iter()
        .map(|refusal| refusal.code().to_owned()),
    );
    actual.extend(
        [
            GraphRefusal::DuplicateStep,
            GraphRefusal::DuplicateEdge,
            GraphRefusal::EntryMissing,
            GraphRefusal::DanglingEdge,
            GraphRefusal::TerminalHasOutgoingEdge,
            GraphRefusal::RouteMissing,
            GraphRefusal::RouteAmbiguous,
            GraphRefusal::TerminalUnreachable,
            GraphRefusal::UnreachableStep,
            GraphRefusal::CycleForbidden,
        ]
        .iter()
        .map(|refusal| refusal.code().to_owned()),
    );
    actual.push(GraphDecision::Valid.code().to_owned());
    actual.push(selected_route_code().to_owned());
    actual.extend(
        [
            AuthorityRefusal::GraphPolicyInvalid,
            AuthorityRefusal::AuthorityBindingMismatch,
        ]
        .iter()
        .map(|refusal| refusal.code().to_owned()),
    );
    actual.push(AuthorityDecision::Valid.code().to_owned());
    actual.extend(
        [
            CausalRefusal::DuplicateDivergent,
            CausalRefusal::IdentityMismatch,
            CausalRefusal::GenerationStale,
            CausalRefusal::SequenceInvalid,
            CausalRefusal::PreviousDigestMismatch,
            CausalRefusal::BudgetDecreased,
            CausalRefusal::BudgetArithmeticInvalid,
        ]
        .iter()
        .map(|refusal| refusal.code().to_owned()),
    );
    actual.push(CausalDecision::Valid.code().to_owned());
    actual.push(CausalDecision::Idempotent.code().to_owned());
    actual.sort_unstable();

    let mut expected = snapshot_lines(STABLE_CODES_SNAPSHOT);
    expected.sort_unstable();

    assert_eq!(
        actual, expected,
        "a ControlRefusal, SimulatedEffectDecision, ControlDecision or (non-delegated) \
         BudgetDecision code changed, was added or was removed without updating \
         tests/compat/stable_codes.snapshot — bump the crate version in Cargo.toml and record \
         the break in docs/compat/BREAKS.md before touching this snapshot"
    );
}
