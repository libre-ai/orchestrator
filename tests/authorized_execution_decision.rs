mod support;

use libre_ai_agent_orchestrator::{DecisionDecision, DecisionObservation, evaluate_human_decision};
use libre_ai_contract_types::ContractRegistry;
use serde_json::Value;
use support::authorized_execution::{valid_decision_request, valid_decision_response};

const NOW: &str = "2026-09-10T11:00:00Z";
const APPROVER: &[&str] = &["mission-approver"];

fn registry() -> ContractRegistry {
    ContractRegistry::embedded().expect("embedded registry")
}

fn observation<'a>(
    request_replaced: bool,
    request_consumed: bool,
    actor_roles: &'a [&'a str],
    revision: u64,
    prior_response: Option<(&'a str, &'a str)>,
) -> DecisionObservation<'a> {
    DecisionObservation::authoritative(
        request_replaced,
        request_consumed,
        actor_roles,
        revision,
        prior_response,
    )
}

fn evaluate(
    request: &Value,
    response: &Value,
    observation: DecisionObservation<'_>,
    now: &str,
) -> DecisionDecision {
    evaluate_human_decision(&registry(), request, response, observation, now)
}

#[test]
fn decision_precedence_covers_every_locked_outcome() {
    let request = valid_decision_request();
    let response = valid_decision_response();

    let mut organization = response.clone();
    organization["organizationId"] = Value::String("ten_abcdef1234567890".to_owned());
    assert_eq!(
        evaluate(
            &request,
            &organization,
            observation(true, true, &[], 3, None),
            "2026-09-10T13:00:00Z",
        )
        .code(),
        "organization-mismatch"
    );

    let mut attempt = response.clone();
    attempt["attemptId"] = Value::String("urn:libre-ai:attempt:other".to_owned());
    assert_eq!(
        evaluate(
            &request,
            &attempt,
            observation(true, true, &[], 3, None),
            "2026-09-10T13:00:00Z",
        )
        .code(),
        "attempt-mismatch"
    );

    assert_eq!(
        evaluate(
            &request,
            &response,
            observation(true, true, &[], 3, None),
            "2026-09-10T13:00:00Z",
        )
        .code(),
        "request-replaced"
    );

    let response_id = response["id"].as_str().expect("response id");
    let response_digest = response["responseDigest"]
        .as_str()
        .expect("response digest");
    assert_eq!(
        evaluate(
            &request,
            &response,
            observation(false, true, &[], 3, Some((response_id, response_digest)),),
            "2026-09-10T13:00:00Z",
        )
        .code(),
        "idempotent-duplicate"
    );
    assert_eq!(
        evaluate(
            &request,
            &response,
            observation(
                false,
                true,
                &[],
                3,
                Some((
                    response_id,
                    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                )),
            ),
            "2026-09-10T13:00:00Z",
        )
        .code(),
        "duplicate-divergent"
    );

    assert_eq!(
        evaluate(
            &request,
            &response,
            observation(false, false, APPROVER, 4, None),
            "2026-09-10T12:00:00Z",
        )
        .code(),
        "request-expired"
    );
    assert_eq!(
        evaluate(
            &request,
            &response,
            observation(false, true, APPROVER, 4, None),
            NOW,
        )
        .code(),
        "request-consumed"
    );

    let mut unknown_choice = response.clone();
    unknown_choice["choiceId"] = Value::String("unknown".to_owned());
    assert_eq!(
        evaluate(
            &request,
            &unknown_choice,
            observation(false, false, APPROVER, 4, None),
            NOW,
        )
        .code(),
        "choice-unknown"
    );
    assert_eq!(
        evaluate(
            &request,
            &response,
            observation(false, false, &["viewer"], 4, None),
            NOW,
        )
        .code(),
        "actor-unauthorized"
    );

    let mut stale = response.clone();
    stale["expectedRevision"] = Value::from(3);
    assert_eq!(
        evaluate(
            &request,
            &stale,
            observation(false, false, APPROVER, 4, None),
            NOW,
        )
        .code(),
        "revision-stale"
    );

    let decision = evaluate(
        &request,
        &response,
        observation(false, false, APPROVER, 4, None),
        NOW,
    );
    assert_eq!(decision.code(), "decision-valid");
    let application = decision.application().expect("valid application");
    assert_eq!(
        application.request_digest(),
        "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
    );
    assert_eq!(application.outcome_code(), "approved");
    assert_eq!(application.expected_revision(), 4);
    let diagnostics = format!("{decision:?} {decision}");
    for private_value in [
        "ten_1234567890abcdef",
        "urn:libre-ai:run:synthetic-run-1",
        application.request_digest(),
    ] {
        assert!(!diagnostics.contains(private_value));
    }
}

#[test]
fn missing_prior_response_observation_refuses_closed() {
    let decision = evaluate_human_decision(
        &registry(),
        &valid_decision_request(),
        &valid_decision_response(),
        DecisionObservation::unavailable(),
        "2026-09-10T12:00:00Z",
    );
    assert_eq!(
        decision.code(),
        "orchestrator.authorized-execution.store-unavailable"
    );
}

#[test]
fn invalid_evaluation_time_refuses_as_schema_invalid_without_reflection() {
    let rejected = "synthetic-private-invalid-time";
    let decision = evaluate(
        &valid_decision_request(),
        &valid_decision_response(),
        observation(false, false, APPROVER, 4, None),
        rejected,
    );
    assert_eq!(
        decision.code(),
        "orchestrator.authorized-execution.schema-invalid"
    );
    assert!(!format!("{decision:?}").contains(rejected));
}
