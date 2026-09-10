mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use libre_ai_agent_orchestrator::{
    DecisionObservation, EffectObservation, EventCollisionObservation, TransferObservation,
    evaluate_causal_transition, evaluate_effect_attestation, evaluate_execution_transfer,
    evaluate_graph, evaluate_graph_authority, evaluate_human_decision,
    parse_authorized_execution_event, parse_authorized_graph,
};
use libre_ai_contract_types::ContractRegistry;
use serde::Deserialize;
use serde_json::{Value, json};
use support::authorized_execution::{EventFixture, event_document, reseal_event_document};

const AUTHORITY_ROOT: &str = "node_modules/@libre-ai/contracts-authority";
const SEMANTIC_VECTORS: &str =
    "contracts/fixtures/authorized-execution-v1/semantic-vectors.v1.json";
const SCHEMA_FIXTURES: &str = "contracts/fixtures/schema-fixtures.v1.json";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorDocument {
    schema_version: String,
    cases: Vec<VectorCase>,
}

#[derive(Deserialize)]
struct VectorCase {
    id: String,
    domain: String,
    input: Value,
    expected: String,
}

fn authority_document(path: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(AUTHORITY_ROOT)
        .join(path);
    let source = fs::read_to_string(path).expect("locked authority must be readable");
    serde_json::from_str(&source).expect("locked authority must be valid JSON")
}

fn adapt_and_evaluate(
    vector: &VectorCase,
    fixtures: &Value,
    registry: &ContractRegistry,
) -> Result<&'static str, &'static str> {
    match vector.domain.as_str() {
        "graph" => adapt_graph(vector, fixtures, registry),
        "causal" => adapt_causal(vector, registry),
        "decision" => adapt_decision(vector, fixtures, registry),
        "effect" => adapt_effect(vector, fixtures, registry),
        "authority" => adapt_authority(vector, fixtures, registry),
        "transfer" => adapt_transfer(vector, fixtures, registry),
        _ => Err("adapter-domain"),
    }
}

fn fixture(fixtures: &Value, schema: &str) -> Result<Value, &'static str> {
    fixtures["cases"]
        .as_array()
        .ok_or("fixture-cases")?
        .iter()
        .find(|candidate| candidate["schema"].as_str() == Some(schema))
        .and_then(|candidate| candidate.get("valid"))
        .cloned()
        .ok_or("fixture-missing")
}

fn string(value: &Value, field: &str) -> Result<String, &'static str> {
    value[field]
        .as_str()
        .map(str::to_owned)
        .ok_or("adapter-string")
}

fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str, &'static str> {
    value[field].as_str().ok_or("adapter-string")
}

fn number(value: &Value, field: &str) -> Result<u64, &'static str> {
    value[field].as_u64().ok_or("adapter-number")
}

fn boolean(value: &Value, field: &str) -> Result<bool, &'static str> {
    value[field].as_bool().ok_or("adapter-boolean")
}

fn step_urn(label: &str) -> String {
    format!("urn:libre-ai:step:{label}")
}

fn edge_urn(label: &str) -> String {
    format!("urn:libre-ai:edge:{label}")
}

fn outcome_identifier(label: &str) -> String {
    format!("outcome-{label}")
}

fn adapt_graph(
    vector: &VectorCase,
    fixtures: &Value,
    registry: &ContractRegistry,
) -> Result<&'static str, &'static str> {
    let reduced = &vector.input["graph"];
    let reduced_steps = reduced["steps"].as_array().ok_or("graph-steps")?;
    let mut steps = Vec::with_capacity(reduced_steps.len());
    let mut synthetic_edges = Vec::new();
    for (index, step) in reduced_steps.iter().enumerate() {
        let label = text(step, "stepId")?;
        let kind = text(step, "kind")?;
        let outcomes = step["outcomeCodes"].as_array().ok_or("graph-outcomes")?;
        if kind == "terminal" {
            steps.push(json!({
                "stepId": step_urn(label),
                "kind": "terminal",
                "outcomeCodes": []
            }));
        } else {
            let mut normalized_outcomes = outcomes
                .iter()
                .map(|outcome| {
                    outcome
                        .as_str()
                        .map(outcome_identifier)
                        .map(Value::String)
                        .ok_or("graph-outcome")
                })
                .collect::<Result<Vec<_>, _>>()?;
            if normalized_outcomes.is_empty() {
                normalized_outcomes.push(Value::String("synthetic-stuck".to_owned()));
                synthetic_edges.push(json!({
                    "edgeId": edge_urn(&format!("synthetic-stuck-{index}")),
                    "fromStepId": step_urn(label),
                    "outcomeCode": "synthetic-stuck",
                    "toStepId": step_urn(label)
                }));
            }
            steps.push(json!({
                "stepId": step_urn(label),
                "kind": kind,
                "outcomeCodes": normalized_outcomes,
                "retryPolicy": {
                    "maximumAttempts": 1,
                    "retryableOutcomeCodes": []
                }
            }));
        }
    }
    let mut edges = reduced["edges"]
        .as_array()
        .ok_or("graph-edges")?
        .iter()
        .map(|edge| {
            Ok(json!({
                "edgeId": edge_urn(text(edge, "edgeId")?),
                "fromStepId": step_urn(text(edge, "fromStepId")?),
                "outcomeCode": outcome_identifier(text(edge, "outcomeCode")?),
                "toStepId": step_urn(text(edge, "toStepId")?)
            }))
        })
        .collect::<Result<Vec<_>, &'static str>>()?;
    edges.extend(synthetic_edges);
    let mut document = fixture(fixtures, "execution-graph.v1.schema.json")?;
    document["entryStepId"] = Value::String(step_urn(text(reduced, "entryStepId")?));
    document["steps"] = Value::Array(steps);
    document["edges"] = Value::Array(edges);
    let graph = parse_authorized_graph(registry, &document).map_err(|_| "graph-boundary")?;
    Ok(evaluate_graph(&graph).code())
}

