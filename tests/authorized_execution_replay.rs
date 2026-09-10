mod support;

use libre_ai_agent_orchestrator::{
    AuthorizedExecutionEvent, EventCollisionObservation, evaluate_causal_transition,
    parse_authorized_execution_event, parse_authorized_graph, replay_authorized_execution,
};
use libre_ai_contract_types::ContractRegistry;
use serde_json::Value;
use support::authorized_execution::{
    EventFixture, encode_state_for_test, event_document, reseal_event_document,
    valid_graph_document,
};

const GRAPH_DIGEST: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const CALCULATION_STEP: &str = "urn:libre-ai:step:calculate-1";
const DECISION_STEP: &str = "urn:libre-ai:step:decision-1";
const EFFECT_STEP: &str = "urn:libre-ai:step:effect-1";
const TERMINAL_STEP: &str = "urn:libre-ai:step:terminal-1";
const ATTEMPT: &str = "urn:libre-ai:attempt:synthetic-attempt";
const WORKER: &str = "urn:libre-ai:worker-invocation:synthetic-worker";

fn parsed_event(
    registry: &ContractRegistry,
    sequence: u64,
    previous_event_digest: Option<&str>,
    tool_calls_delta: u64,
    tool_calls_total: u64,
) -> AuthorizedExecutionEvent {
    let document = event_document(&EventFixture {
        event_type: "effect-terminal",
        sequence,
        previous_event_digest,
        graph_digest: GRAPH_DIGEST,
        step_id: Some(EFFECT_STEP),
        attempt_id: Some(ATTEMPT),
        worker_invocation_id: Some(WORKER),
        selected_edge_id: Some("urn:libre-ai:edge:committed-1"),
        outcome_code: None,
        effect_status: Some("committed"),
        tool_calls_delta,
        tool_calls_total,
    });
    parse_authorized_execution_event(registry, &document).expect("valid event")
}

fn mutated_event(
    registry: &ContractRegistry,
    base: &AuthorizedExecutionEvent,
    mutate: impl FnOnce(&mut Value),
) -> AuthorizedExecutionEvent {
    let mut document = event_document(&EventFixture {
        event_type: "effect-terminal",
        sequence: base.sequence() + 1,
        previous_event_digest: Some(base.digest()),
        graph_digest: GRAPH_DIGEST,
        step_id: Some(EFFECT_STEP),
        attempt_id: Some(ATTEMPT),
        worker_invocation_id: Some(WORKER),
        selected_edge_id: Some("urn:libre-ai:edge:committed-1"),
        outcome_code: None,
        effect_status: Some("committed"),
        tool_calls_delta: 0,
        tool_calls_total: 0,
    });
    mutate(&mut document);
    reseal_event_document(&mut document);
    parse_authorized_execution_event(registry, &document)
        .expect("mutated event remains schema-valid")
}

#[test]
fn exact_collision_is_idempotent_but_divergent_collision_quarantines() {
    let registry = ContractRegistry::embedded().expect("embedded registry");
    let event = parsed_event(&registry, 1, None, 0, 0);
    assert_eq!(
        evaluate_causal_transition(
            None,
            &event,
            EventCollisionObservation::Existing {
                event_id: event.id(),
                sequence: event.sequence(),
                event_digest: event.digest(),
            },
        )
        .code(),
        "idempotent-duplicate",
    );
    assert_eq!(
        evaluate_causal_transition(
            None,
            &event,
            EventCollisionObservation::Existing {
                event_id: event.id(),
                sequence: event.sequence(),
                event_digest: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            },
        )
        .code(),
        "duplicate-divergent",
    );
}

#[test]
fn event_digest_is_verified_at_the_validated_boundary() {
    let registry = ContractRegistry::embedded().expect("embedded registry");
    let mut document = event_document(&EventFixture {
        event_type: "effect-terminal",
        sequence: 1,
        previous_event_digest: None,
        graph_digest: GRAPH_DIGEST,
        step_id: Some(EFFECT_STEP),
        attempt_id: Some(ATTEMPT),
        worker_invocation_id: Some(WORKER),
        selected_edge_id: Some("urn:libre-ai:edge:committed-1"),
        outcome_code: None,
        effect_status: Some("committed"),
        tool_calls_delta: 0,
        tool_calls_total: 0,
    });
    document["occurredAt"] = Value::String("2026-09-10T10:00:02Z".to_owned());

    let refusal = parse_authorized_execution_event(&registry, &document)
        .expect_err("a stale digest must fail closed");
    assert_eq!(
        refusal.code(),
        "orchestrator.authorized-execution.schema-invalid"
    );
}

#[test]
fn causal_store_unavailability_fails_closed() {
    let registry = ContractRegistry::embedded().expect("embedded registry");
    let event = parsed_event(&registry, 1, None, 0, 0);
    assert_eq!(
        evaluate_causal_transition(None, &event, EventCollisionObservation::Unavailable).code(),
        "orchestrator.authorized-execution.store-unavailable"
    );
}

