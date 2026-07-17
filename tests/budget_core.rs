use libre_ai_agent_orchestrator::{EventStoreObservation, PlanBudgetLimits, evaluate_budget_event};
use libre_ai_contract_types::event_chain::{
    AcceptedEventCollision, OrchestratorBudgetCounters, OrchestratorCausalEventFacts,
};

fn counters(tool_calls: u64, changed_bytes: u64) -> OrchestratorBudgetCounters {
    OrchestratorBudgetCounters {
        duration_seconds: 1,
        tool_calls,
        input_tokens: 10,
        output_tokens: 5,
        processes_started: 1,
        files_changed: 1,
        changed_bytes,
    }
}

fn event(
    sequence: u64,
    delta: OrchestratorBudgetCounters,
    total: OrchestratorBudgetCounters,
) -> OrchestratorCausalEventFacts {
    OrchestratorCausalEventFacts {
        id: format!("urn:libre-ai:event:{sequence}"),
        event_digest: if sequence == 1 {
            "a".repeat(64)
        } else {
            "b".repeat(64)
        },
        tenant_id: "ten_alpha000000000".to_owned(),
        mission_id: "urn:libre-ai:mission:alpha".to_owned(),
        run_id: "urn:libre-ai:run:alpha".to_owned(),
        orchestrator_id: "orchestrator-alpha".to_owned(),
        plan_digest: "c".repeat(64),
        authorization_digest: "d".repeat(64),
        sequence,
        previous_event_digest: (sequence > 1).then(|| "a".repeat(64)),
        attempt: 1,
        budget_delta: delta,
        budget_total: total,
    }
}

fn additional(tool_calls: u64, changed_bytes: u64) -> OrchestratorBudgetCounters {
    OrchestratorBudgetCounters {
        duration_seconds: 0,
        tool_calls,
        input_tokens: 0,
        output_tokens: 0,
        processes_started: 0,
        files_changed: 0,
        changed_bytes,
    }
}

fn store<'a>(
    previous: Option<&'a OrchestratorCausalEventFacts>,
    collision: Option<&'a AcceptedEventCollision>,
) -> EventStoreObservation<'a> {
    EventStoreObservation::Available {
        previous,
        collision,
    }
}

fn limits(max_tool_calls: u64) -> PlanBudgetLimits {
    PlanBudgetLimits {
        plan_digest: "c".repeat(64),
        max_duration_seconds: 60,
        max_tool_calls,
        max_input_tokens: 1_000,
        max_output_tokens: 1_000,
        max_processes: 4,
        max_files_changed: 10,
        max_changed_bytes: 1_024,
    }
}

#[test]
fn accepts_exact_monotone_arithmetic_within_the_plan() {
    let first = event(1, counters(1, 10), counters(1, 10));
    let second = event(2, additional(1, 20), counters(2, 30));
    assert_eq!(
        evaluate_budget_event(store(Some(&first), None), &second, &limits(2)).code(),
        "valid"
    );
}

#[test]
fn rejects_plan_substitution_before_budget_evaluation() {
    let first = event(1, counters(1, 10), counters(1, 10));
    let mut substituted = limits(2);
    substituted.plan_digest = "e".repeat(64);
    assert_eq!(
        evaluate_budget_event(store(None, None), &first, &substituted).code(),
        "plan-identity-mismatch"
    );
}

#[test]
fn rejects_plan_limit_overrun_after_valid_chain_arithmetic() {
    let first = event(1, counters(1, 10), counters(1, 10));
    let second = event(2, additional(1, 20), counters(2, 30));
    assert_eq!(
        evaluate_budget_event(store(Some(&first), None), &second, &limits(1)).code(),
        "plan-budget-exceeded"
    );
}

#[test]
fn enforces_every_plan_budget_component() {
    let first = event(1, counters(1, 10), counters(1, 10));
    let mut cases = Vec::new();
    let mut duration = limits(10);
    duration.max_duration_seconds = 0;
    cases.push(duration);
    let mut tools = limits(0);
    tools.max_tool_calls = 0;
    cases.push(tools);
    let mut input = limits(10);
    input.max_input_tokens = 0;
    cases.push(input);
    let mut output = limits(10);
    output.max_output_tokens = 0;
    cases.push(output);
    let mut processes = limits(10);
    processes.max_processes = 0;
    cases.push(processes);
    let mut files = limits(10);
    files.max_files_changed = 0;
    cases.push(files);
    let mut bytes = limits(10);
    bytes.max_changed_bytes = 0;
    cases.push(bytes);

    for plan_limits in cases {
        assert_eq!(
            evaluate_budget_event(store(None, None), &first, &plan_limits).code(),
            "plan-budget-exceeded"
        );
    }
}

#[test]
fn preserves_closed_chain_reasons_before_plan_limit_checks() {
    let first = event(1, counters(1, 10), counters(1, 10));
    let invalid = event(3, additional(1, 20), counters(2, 30));
    assert_eq!(
        evaluate_budget_event(store(Some(&first), None), &invalid, &limits(2)).code(),
        "sequence-invalid"
    );
}

#[test]
fn causal_store_outage_fails_closed() {
    let first = event(1, counters(1, 10), counters(1, 10));
    assert_eq!(
        evaluate_budget_event(EventStoreObservation::Unavailable, &first, &limits(2)).code(),
        "causal-store-unavailable"
    );
}

#[test]
fn exact_duplicate_is_idempotent_and_divergence_is_quarantined() {
    let first = event(1, counters(1, 10), counters(1, 10));
    let exact = AcceptedEventCollision {
        id: first.id.clone(),
        sequence: first.sequence,
        event_digest: first.event_digest.clone(),
    };
    assert_eq!(
        evaluate_budget_event(store(None, Some(&exact)), &first, &limits(2)).code(),
        "idempotent-duplicate"
    );

    let divergent = AcceptedEventCollision {
        event_digest: "f".repeat(64),
        ..exact
    };
    assert_eq!(
        evaluate_budget_event(store(None, Some(&divergent)), &first, &limits(2)).code(),
        "duplicate-divergent"
    );
}