fn adapt_causal(
    vector: &VectorCase,
    registry: &ContractRegistry,
) -> Result<&'static str, &'static str> {
    let previous_facts = &vector.input["previous"];
    let current_facts = &vector.input["current"];
    let previous_sequence = number(previous_facts, "sequence")?;
    let mut previous_document = event_document(&EventFixture {
        event_type: "effect-terminal",
        sequence: previous_sequence,
        previous_event_digest: None,
        graph_digest: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        step_id: Some("urn:libre-ai:step:synthetic-effect-1"),
        attempt_id: Some("urn:libre-ai:attempt:synthetic-attempt-1"),
        worker_invocation_id: Some("urn:libre-ai:worker-invocation:synthetic-worker-1"),
        selected_edge_id: Some("urn:libre-ai:edge:synthetic-edge-1"),
        outcome_code: None,
        effect_status: Some("committed"),
        tool_calls_delta: 0,
        tool_calls_total: 0,
    });
    previous_document["generation"] = Value::from(number(previous_facts, "generation")?);
    previous_document["budgetDelta"] = previous_facts["budgetDelta"].clone();
    previous_document["budgetTotal"] = previous_facts["budgetTotal"].clone();
    reseal_event_document(&mut previous_document);
    let previous = parse_authorized_execution_event(registry, &previous_document)
        .map_err(|_| "causal-previous-boundary")?;

    let current_sequence = number(current_facts, "sequence")?;
    let predecessor_matches = current_facts["previousEventDigest"] == previous_facts["eventDigest"];
    let predecessor = predecessor_matches.then_some(previous.digest());
    let divergent_predecessor = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    let mut current_document = event_document(&EventFixture {
        event_type: "effect-terminal",
        sequence: current_sequence,
        previous_event_digest: predecessor
            .or_else(|| (current_sequence > 1).then_some(divergent_predecessor)),
        graph_digest: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        step_id: Some("urn:libre-ai:step:synthetic-effect-1"),
        attempt_id: Some("urn:libre-ai:attempt:synthetic-attempt-1"),
        worker_invocation_id: Some("urn:libre-ai:worker-invocation:synthetic-worker-1"),
        selected_edge_id: Some("urn:libre-ai:edge:synthetic-edge-1"),
        outcome_code: None,
        effect_status: Some("committed"),
        tool_calls_delta: 0,
        tool_calls_total: 0,
    });
    bind_causal_identity(&mut current_document, previous_facts, current_facts)?;
    current_document["generation"] = Value::from(number(current_facts, "generation")?);
    current_document["budgetDelta"] = current_facts["budgetDelta"].clone();
    current_document["budgetTotal"] = current_facts["budgetTotal"].clone();
    reseal_event_document(&mut current_document);
    let current = parse_authorized_execution_event(registry, &current_document)
        .map_err(|_| "causal-current-boundary")?;

    let collision = match vector.input.get("collision") {
        None | Some(Value::Null) => EventCollisionObservation::Absent,
        Some(raw) => EventCollisionObservation::Existing {
            event_id: if raw["id"] == current_facts["id"] {
                current.id()
            } else {
                "urn:libre-ai:event:other"
            },
            sequence: number(raw, "sequence")?,
            event_digest: if raw["eventDigest"] == current_facts["eventDigest"] {
                current.digest()
            } else {
                divergent_predecessor
            },
        },
    };
    Ok(evaluate_causal_transition(Some(&previous), &current, collision).code())
}