#[test]
fn causal_precedence_covers_every_locked_outcome() {
    let registry = ContractRegistry::embedded().expect("embedded registry");
    let previous = parsed_event(&registry, 1, None, 0, 0);

    let identity = mutated_event(&registry, &previous, |document| {
        document["organizationId"] = Value::String("ten_abcdef1234567890".to_owned());
    });
    assert_eq!(
        evaluate_causal_transition(
            Some(&previous),
            &identity,
            EventCollisionObservation::Absent
        )
        .code(),
        "identity-mismatch"
    );

    let generation = mutated_event(&registry, &previous, |document| {
        document["generation"] = Value::from(2);
    });
    assert_eq!(
        evaluate_causal_transition(
            Some(&previous),
            &generation,
            EventCollisionObservation::Absent
        )
        .code(),
        "generation-stale"
    );

    let sequence = mutated_event(&registry, &previous, |document| {
        document["sequence"] = Value::from(3);
    });
    assert_eq!(
        evaluate_causal_transition(
            Some(&previous),
            &sequence,
            EventCollisionObservation::Absent
        )
        .code(),
        "sequence-invalid"
    );

    let predecessor = mutated_event(&registry, &previous, |document| {
        document["previousEventDigest"] = Value::String(
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_owned(),
        );
    });
    assert_eq!(
        evaluate_causal_transition(
            Some(&previous),
            &predecessor,
            EventCollisionObservation::Absent
        )
        .code(),
        "previous-digest-mismatch"
    );

    let previous_with_budget = parsed_event(&registry, 1, None, 2, 2);
    let decreased = mutated_event(&registry, &previous_with_budget, |document| {
        document["budgetTotal"]["toolCalls"] = Value::from(1);
    });
    assert_eq!(
        evaluate_causal_transition(
            Some(&previous_with_budget),
            &decreased,
            EventCollisionObservation::Absent
        )
        .code(),
        "budget-decreased"
    );

    let arithmetic = mutated_event(&registry, &previous, |document| {
        document["budgetDelta"]["toolCalls"] = Value::from(1);
        document["budgetTotal"]["toolCalls"] = Value::from(0);
    });
    assert_eq!(
        evaluate_causal_transition(
            Some(&previous),
            &arithmetic,
            EventCollisionObservation::Absent
        )
        .code(),
        "budget-arithmetic-invalid"
    );

    let valid = mutated_event(&registry, &previous, |document| {
        document["budgetDelta"]["toolCalls"] = Value::from(1);
        document["budgetTotal"]["toolCalls"] = Value::from(1);
    });
    assert_eq!(
        evaluate_causal_transition(Some(&previous), &valid, EventCollisionObservation::Absent)
            .code(),
        "event-valid"
    );
}

fn push_event(
    registry: &ContractRegistry,
    events: &mut Vec<AuthorizedExecutionEvent>,
    event_type: &str,
    step_id: Option<&str>,
    selected_edge_id: Option<&str>,
    outcome_code: Option<&str>,
    effect_status: Option<&str>,
) {
    let sequence = events.len() as u64 + 1;
    let previous_event_digest = events.last().map(AuthorizedExecutionEvent::digest);
    let document = event_document(&EventFixture {
        event_type,
        sequence,
        previous_event_digest,
        graph_digest: "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        step_id,
        attempt_id: matches!(
            event_type,
            "invocation-started"
                | "step-result-recorded"
                | "decision-requested"
                | "decision-consumed"
                | "effect-reserved"
                | "effect-started"
                | "effect-terminal"
                | "effect-unknown"
        )
        .then_some(ATTEMPT),
        worker_invocation_id: matches!(
            event_type,
            "invocation-started"
                | "step-result-recorded"
                | "effect-reserved"
                | "effect-started"
                | "effect-terminal"
                | "effect-unknown"
        )
        .then_some(WORKER),
        selected_edge_id,
        outcome_code,
        effect_status,
        tool_calls_delta: 0,
        tool_calls_total: 0,
    });
    events.push(
        parse_authorized_execution_event(registry, &document).expect("chain event must validate"),
    );
}

