use std::fmt::Write;
use std::hint::black_box;
use std::time::Instant;

use libre_ai_agent_orchestrator::{
    AuthorizedExecutionEvent, evaluate_graph, parse_authorized_execution_event,
    parse_authorized_graph, replay_authorized_execution, select_graph_transition,
};
use libre_ai_contract_types::ContractRegistry;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const ORGANIZATION: &str = "ten_1234567890abcdef";
const GRAPH_ID: &str = "urn:libre-ai:graph:benchmark-graph";
const GRAPH_DIGEST: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const ATTEMPT: &str = "urn:libre-ai:attempt:benchmark-attempt";
const WORKER: &str = "urn:libre-ai:worker-invocation:benchmark-worker";

fn step_id(index: usize) -> String {
    format!("urn:libre-ai:step:n{index}")
}

fn edge_id(kind: &str, index: usize) -> String {
    format!("urn:libre-ai:edge:{kind}-{index}")
}

fn maximum_graph_document() -> Value {
    let mut steps = Vec::with_capacity(256);
    let mut edges = Vec::with_capacity(512);
    for index in 0..255 {
        let mut outcomes = vec!["continue", "alternate"];
        if index < 2 {
            outcomes.push("alternate-two");
        }
        steps.push(json!({
            "stepId": step_id(index),
            "kind": "calculation",
            "outcomeCodes": outcomes,
            "retryPolicy": {
                "maximumAttempts": 1,
                "retryableOutcomeCodes": []
            }
        }));
        edges.push(json!({
            "edgeId": edge_id("continue", index),
            "fromStepId": step_id(index),
            "outcomeCode": "continue",
            "toStepId": step_id(index + 1)
        }));
        edges.push(json!({
            "edgeId": edge_id("alternate", index),
            "fromStepId": step_id(index),
            "outcomeCode": "alternate",
            "toStepId": step_id(255)
        }));
        if index < 2 {
            edges.push(json!({
                "edgeId": edge_id("alternate-two", index),
                "fromStepId": step_id(index),
                "outcomeCode": "alternate-two",
                "toStepId": step_id(255)
            }));
        }
    }
    steps.push(json!({
        "stepId": step_id(255),
        "kind": "terminal",
        "outcomeCodes": []
    }));
    json!({
        "schemaVersion": "libre-ai.execution-graph.v1",
        "id": GRAPH_ID,
        "organizationId": ORGANIZATION,
        "entryStepId": step_id(0),
        "steps": steps,
        "edges": edges,
        "createdAt": "2026-09-10T10:00:00Z",
        "graphDigest": GRAPH_DIGEST
    })
}

struct EventInput<'a> {
    event_type: &'a str,
    step_id: Option<String>,
    selected_edge_id: Option<String>,
    outcome_code: Option<&'a str>,
}

fn append_event(
    registry: &ContractRegistry,
    events: &mut Vec<AuthorizedExecutionEvent>,
    input: EventInput<'_>,
) {
    let sequence = u64::try_from(events.len())
        .ok()
        .and_then(|value| value.checked_add(1))
        .expect("bounded benchmark sequence");
    let previous_event_digest = events.last().map_or(Value::Null, |event| {
        Value::String(event.digest().to_owned())
    });
    let has_attempt = matches!(
        input.event_type,
        "invocation-started" | "step-result-recorded"
    );
    let selected_edge_id = input
        .selected_edge_id
        .as_ref()
        .map_or(Value::Null, |value| Value::String(value.clone()));
    let data = match input.event_type {
        "graph-activated" => json!({
            "graphRef": artifact_reference(GRAPH_ID)
        }),
        "step-authorized" => json!({
            "authorizationRef": artifact_reference("urn:libre-ai:authorization:benchmark")
        }),
        "invocation-started" => json!({
            "invocationRef": artifact_reference("urn:libre-ai:invocation:benchmark")
        }),
        "step-result-recorded" => json!({
            "resultRef": artifact_reference("urn:libre-ai:result:benchmark"),
            "outcomeCode": input.outcome_code.expect("result outcome")
        }),
        "run-completed" => json!({ "outcomeCode": "completed" }),
        _ => panic!("unsupported benchmark event"),
    };
    let mut document = json!({
        "schemaVersion": "libre-ai.orchestrator-event.v3",
        "id": format!("urn:libre-ai:event:benchmark-{sequence}"),
        "organizationId": ORGANIZATION,
        "missionId": "urn:libre-ai:mission:benchmark",
        "runId": "urn:libre-ai:run:benchmark",
        "orchestratorId": "orchestrator_benchmark",
        "planDigest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "authorizationDigest": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "graphDigest": GRAPH_DIGEST,
        "generation": 1,
        "sequence": sequence,
        "previousEventDigest": previous_event_digest,
        "stepId": input.step_id,
        "attemptId": if has_attempt { Value::String(ATTEMPT.to_owned()) } else { Value::Null },
        "workerInvocationId": if has_attempt { Value::String(WORKER.to_owned()) } else { Value::Null },
        "selectedEdgeId": selected_edge_id,
        "cause": {
            "kind": "event",
            "id": "urn:libre-ai:event:benchmark-cause",
            "digest": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
        },
        "type": input.event_type,
        "budgetDelta": budget_counters(),
        "budgetTotal": budget_counters(),
        "occurredAt": "2026-09-10T10:00:00Z",
        "data": data
    });
    let canonical = serde_jcs::to_vec(&document).expect("benchmark event canonicalization");
    let mut digest = String::with_capacity(64);
    for byte in Sha256::digest(canonical) {
        write!(&mut digest, "{byte:02x}").expect("String writes cannot fail");
    }
    document["eventDigest"] = Value::String(digest);
    events.push(
        parse_authorized_execution_event(registry, &document)
            .expect("benchmark event must validate"),
    );
}