fn bind_causal_identity(
    document: &mut Value,
    previous: &Value,
    current: &Value,
) -> Result<(), &'static str> {
    let fields = [
        ("organizationId", "ten_abcdef1234567890"),
        ("missionId", "urn:libre-ai:mission:other"),
        ("runId", "urn:libre-ai:run:other"),
        ("orchestratorId", "orchestrator_other"),
        (
            "planDigest",
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        ),
        (
            "graphDigest",
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        ),
        (
            "authorizationDigest",
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        ),
    ];
    for (field, alternate) in fields {
        if text(previous, field)? != text(current, field)? {
            document[field] = Value::String(alternate.to_owned());
        }
    }
    Ok(())
}

fn adapt_decision(
    vector: &VectorCase,
    fixtures: &Value,
    registry: &ContractRegistry,
) -> Result<&'static str, &'static str> {
    let facts = &vector.input;
    let request_facts = &facts["request"];
    let response_facts = &facts["response"];
    let mut request = fixture(fixtures, "human-decision-request.v1.schema.json")?;
    let mut response = fixture(fixtures, "human-decision-response.v1.schema.json")?;

    request["requiredRole"] = Value::String(string(request_facts, "requiredRole")?);
    request["expectedRevision"] = Value::from(number(request_facts, "expectedRevision")?);
    request["expiresAt"] = Value::String(string(request_facts, "expiresAt")?);
    let choice_ids = request_facts["choiceIds"]
        .as_array()
        .ok_or("decision-choice-ids")?;
    request["choices"] = Value::Array(
        choice_ids
            .iter()
            .enumerate()
            .map(|(index, choice)| {
                let choice = choice.as_str().ok_or("decision-choice")?;
                Ok(json!({
                    "choiceId": choice,
                    "label": format!("Choice {}", index + 1),
                    "consequenceCode": format!("{choice}-outcome")
                }))
            })
            .collect::<Result<Vec<_>, &'static str>>()?,
    );
    response["choiceId"] = Value::String(string(response_facts, "choiceId")?);
    response["expectedRevision"] = Value::from(number(response_facts, "expectedRevision")?);
    response["actorAuthorization"]["role"] = request["requiredRole"].clone();
    response["requestDigest"] = request["requestDigest"].clone();

    if request_facts["organizationId"] != response_facts["organizationId"] {
        response["organizationId"] = Value::String("ten_abcdef1234567890".to_owned());
    }
    if request_facts["attemptId"] != response_facts["attemptId"] {
        response["attemptId"] = Value::String("urn:libre-ai:attempt:other".to_owned());
    }

    let role_storage = response_facts["actorRoles"]
        .as_array()
        .ok_or("decision-roles")?
        .iter()
        .map(|role| role.as_str().map(str::to_owned).ok_or("decision-role"))
        .collect::<Result<Vec<_>, _>>()?;
    let actor_roles = role_storage.iter().map(String::as_str).collect::<Vec<_>>();
    let prior_response = match facts.get("priorResponse") {
        None | Some(Value::Null) => None,
        Some(prior) => Some((
            if prior["id"] == response_facts["id"] {
                response["id"].as_str().ok_or("decision-response-id")?
            } else {
                "urn:libre-ai:decision-response:other"
            },
            if prior["responseDigest"] == response_facts["responseDigest"] {
                response["responseDigest"]
                    .as_str()
                    .ok_or("decision-response-digest")?
            } else {
                "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
            },
        )),
    };
    let observation = DecisionObservation::authoritative(
        boolean(facts, "replaced")?,
        boolean(facts, "consumed")?,
        &actor_roles,
        number(request_facts, "expectedRevision")?,
        prior_response,
    );
    Ok(evaluate_human_decision(
        registry,
        &request,
        &response,
        observation,
        text(facts, "now")?,
    )
    .code())
}

