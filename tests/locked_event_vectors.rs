use libre_ai_agent_orchestrator::{EventStoreObservation, PlanBudgetLimits, evaluate_budget_event};
use libre_ai_contract_types::event_chain::{AcceptedEventCollision, OrchestratorCausalEventFacts};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Vectors {
    pair: Value,
    genesis: Value,
    cases: Vec<VectorCase>,
}

#[derive(Deserialize)]
struct VectorCase {
    id: String,
    scenario: String,
    mutations: Vec<Mutation>,
    collision: String,
    expected: String,
}

#[derive(Deserialize)]
struct Mutation {
    target: String,
    path: String,
    value: Value,
}

fn set_path(target: &mut Value, path: &str, value: Value) {
    let mut segments = path.split('.').peekable();
    let mut cursor = target;
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            cursor
                .as_object_mut()
                .expect("mutation parent must be an object")
                .insert(segment.to_owned(), value);
            return;
        }
        cursor = cursor
            .as_object_mut()
            .and_then(|object| object.get_mut(segment))
            .expect("mutation path must exist");
    }
    panic!("mutation path cannot be empty");
}

fn collision(mode: &str, current: &OrchestratorCausalEventFacts) -> Option<AcceptedEventCollision> {
    match mode {
        "none" => None,
        "exact-current" => Some(AcceptedEventCollision {
            id: current.id.clone(),
            sequence: current.sequence,
            event_digest: current.event_digest.clone(),
        }),
        "same-id-different-digest" => Some(AcceptedEventCollision {
            id: current.id.clone(),
            sequence: current.sequence,
            event_digest: "b".repeat(64),
        }),
        "same-sequence-different-id" => Some(AcceptedEventCollision {
            id: "urn:libre-ai:event:collision".to_owned(),
            sequence: current.sequence,
            event_digest: "b".repeat(64),
        }),
        _ => panic!("unknown collision mode"),
    }
}

fn schema_maximum_fixture_limits(plan_digest: String) -> PlanBudgetLimits {
    PlanBudgetLimits {
        plan_digest,
        max_duration_seconds: 604_800,
        max_tool_calls: 100_000,
        max_input_tokens: 1_000_000_000,
        max_output_tokens: 1_000_000_000,
        max_processes: 1_024,
        max_files_changed: 100_000,
        max_changed_bytes: 1_073_741_824,
    }
}

#[test]
fn control_core_preserves_every_locked_event_chain_reason_code() {
    let document = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/fixtures/agent-orchestration-v1/event-chain-vectors.v1.json"
    ));
    let vectors: Vectors = serde_json::from_str(document).expect("locked vectors must parse");

    for case in vectors.cases {
        let mut scenario = match case.scenario.as_str() {
            "pair" => vectors.pair.clone(),
            "genesis" => vectors.genesis.clone(),
            _ => panic!("unknown scenario"),
        };
        for mutation in case.mutations {
            let target = scenario
                .as_object_mut()
                .and_then(|object| object.get_mut(&mutation.target))
                .expect("mutation target must exist");
            set_path(target, &mutation.path, mutation.value);
        }

        let object = scenario.as_object().expect("scenario must be an object");
        let previous: Option<OrchestratorCausalEventFacts> =
            serde_json::from_value(object.get("previous").expect("previous").clone())
                .expect("previous event must parse");
        let current: OrchestratorCausalEventFacts =
            serde_json::from_value(object.get("current").expect("current").clone())
                .expect("current event must parse");
        let collision = collision(&case.collision, &current);
        let limits = schema_maximum_fixture_limits(current.plan_digest.clone());

        assert_eq!(
            evaluate_budget_event(
                EventStoreObservation::Available {
                    previous: previous.as_ref(),
                    collision: collision.as_ref(),
                },
                &current,
                &limits
            )
            .code(),
            case.expected,
            "{}",
            case.id
        );
    }
}