fn artifact_reference(id: &str) -> Value {
    json!({
        "id": id,
        "digest": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "mediaType": "application/json"
    })
}

fn budget_counters() -> Value {
    json!({
        "durationSeconds": 0,
        "toolCalls": 0,
        "inputTokens": 0,
        "outputTokens": 0,
        "processesStarted": 0,
        "filesChanged": 0,
        "changedBytes": 0
    })
}

fn long_event_chain(registry: &ContractRegistry) -> Vec<AuthorizedExecutionEvent> {
    let mut events = Vec::with_capacity(767);
    append_event(
        registry,
        &mut events,
        EventInput {
            event_type: "graph-activated",
            step_id: None,
            selected_edge_id: None,
            outcome_code: None,
        },
    );
    for index in 0..255 {
        append_event(
            registry,
            &mut events,
            EventInput {
                event_type: "step-authorized",
                step_id: Some(step_id(index)),
                selected_edge_id: None,
                outcome_code: None,
            },
        );
        append_event(
            registry,
            &mut events,
            EventInput {
                event_type: "invocation-started",
                step_id: Some(step_id(index)),
                selected_edge_id: None,
                outcome_code: None,
            },
        );
        append_event(
            registry,
            &mut events,
            EventInput {
                event_type: "step-result-recorded",
                step_id: Some(step_id(index)),
                selected_edge_id: Some(edge_id("continue", index)),
                outcome_code: Some("continue"),
            },
        );
    }
    append_event(
        registry,
        &mut events,
        EventInput {
            event_type: "run-completed",
            step_id: None,
            selected_edge_id: None,
            outcome_code: None,
        },
    );
    events
}

fn main() {
    let registry = ContractRegistry::embedded().expect("embedded registry");
    let graph_document = maximum_graph_document();
    let graph = parse_authorized_graph(&registry, &graph_document).expect("maximum graph");
    let events = long_event_chain(&registry);
    replay_authorized_execution(&graph, &events).expect("benchmark replay");

    let iterations = 100_u32;
    let graph_started = Instant::now();
    for _ in 0..iterations {
        black_box(evaluate_graph(black_box(&graph)));
    }
    let graph_elapsed = graph_started.elapsed();

    let route_started = Instant::now();
    for _ in 0..iterations {
        black_box(select_graph_transition(
            black_box(&graph),
            black_box(&step_id(127)),
            black_box("continue"),
        ));
    }
    let route_elapsed = route_started.elapsed();

    let replay_started = Instant::now();
    for _ in 0..iterations {
        black_box(
            replay_authorized_execution(black_box(&graph), black_box(&events))
                .expect("benchmark replay"),
        );
    }
    let replay_elapsed = replay_started.elapsed();

    println!(
        "authorized_execution.graph_validation iterations={} ns_per_iteration={}",
        iterations,
        graph_elapsed.as_nanos() / iterations as u128,
    );
    println!(
        "authorized_execution.route_selection iterations={} ns_per_iteration={}",
        iterations,
        route_elapsed.as_nanos() / iterations as u128,
    );
    println!(
        "authorized_execution.replay events={} iterations={} ns_per_event={}",
        events.len(),
        iterations,
        replay_elapsed.as_nanos() / (events.len() as u128 * iterations as u128),
    );
}
