#![allow(dead_code)]

use std::fmt::Write;
use std::fs;
use std::path::PathBuf;

use serde_json::Value;
use sha2::{Digest, Sha256};

use libre_ai_agent_orchestrator::AuthorizedExecutionState;

const SCHEMA_FIXTURES: &str =
    "node_modules/@libre-ai/contracts-authority/contracts/fixtures/schema-fixtures.v1.json";

pub fn schema_fixture(schema_name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(SCHEMA_FIXTURES);
    let source = fs::read_to_string(path).expect("locked schema fixtures must be readable");
    let document: Value =
        serde_json::from_str(&source).expect("locked schema fixtures must contain valid JSON");

    document["cases"]
        .as_array()
        .expect("schema fixtures must contain an array")
        .iter()
        .find(|fixture| fixture["schema"].as_str() == Some(schema_name))
        .and_then(|fixture| fixture.get("valid"))
        .cloned()
        .expect("requested locked schema fixture must exist")
}

pub fn valid_graph_document() -> Value {
    schema_fixture("execution-graph.v1.schema.json")
}

#[allow(dead_code)]
pub fn valid_plan_document() -> Value {
    schema_fixture("execution-plan-body.v2.schema.json")
}

pub fn valid_decision_request() -> Value {
    schema_fixture("human-decision-request.v1.schema.json")
}

pub fn valid_decision_response() -> Value {
    let request = valid_decision_request();
    let mut response = schema_fixture("human-decision-response.v1.schema.json");
    response["requestDigest"] = request["requestDigest"].clone();
    response
}

pub fn valid_execution_transfer() -> Value {
    schema_fixture("execution-transfer.v1.schema.json")
}

pub struct EventFixture<'a> {
    pub event_type: &'a str,
    pub sequence: u64,
    pub previous_event_digest: Option<&'a str>,
    pub graph_digest: &'a str,
    pub step_id: Option<&'a str>,
    pub attempt_id: Option<&'a str>,
    pub worker_invocation_id: Option<&'a str>,
    pub selected_edge_id: Option<&'a str>,
    pub outcome_code: Option<&'a str>,
    pub effect_status: Option<&'a str>,
    pub tool_calls_delta: u64,
    pub tool_calls_total: u64,
}

pub fn event_document(fixture: &EventFixture<'_>) -> Value {
    let mut document = schema_fixture("orchestrator-event.v3.schema.json");
    document["id"] = Value::String(format!("urn:libre-ai:event:event-{}", fixture.sequence));
    document["sequence"] = Value::from(fixture.sequence);
    document["previousEventDigest"] = fixture
        .previous_event_digest
        .map_or(Value::Null, |digest| Value::String(digest.to_owned()));
    document["graphDigest"] = Value::String(fixture.graph_digest.to_owned());
    document["stepId"] = optional_string(fixture.step_id);
    document["attemptId"] = optional_string(fixture.attempt_id);
    document["workerInvocationId"] = optional_string(fixture.worker_invocation_id);
    document["selectedEdgeId"] = optional_string(fixture.selected_edge_id);
    document["type"] = Value::String(fixture.event_type.to_owned());
    document["budgetDelta"] = budget_counters(fixture.tool_calls_delta);
    document["budgetTotal"] = budget_counters(fixture.tool_calls_total);
    document["cause"] = serde_json::json!({
        "kind": if fixture.event_type.starts_with("effect-") { "effect" } else { "event" },
        "id": "urn:libre-ai:event:synthetic-cause",
        "digest": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
    });
    document["data"] = event_data(fixture);
    reseal_event_document(&mut document);
    document
}

pub fn reseal_event_document(document: &mut Value) {
    let object = document
        .as_object_mut()
        .expect("event document must be an object");
    object.remove("eventDigest");
    let canonical = serde_jcs::to_vec(document).expect("synthetic event must canonicalize");
    let mut digest = String::with_capacity(64);
    for byte in Sha256::digest(canonical) {
        write!(&mut digest, "{byte:02x}").expect("writing to a String cannot fail");
    }
    document["eventDigest"] = Value::String(digest);
}

#[allow(dead_code)]
pub fn encode_state_for_test(state: &AuthorizedExecutionState) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        state.sequence(),
        state.event_digest(),
        state.ready_step_id().unwrap_or("none"),
        state.tool_calls_total(),
        state.is_completed(),
        state.is_quarantined(),
    )
}

fn optional_string(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |value| Value::String(value.to_owned()))
}

fn budget_counters(tool_calls: u64) -> Value {
    serde_json::json!({
        "durationSeconds": 0,
        "toolCalls": tool_calls,
        "inputTokens": 0,
        "outputTokens": 0,
        "processesStarted": 0,
        "filesChanged": 0,
        "changedBytes": 0
    })
}

fn event_data(fixture: &EventFixture<'_>) -> Value {
    match fixture.event_type {
        "graph-activated" => serde_json::json!({
            "graphRef": artifact_reference("urn:libre-ai:graph:synthetic-graph-1")
        }),
        "step-authorized" => serde_json::json!({
            "authorizationRef": artifact_reference("urn:libre-ai:authorization:synthetic")
        }),
        "invocation-started" => serde_json::json!({
            "invocationRef": artifact_reference("urn:libre-ai:invocation:synthetic")
        }),
        "step-result-recorded" => serde_json::json!({
            "resultRef": artifact_reference("urn:libre-ai:result:synthetic"),
            "outcomeCode": fixture.outcome_code.expect("result outcome")
        }),
        "decision-requested" => serde_json::json!({
            "decisionRequestRef": artifact_reference("urn:libre-ai:decision-request:synthetic")
        }),
        "decision-consumed" => serde_json::json!({
            "decisionResponseRef": artifact_reference("urn:libre-ai:decision-response:synthetic"),
            "outcomeCode": fixture.outcome_code.expect("decision outcome")
        }),
        "effect-reserved" | "effect-started" | "effect-terminal" | "effect-unknown" => {
            serde_json::json!({
                "effectId": "urn:libre-ai:effect:synthetic",
                "effectEmissionId": "urn:libre-ai:emission:synthetic",
                "effectStatus": fixture.effect_status.expect("effect status"),
                "lifecycleRef": artifact_reference("urn:libre-ai:effect-attestation:synthetic")
            })
        }
        "predecessor-sealed" => serde_json::json!({
            "sealedRevision": 1,
            "terminalEffectInventoryDigest": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
        }),
        "generation-transferred" => serde_json::json!({
            "executionTransferRef": artifact_reference("urn:libre-ai:transfer:synthetic")
        }),
        "run-blocked" | "quarantined" => serde_json::json!({
            "reasonCode": "orchestrator-causal-conflict",
            "lifecycleRef": artifact_reference("urn:libre-ai:evidence:synthetic")
        }),
        "run-completed" => serde_json::json!({
            "outcomeCode": fixture.outcome_code.expect("completion outcome")
        }),
        other => panic!("unsupported synthetic event type: {other}"),
    }
}

fn artifact_reference(id: &str) -> Value {
    serde_json::json!({
        "id": id,
        "digest": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "mediaType": "application/json"
    })
}
