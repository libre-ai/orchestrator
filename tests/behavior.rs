use libre_ai_agent_orchestrator::{
    CanonicalReference, ErrorCode, EventStreamSummary, MissionBudgets, MissionContext,
    NetworkBudget, Scenario, SimulationRequest, event_id, idempotency_key, simulate_v1,
    validate_event_values_v1,
};
use serde_json::json;
use std::fs;
use std::path::Path;

fn handoff() -> Vec<u8> {
    fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/handoff.valid.json"))
        .expect("handoff fixture")
}

fn context(duration: u64) -> MissionContext {
    MissionContext {
        mission_id: "urn:libre-ai:mission:mission-1".to_owned(),
        started_at: "2026-07-16T00:00:00Z".parse().expect("timestamp"),
        budgets: MissionBudgets {
            max_duration_seconds: duration,
            max_tool_calls: 0,
            network: NetworkBudget::None,
        },
    }
}

fn reference(kind: &str, digest: char) -> CanonicalReference {
    CanonicalReference {
        id: format!("urn:libre-ai:{kind}:{kind}-1"),
        digest: digest.to_string().repeat(64),
        media_type: "application/json".to_owned(),
    }
}

fn request(scenario: Scenario, duration: u64) -> SimulationRequest {
    SimulationRequest {
        context: context(duration),
        scenario,
        artifact: (scenario == Scenario::Complete).then(|| reference("artifact", 'b')),
        evidence: (scenario == Scenario::Complete).then(|| reference("evidence", 'c')),
    }
}

#[test]
fn complete_is_deterministic_and_valid() {
    let handoff = handoff();
    let request = request(Scenario::Complete, 2);
    let first = simulate_v1(&handoff, &request).expect("simulation");
    assert_eq!(first, simulate_v1(&handoff, &request).expect("replay"));
    assert_eq!(
        validate_event_values_v1(&handoff, &request.context, &first),
        Ok(EventStreamSummary {
            accepted_events: 3,
            replayed_events: 0,
            cursor: 3,
        })
    );
}

#[test]
fn fixed_scenarios_and_duration_budget_are_bounded() {
    let handoff = handoff();
    let blocked = simulate_v1(&handoff, &request(Scenario::Blocked, 2)).expect("blocked");
    let budget = simulate_v1(&handoff, &request(Scenario::Complete, 1)).expect("budget");
    let failed = simulate_v1(&handoff, &request(Scenario::Failed, 1)).expect("failed");
    assert_eq!(blocked[2]["type"], "decision-requested");
    assert_eq!(budget[1]["type"], "budget-exceeded");
    assert_eq!(failed[1]["type"], "failed");
    assert!(blocked.len() <= 3 && budget.len() <= 3 && failed.len() <= 3);
}

#[test]
fn exact_replay_is_noop_and_mutation_conflicts() {
    let handoff = handoff();
    let request = request(Scenario::Complete, 2);
    let events = simulate_v1(&handoff, &request).expect("events");
    let mut replay = vec![
        events[0].clone(),
        events[0].clone(),
        events[1].clone(),
        events[2].clone(),
    ];
    assert_eq!(
        validate_event_values_v1(&handoff, &request.context, &replay)
            .expect("exact replay")
            .replayed_events,
        1
    );
    replay[1]["occurredAt"] = json!("2026-07-16T00:00:01Z");
    assert_eq!(
        validate_event_values_v1(&handoff, &request.context, &replay)
            .expect_err("mutated replay")
            .code(),
        ErrorCode::IdempotencyConflict
    );
}

#[test]
fn gaps_context_and_forged_ids_fail_closed() {
    let handoff = handoff();
    let request = request(Scenario::Complete, 2);
    let events = simulate_v1(&handoff, &request).expect("events");
    assert_eq!(
        validate_event_values_v1(&handoff, &request.context, &events[1..])
            .expect_err("gap")
            .code(),
        ErrorCode::EventSequenceInvalid
    );
    let mut wrong = events.clone();
    wrong[0]["tenantId"] = json!("ten_ffffffffffffffff");
    assert_eq!(
        validate_event_values_v1(&handoff, &request.context, &wrong)
            .expect_err("context")
            .code(),
        ErrorCode::EventContextMismatch
    );
    let mut forged = events;
    forged[0]["id"] = json!("urn:libre-ai:event:forged");
    assert_eq!(
        validate_event_values_v1(&handoff, &request.context, &forged)
            .expect_err("id")
            .code(),
        ErrorCode::EventIdInvalid
    );
}

#[test]
fn stricter_profile_rejects_pause_progress_and_reference_confusion() {
    let handoff = handoff();
    let request = request(Scenario::Complete, 2);
    let events = simulate_v1(&handoff, &request).expect("events");
    let mut paused = events.clone();
    paused[1]["type"] = json!("paused");
    paused[1]["id"] = json!(event_id(
        "ten_1234567890abcdef",
        &request.context.mission_id,
        2,
        "paused"
    ));
    paused[1]["data"] = json!({});
    assert_eq!(
        validate_event_values_v1(&handoff, &request.context, &paused)
            .expect_err("pause")
            .code(),
        ErrorCode::EventTypeForbidden
    );
    let mut regressed = events.clone();
    regressed[1]["data"]["progressPermille"] = json!(0);
    assert_eq!(
        validate_event_values_v1(&handoff, &request.context, &regressed)
            .expect_err("progress")
            .code(),
        ErrorCode::EventTransitionInvalid
    );
    let mut confused = events;
    confused[2]["data"]["artifact"]["id"] = json!("urn:libre-ai:evidence:not-artifact");
    assert_eq!(
        validate_event_values_v1(&handoff, &request.context, &confused)
            .expect_err("reference")
            .code(),
        ErrorCode::EventTransitionInvalid
    );
}

#[test]
fn runtime_schema_enforces_result_evidence() {
    let handoff = handoff();
    let request = request(Scenario::Complete, 2);
    let mut events = simulate_v1(&handoff, &request).expect("events");
    events[2]["data"]
        .as_object_mut()
        .expect("data")
        .remove("evidence");
    assert_eq!(
        validate_event_values_v1(&handoff, &request.context, &events)
            .expect_err("evidence")
            .code(),
        ErrorCode::ContractInvalid
    );
}

#[test]
fn network_and_expired_handoff_are_refused() {
    let handoff = handoff();
    let mut network = request(Scenario::Failed, 1);
    network.context.budgets.network = NetworkBudget::Allowlisted;
    assert_eq!(
        simulate_v1(&handoff, &network).expect_err("network").code(),
        ErrorCode::G2NetworkUnsupported
    );
    let mut expired = request(Scenario::Failed, 1);
    expired.context.started_at = "2026-07-17T00:00:00Z".parse().expect("timestamp");
    assert_eq!(
        simulate_v1(&handoff, &expired).expect_err("expiry").code(),
        ErrorCode::HandoffExpired
    );
}

#[test]
fn ids_and_http_idempotency_keys_are_stable() {
    let id = event_id(
        "ten_1234567890abcdef",
        "urn:libre-ai:mission:mission-1",
        1,
        "started",
    );
    assert_eq!(
        id,
        "urn:libre-ai:event:f7fdab686b3227f933b45764620fd62074a7a26d515bdc7d02bc80ae7dd5a387"
    );
    assert_eq!(
        idempotency_key(&id).as_deref(),
        Some("idem_f7fdab686b3227f933b45764620fd62074a7a26d515bdc7d02bc80ae7dd5a387")
    );
}