fn adapt_transfer(
    vector: &VectorCase,
    fixtures: &Value,
    registry: &ContractRegistry,
) -> Result<&'static str, &'static str> {
    let facts = &vector.input;
    let transfer_facts = &facts["transfer"];
    let state = &facts["state"];
    let mut transfer = fixture(fixtures, "execution-transfer.v1.schema.json")?;
    transfer["currentGeneration"] = Value::from(number(transfer_facts, "currentGeneration")?);
    transfer["expectedRevision"] = Value::from(number(transfer_facts, "expectedRevision")?);
    transfer["issuedAt"] = Value::String(string(transfer_facts, "issuedAt")?);
    transfer["expiresAt"] = Value::String(string(transfer_facts, "expiresAt")?);

    map_transfer_identity(
        &mut transfer,
        transfer_facts,
        state,
        "organizationId",
        "ten_1234567890abcdef",
        "ten_abcdef1234567890",
    )?;
    map_transfer_identity(
        &mut transfer,
        transfer_facts,
        state,
        "missionId",
        "urn:libre-ai:mission:synthetic-mission-1",
        "urn:libre-ai:mission:other",
    )?;
    map_transfer_identity(
        &mut transfer,
        transfer_facts,
        state,
        "predecessorRunId",
        "urn:libre-ai:run:synthetic-run-1",
        "urn:libre-ai:run:other",
    )?;
    map_transfer_identity(
        &mut transfer,
        transfer_facts,
        state,
        "predecessorPlanDigest",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    )?;
    map_transfer_identity(
        &mut transfer,
        transfer_facts,
        state,
        "successorPlanDigest",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    )?;

    let prior_transfer = match facts.get("collision") {
        None | Some(Value::Null) => None,
        Some(collision) => Some((
            if collision["id"] == transfer_facts["id"] {
                transfer["id"].as_str().ok_or("transfer-id")?
            } else {
                "urn:libre-ai:transfer:other"
            },
            number(collision, "currentGeneration")?,
            if collision["transferDigest"] == transfer_facts["transferDigest"] {
                transfer["transferDigest"]
                    .as_str()
                    .ok_or("transfer-digest")?
            } else {
                "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
            },
        )),
    };
    let observation = TransferObservation::Authoritative {
        organization_id: "ten_1234567890abcdef",
        mission_id: "urn:libre-ai:mission:synthetic-mission-1",
        predecessor_run_id: "urn:libre-ai:run:synthetic-run-1",
        predecessor_plan_digest: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        successor_plan_digest: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        current_generation: number(state, "currentGeneration")?,
        revision: number(state, "revision")?,
        generation_consumed: boolean(state, "generationConsumed")?,
        prior_transfer,
    };
    Ok(evaluate_execution_transfer(registry, &transfer, observation, text(facts, "now")?).code())
}

fn map_transfer_identity(
    document: &mut Value,
    transfer: &Value,
    state: &Value,
    field: &str,
    authoritative: &str,
    alternate: &str,
) -> Result<(), &'static str> {
    document[field] = Value::String(
        if text(transfer, field)? == text(state, field)? {
            authoritative
        } else {
            alternate
        }
        .to_owned(),
    );
    Ok(())
}

fn adapt_effect(
    vector: &VectorCase,
    fixtures: &Value,
    registry: &ContractRegistry,
) -> Result<&'static str, &'static str> {
    let facts = &vector.input;
    let attestation_facts = &facts["attestation"];
    let mut attestation = fixture(fixtures, "effect-attestation.v1.schema.json")?;
    let organization_matches =
        attestation_facts["organizationId"] == facts["expectedOrganizationId"];
    let run_matches = attestation_facts["runId"] == facts["expectedRunId"];
    let attempt_matches = attestation_facts["attemptId"] == facts["expectedAttemptId"];
    if !organization_matches {
        attestation["organizationId"] = Value::String("ten_abcdef1234567890".to_owned());
    }
    if !run_matches {
        attestation["runId"] = Value::String("urn:libre-ai:run:other".to_owned());
    }
    if !attempt_matches {
        attestation["attemptId"] = Value::String("urn:libre-ai:attempt:other".to_owned());
    }
    attestation["generation"] = Value::from(number(attestation_facts, "generation")?);
    attestation["fencingValue"] = attestation_facts["fencing"].clone();
    attestation["status"] = Value::String(string(attestation_facts, "status")?);

    let current_emission_id = attestation["effectEmissionId"]
        .as_str()
        .ok_or("effect-emission-id")?;
    let current_digest = attestation["preimageDigest"]
        .as_str()
        .ok_or("effect-emission-digest")?;
    let prior_emission = match facts.get("priorEmission") {
        None | Some(Value::Null) => None,
        Some(prior) => Some((
            if prior["effectEmissionId"] == attestation_facts["effectEmissionId"] {
                current_emission_id
            } else {
                "urn:libre-ai:emission:other"
            },
            if prior["emissionDigest"] == attestation_facts["emissionDigest"] {
                current_digest
            } else {
                "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
            },
        )),
    };
    let existing_attempt_emission_id = match facts.get("existingAttemptEmissionId") {
        None | Some(Value::Null) => None,
        Some(raw) if raw == &attestation_facts["effectEmissionId"] => Some(current_emission_id),
        Some(_) => Some("urn:libre-ai:emission:other"),
    };
    let active_fencing = facts["activeFencing"].as_u64();
    let observation = EffectObservation::Authoritative {
        expected_organization_id: "ten_1234567890abcdef",
        expected_run_id: "urn:libre-ai:run:synthetic-run-1",
        expected_attempt_id: "urn:libre-ai:attempt:synthetic-attempt-1",
        current_generation: number(facts, "currentGeneration")?,
        active_fencing,
        expected_executor_profile_digest: attestation["executorProfileRef"]["digest"]
            .as_str()
            .ok_or("effect-profile-digest")?,
        generation_consumed: boolean(facts, "generationConsumed")?,
        lineage_closed: boolean(facts, "lineageClosed")?,
        predecessor_effects_terminal: boolean(facts, "predecessorEffectsTerminal")?,
        executor_profile_qualified: boolean(facts, "executorProfileQualified")?,
        prior_emission,
        existing_attempt_emission_id,
    };
    Ok(evaluate_effect_attestation(registry, &attestation, observation).code())
}