fn complete_chain(registry: &ContractRegistry) -> Vec<AuthorizedExecutionEvent> {
    let mut events = Vec::new();
    push_event(
        registry,
        &mut events,
        "graph-activated",
        None,
        None,
        None,
        None,
    );
    push_event(
        registry,
        &mut events,
        "step-authorized",
        Some(CALCULATION_STEP),
        None,
        None,
        None,
    );
    push_event(
        registry,
        &mut events,
        "invocation-started",
        Some(CALCULATION_STEP),
        None,
        None,
        None,
    );
    push_event(
        registry,
        &mut events,
        "step-result-recorded",
        Some(CALCULATION_STEP),
        Some("urn:libre-ai:edge:calculated-1"),
        Some("calculation-ready"),
        None,
    );
    push_event(
        registry,
        &mut events,
        "step-authorized",
        Some(DECISION_STEP),
        None,
        None,
        None,
    );
    push_event(
        registry,
        &mut events,
        "decision-requested",
        Some(DECISION_STEP),
        None,
        None,
        None,
    );
    push_event(
        registry,
        &mut events,
        "decision-consumed",
        Some(DECISION_STEP),
        Some("urn:libre-ai:edge:approved-1"),
        Some("approved"),
        None,
    );
    push_event(
        registry,
        &mut events,
        "step-authorized",
        Some(EFFECT_STEP),
        None,
        None,
        None,
    );
    push_event(
        registry,
        &mut events,
        "effect-reserved",
        Some(EFFECT_STEP),
        None,
        None,
        Some("reserved"),
    );
    push_event(
        registry,
        &mut events,
        "effect-started",
        Some(EFFECT_STEP),
        None,
        None,
        Some("started"),
    );
    push_event(
        registry,
        &mut events,
        "effect-terminal",
        Some(EFFECT_STEP),
        Some("urn:libre-ai:edge:committed-1"),
        None,
        Some("committed"),
    );
    push_event(
        registry,
        &mut events,
        "step-result-recorded",
        Some(EFFECT_STEP),
        Some("urn:libre-ai:edge:committed-1"),
        Some("effect-committed"),
        None,
    );
    push_event(
        registry,
        &mut events,
        "run-completed",
        None,
        None,
        Some("completed"),
        None,
    );
    events
}

#[test]
fn complete_chain_replays_deterministically() {
    let registry = ContractRegistry::embedded().expect("embedded registry");
    let graph =
        parse_authorized_graph(&registry, &valid_graph_document()).expect("valid graph fixture");
    let events = complete_chain(&registry);
    let independent_events = complete_chain(&registry);

    let left = replay_authorized_execution(&graph, &events).expect("first replay");
    let right = replay_authorized_execution(&graph, &independent_events).expect("second replay");
    assert_eq!(left, right);
    assert!(left.is_completed());
    assert_eq!(left.sequence(), 13);
    assert_eq!(left.ready_step_id(), Some(TERMINAL_STEP));
    assert_eq!(left.tool_calls_total(), 0);
    assert_eq!(encode_state_for_test(&left), encode_state_for_test(&right));
}

#[test]
fn exact_duplicate_replay_leaves_state_unchanged() {
    let registry = ContractRegistry::embedded().expect("embedded registry");
    let graph =
        parse_authorized_graph(&registry, &valid_graph_document()).expect("valid graph fixture");
    let canonical = complete_chain(&registry);
    let mut with_duplicate = Vec::with_capacity(canonical.len() + 1);
    with_duplicate.push(canonical[0].clone());
    with_duplicate.push(canonical[0].clone());
    with_duplicate.extend(canonical.iter().skip(1).cloned());

    let expected = replay_authorized_execution(&graph, &canonical).expect("canonical replay");
    let actual = replay_authorized_execution(&graph, &with_duplicate).expect("idempotent replay");
    assert_eq!(actual, expected);
}

#[test]
fn divergent_duplicate_and_second_ready_step_quarantine() {
    let registry = ContractRegistry::embedded().expect("embedded registry");
    let graph =
        parse_authorized_graph(&registry, &valid_graph_document()).expect("valid graph fixture");
    let canonical = complete_chain(&registry);

    let mut divergent_document = event_document(&EventFixture {
        event_type: "graph-activated",
        sequence: 1,
        previous_event_digest: None,
        graph_digest: "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        step_id: None,
        attempt_id: None,
        worker_invocation_id: None,
        selected_edge_id: None,
        outcome_code: None,
        effect_status: None,
        tool_calls_delta: 0,
        tool_calls_total: 0,
    });
    divergent_document["occurredAt"] = Value::String("2026-09-10T10:00:02Z".to_owned());
    reseal_event_document(&mut divergent_document);
    let divergent = parse_authorized_execution_event(&registry, &divergent_document)
        .expect("divergent event remains individually valid");
    let collision_state = replay_authorized_execution(&graph, &[canonical[0].clone(), divergent])
        .expect("divergent collision produces quarantined state");
    assert!(collision_state.is_quarantined());

    let mut second_ready = vec![canonical[0].clone()];
    push_event(
        &registry,
        &mut second_ready,
        "step-authorized",
        Some(CALCULATION_STEP),
        None,
        None,
        None,
    );
    let authorized_state = replay_authorized_execution(&graph, &second_ready)
        .expect("one authorized step is a valid state");
    assert_eq!(authorized_state.ready_step_id(), None);
    push_event(
        &registry,
        &mut second_ready,
        "step-authorized",
        Some(CALCULATION_STEP),
        None,
        None,
        None,
    );
    let forbidden_state = replay_authorized_execution(&graph, &second_ready)
        .expect("forbidden phase transition produces quarantined state");
    assert!(forbidden_state.is_quarantined());
    assert_eq!(forbidden_state.ready_step_id(), None);
}
