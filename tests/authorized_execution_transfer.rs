mod support;

use libre_ai_agent_orchestrator::{
    TransferDecision, TransferObservation, evaluate_execution_transfer,
};
use libre_ai_contract_types::ContractRegistry;
use serde_json::Value;
use support::authorized_execution::valid_execution_transfer;

const ORGANIZATION: &str = "ten_1234567890abcdef";
const MISSION: &str = "urn:libre-ai:mission:synthetic-mission-1";
const RUN: &str = "urn:libre-ai:run:synthetic-run-1";
const PREDECESSOR_PLAN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SUCCESSOR_PLAN: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const TRANSFER_ID: &str = "urn:libre-ai:transfer:synthetic-transfer-1";
const TRANSFER_DIGEST: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const NOW: &str = "2026-09-10T10:05:00Z";

fn registry() -> ContractRegistry {
    ContractRegistry::embedded().expect("embedded registry")
}

fn observation<'a>(
    successor_plan_digest: &'a str,
    current_generation: u64,
    revision: u64,
    generation_consumed: bool,
    prior_transfer: Option<(&'a str, u64, &'a str)>,
) -> TransferObservation<'a> {
    TransferObservation::Authoritative {
        organization_id: ORGANIZATION,
        mission_id: MISSION,
        predecessor_run_id: RUN,
        predecessor_plan_digest: PREDECESSOR_PLAN,
        successor_plan_digest,
        current_generation,
        revision,
        generation_consumed,
        prior_transfer,
    }
}

fn evaluate(transfer: &Value, observation: TransferObservation<'_>, now: &str) -> TransferDecision {
    evaluate_execution_transfer(&registry(), transfer, observation, now)
}

#[test]
fn transfer_precedence_covers_every_locked_outcome() {
    let transfer = valid_execution_transfer();

    assert_eq!(
        evaluate(
            &transfer,
            observation(
                SUCCESSOR_PLAN,
                1,
                4,
                true,
                Some((TRANSFER_ID, 1, TRANSFER_DIGEST)),
            ),
            "2026-09-10T10:15:00Z",
        )
        .code(),
        "idempotent-duplicate"
    );
    assert_eq!(
        evaluate(
            &transfer,
            observation(
                SUCCESSOR_PLAN,
                1,
                4,
                true,
                Some((
                    TRANSFER_ID,
                    1,
                    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                )),
            ),
            "2026-09-10T10:15:00Z",
        )
        .code(),
        "duplicate-divergent"
    );
    assert_eq!(
        evaluate(
            &transfer,
            observation(SUCCESSOR_PLAN, 1, 4, false, None),
            "2026-09-10T10:15:00Z",
        )
        .code(),
        "transfer-expired"
    );
    assert_eq!(
        evaluate(
            &transfer,
            observation(SUCCESSOR_PLAN, 1, 4, true, None),
            NOW,
        )
        .code(),
        "generation-consumed"
    );

    let mut identity = transfer.clone();
    identity["successorPlanDigest"] = Value::String(
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_owned(),
    );
    assert_eq!(
        evaluate(
            &identity,
            observation(SUCCESSOR_PLAN, 1, 4, false, None),
            NOW,
        )
        .code(),
        "identity-mismatch"
    );

    let mut generation = transfer.clone();
    generation["currentGeneration"] = Value::from(2);
    assert_eq!(
        evaluate(
            &generation,
            observation(SUCCESSOR_PLAN, 1, 4, false, None),
            NOW,
        )
        .code(),
        "generation-stale"
    );

    let mut revision = transfer.clone();
    revision["expectedRevision"] = Value::from(3);
    assert_eq!(
        evaluate(
            &revision,
            observation(SUCCESSOR_PLAN, 1, 4, false, None),
            NOW,
        )
        .code(),
        "revision-stale"
    );

    let authoritative = observation(SUCCESSOR_PLAN, 1, 4, false, None);
    let unchanged = authoritative;
    let decision = evaluate(&transfer, authoritative, NOW);
    assert_eq!(authoritative, unchanged);
    assert_eq!(decision.code(), "transfer-valid");
    let application = decision.application().expect("valid application");
    assert_eq!(application.expected_seal_revision(), 4);
    assert_eq!(application.successor_generation(), 2);
    let diagnostics = format!("{decision:?} {decision}");
    for private_value in [ORGANIZATION, RUN, PREDECESSOR_PLAN, SUCCESSOR_PLAN] {
        assert!(!diagnostics.contains(private_value));
    }
}

#[test]
fn unavailable_lineage_and_invalid_time_refuse_closed() {
    let transfer = valid_execution_transfer();
    assert_eq!(
        evaluate_execution_transfer(
            &registry(),
            &transfer,
            TransferObservation::unavailable(),
            NOW,
        )
        .code(),
        "orchestrator.authorized-execution.store-unavailable"
    );

    let rejected = "synthetic-private-invalid-time";
    let decision = evaluate(
        &transfer,
        observation(SUCCESSOR_PLAN, 1, 4, false, None),
        rejected,
    );
    assert_eq!(
        decision.code(),
        "orchestrator.authorized-execution.schema-invalid"
    );
    assert!(!format!("{decision:?}").contains(rejected));
}