fn adapt_authority(
    vector: &VectorCase,
    fixtures: &Value,
    registry: &ContractRegistry,
) -> Result<&'static str, &'static str> {
    let facts = &vector.input;
    let graph_facts = &facts["graph"];
    let plan_facts = &facts["plan"];
    let mut graph_document = fixture(fixtures, "execution-graph.v1.schema.json")?;
    let mut plan_document = fixture(fixtures, "execution-plan-body.v2.schema.json")?;

    let reduced_steps = graph_facts["steps"].as_array().ok_or("authority-steps")?;
    let invalid_retry = reduced_steps.iter().any(|step| {
        let Some(retryable) = step["retryPolicy"]["retryableOutcomeCodes"].as_array() else {
            return false;
        };
        let Some(outcomes) = step["outcomeCodes"].as_array() else {
            return false;
        };
        retryable.iter().any(|outcome| !outcomes.contains(outcome))
    });
    if invalid_retry {
        graph_document["steps"][0]["retryPolicy"]["retryableOutcomeCodes"] = json!(["unknown"]);
    }
    let duplicate_choice = reduced_steps.iter().any(|step| {
        let Some(choices) = step["decisionPolicy"]["choices"].as_array() else {
            return false;
        };
        let mut ids = BTreeSet::new();
        choices
            .iter()
            .filter_map(|choice| choice["choiceId"].as_str())
            .any(|id| !ids.insert(id))
    });
    if duplicate_choice {
        graph_document["steps"][1]["decisionPolicy"]["choices"][1]["choiceId"] =
            Value::String("approve".to_owned());
    }
    if graph_facts["graphDigest"] != plan_facts["executionGraph"]["digest"] {
        plan_document["executionGraph"]["digest"] = Value::String(
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_owned(),
        );
    }
    let graph = parse_authorized_graph(registry, &graph_document)
        .map_err(|_| "authority-graph-boundary")?;
    Ok(evaluate_graph_authority(&graph, &plan_document, registry).code())
}

#[test]
fn every_locked_semantic_vector_is_executed_by_rust() {
    let document: VectorDocument = serde_json::from_value(authority_document(SEMANTIC_VECTORS))
        .expect("semantic vector envelope");
    let fixtures = authority_document(SCHEMA_FIXTURES);
    let registry = ContractRegistry::embedded().expect("embedded registry");

    assert_eq!(
        document.schema_version,
        "libre-ai.authorized-execution-semantic-vectors.v1"
    );
    assert_eq!(document.cases.len(), 54);
    let mut inventory = BTreeMap::<&str, usize>::new();
    let mut ids = BTreeSet::new();
    for vector in &document.cases {
        *inventory.entry(vector.domain.as_str()).or_default() += 1;
        assert!(ids.insert(vector.id.as_str()), "duplicate vector id");
    }
    assert_eq!(
        inventory,
        BTreeMap::from([
            ("authority", 4),
            ("causal", 9),
            ("decision", 11),
            ("effect", 11),
            ("graph", 11),
            ("transfer", 8),
        ])
    );

    let mut executed = BTreeSet::new();
    for vector in &document.cases {
        let actual = adapt_and_evaluate(vector, &fixtures, &registry)
            .unwrap_or_else(|code| panic!("{} [{}]: {code}", vector.id, vector.domain));
        assert_eq!(actual, vector.expected, "{} [{}]", vector.id, vector.domain);
        assert!(executed.insert(vector.id.as_str()), "duplicate vector id");
    }
    assert_eq!(executed, ids);
}
