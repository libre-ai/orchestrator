mod support;

use libre_ai_agent_orchestrator::{
    evaluate_graph, evaluate_graph_authority, parse_authorized_graph, select_graph_transition,
};
use libre_ai_contract_types::ContractRegistry;
use serde_json::{Value, json};
use support::authorized_execution::{valid_graph_document, valid_plan_document};

const ORGANIZATION_ID: &str = "ten_1234567890abcdef";
const GRAPH_ID: &str = "urn:libre-ai:graph:synthetic-graph";
const GRAPH_DIGEST: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn step_id(label: &str) -> String {
    format!("urn:libre-ai:step:{label}")
}

fn calculation_step(label: &str, outcomes: &[&str]) -> Value {
    json!({
        "stepId": step_id(label),
        "kind": "calculation",
        "outcomeCodes": outcomes,
        "retryPolicy": {
            "maximumAttempts": 1,
            "retryableOutcomeCodes": []
        }
    })
}

fn terminal_step(label: &str) -> Value {
    json!({
        "stepId": step_id(label),
        "kind": "terminal",
        "outcomeCodes": []
    })
}

fn edge(label: &str, from: &str, outcome: &str, to: &str) -> Value {
    json!({
        "edgeId": format!("urn:libre-ai:edge:{label}"),
        "fromStepId": step_id(from),
        "outcomeCode": outcome,
        "toStepId": step_id(to)
    })
}

fn graph_document(entry: &str, steps: Vec<Value>, edges: Vec<Value>) -> Value {
    json!({
        "schemaVersion": "libre-ai.execution-graph.v1",
        "id": GRAPH_ID,
        "organizationId": ORGANIZATION_ID,
        "entryStepId": step_id(entry),
        "steps": steps,
        "edges": edges,
        "createdAt": "2030-01-01T00:00:00Z",
        "graphDigest": GRAPH_DIGEST
    })
}

fn decision_code(registry: &ContractRegistry, document: &Value) -> &'static str {
    let graph = parse_authorized_graph(registry, document).expect("graph document must validate");
    evaluate_graph(&graph).code()
}

#[test]
fn rejected_wire_value_never_enters_the_error() {
    let registry = ContractRegistry::embedded().expect("embedded registry");
    let secret = "synthetic-private-prompt";
    let error = parse_authorized_graph(&registry, &json!({ "secret": secret }))
        .expect_err("invalid graph must close");
    assert_eq!(
        error.code(),
        "orchestrator.authorized-execution.schema-invalid"
    );
    assert!(!format!("{error:?}").contains(secret));
    assert!(!error.to_string().contains(secret));
}

#[test]
fn graph_refusals_follow_the_locked_precedence() {
    let registry = ContractRegistry::embedded().expect("embedded registry");

    let cases = [
        (
            "duplicate-step",
            graph_document(
                "a",
                vec![calculation_step("a", &["ready"]), terminal_step("a")],
                vec![],
            ),
        ),
        (
            "duplicate-edge",
            graph_document(
                "a",
                vec![calculation_step("a", &["ready"]), terminal_step("z")],
                vec![
                    edge("same", "a", "ready", "z"),
                    edge("same", "a", "ready", "z"),
                ],
            ),
        ),
        (
            "entry-missing",
            graph_document("missing", vec![terminal_step("z")], vec![]),
        ),
        (
            "dangling-edge",
            graph_document(
                "a",
                vec![calculation_step("a", &["ready"]), terminal_step("z")],
                vec![edge("dangling", "a", "ready", "missing")],
            ),
        ),
        (
            "terminal-has-outgoing-edge",
            graph_document(
                "a",
                vec![calculation_step("a", &["ready"]), terminal_step("z")],
                vec![
                    edge("finish", "a", "ready", "z"),
                    edge("escape", "z", "again", "a"),
                ],
            ),
        ),
        (
            "route-missing",
            graph_document(
                "a",
                vec![calculation_step("a", &["ready"]), terminal_step("z")],
                vec![],
            ),
        ),
        (
            "route-ambiguous",
            graph_document(
                "a",
                vec![calculation_step("a", &["ready"]), terminal_step("z")],
                vec![
                    edge("first", "a", "ready", "z"),
                    edge("second", "a", "ready", "z"),
                ],
            ),
        ),
        (
            "terminal-unreachable",
            graph_document(
                "a",
                vec![calculation_step("a", &["again"]), terminal_step("z")],
                vec![edge("loop", "a", "again", "a")],
            ),
        ),
        (
            "unreachable-step",
            graph_document(
                "a",
                vec![
                    calculation_step("a", &["ready"]),
                    calculation_step("orphan", &["ready"]),
                    terminal_step("z"),
                ],
                vec![
                    edge("finish", "a", "ready", "z"),
                    edge("orphan-finish", "orphan", "ready", "z"),
                ],
            ),
        ),
        (
            "cycle-forbidden",
            graph_document(
                "a",
                vec![
                    calculation_step("a", &["continue", "done"]),
                    calculation_step("b", &["again"]),
                    terminal_step("z"),
                ],
                vec![
                    edge("continue", "a", "continue", "b"),
                    edge("done", "a", "done", "z"),
                    edge("again", "b", "again", "a"),
                ],
            ),
        ),
        (
            "graph-valid",
            graph_document(
                "a",
                vec![calculation_step("a", &["ready"]), terminal_step("z")],
                vec![edge("finish", "a", "ready", "z")],
            ),
        ),
    ];

    for (expected, document) in cases {
        assert_eq!(decision_code(&registry, &document), expected, "{expected}");
    }
}

#[test]
fn duplicate_step_precedes_all_later_graph_refusals() {
    let registry = ContractRegistry::embedded().expect("embedded registry");
    let document = graph_document(
        "missing",
        vec![calculation_step("a", &["ready"]), terminal_step("a")],
        vec![],
    );
    assert_eq!(decision_code(&registry, &document), "duplicate-step");
}

#[test]
fn route_selection_is_closed_and_order_independent() {
    let registry = ContractRegistry::embedded().expect("embedded registry");
    let first_document = graph_document(
        "a",
        vec![
            calculation_step("a", &["ready", "fallback"]),
            terminal_step("y"),
            terminal_step("z"),
        ],
        vec![
            edge("fallback", "a", "fallback", "y"),
            edge("ready", "a", "ready", "z"),
        ],
    );
    let mut reversed_document = first_document.clone();
    reversed_document["edges"]
        .as_array_mut()
        .expect("edges")
        .reverse();
    let first = parse_authorized_graph(&registry, &first_document).expect("first graph");
    let reversed = parse_authorized_graph(&registry, &reversed_document).expect("reversed graph");

    assert_eq!(
        select_graph_transition(&first, &step_id("a"), "ready").target_step_id(),
        Some(step_id("z").as_str())
    );
    assert_eq!(
        select_graph_transition(&first, &step_id("a"), "ready"),
        select_graph_transition(&reversed, &step_id("a"), "ready")
    );
    assert_eq!(
        select_graph_transition(&first, &step_id("a"), "unknown").code(),
        "route-missing"
    );
}

#[test]
fn graph_authority_covers_every_locked_case() {
    let registry = ContractRegistry::embedded().expect("embedded registry");
    let graph_document = valid_graph_document();
    let graph = parse_authorized_graph(&registry, &graph_document).expect("valid graph fixture");
    let plan = valid_plan_document();
    assert_eq!(
        evaluate_graph_authority(&graph, &plan, &registry).code(),
        "authority-valid"
    );

    let mut invalid_retry = graph_document.clone();
    invalid_retry["steps"][0]["retryPolicy"]["retryableOutcomeCodes"] = json!(["unknown"]);
    let invalid_retry =
        parse_authorized_graph(&registry, &invalid_retry).expect("schema-valid retry mismatch");
    assert_eq!(
        evaluate_graph_authority(&invalid_retry, &plan, &registry).code(),
        "graph-policy-invalid"
    );

    let mut duplicate_choice = graph_document.clone();
    duplicate_choice["steps"][1]["decisionPolicy"]["choices"][1]["choiceId"] = json!("approve");
    let duplicate_choice = parse_authorized_graph(&registry, &duplicate_choice)
        .expect("schema-valid duplicate choice");
    assert_eq!(
        evaluate_graph_authority(&duplicate_choice, &plan, &registry).code(),
        "graph-policy-invalid"
    );

    let mut substituted_plan = plan;
    substituted_plan["executionGraph"]["digest"] =
        json!("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee");
    assert_eq!(
        evaluate_graph_authority(&graph, &substituted_plan, &registry).code(),
        "authority-binding-mismatch"
    );
}

#[test]
fn malformed_plan_is_a_redacted_boundary_refusal() {
    let registry = ContractRegistry::embedded().expect("embedded registry");
    let graph = parse_authorized_graph(&registry, &valid_graph_document()).expect("valid graph");
    let secret = "synthetic-private-plan";
    let decision = evaluate_graph_authority(&graph, &json!({ "secret": secret }), &registry);
    assert_eq!(
        decision.code(),
        "orchestrator.authorized-execution.schema-invalid"
    );
    assert!(!format!("{decision:?}").contains(secret));
}

#[test]
fn deterministic_graphs_up_to_four_nodes_are_order_independent() {
    let registry = ContractRegistry::embedded().expect("embedded registry");
    for node_count in 1_u32..=4 {
        let steps = (0..node_count)
            .map(|index| {
                if index + 1 == node_count {
                    terminal_step(&format!("n{index}"))
                } else {
                    calculation_step(&format!("n{index}"), &["ready"])
                }
            })
            .collect::<Vec<_>>();
        let candidates = (0..node_count)
            .flat_map(|from| (0..node_count).map(move |to| (from, to)))
            .collect::<Vec<_>>();

        for mask in 0_u32..(1_u32 << candidates.len()) {
            let edges = candidates
                .iter()
                .enumerate()
                .filter(|(index, _)| mask & (1_u32 << index) != 0)
                .map(|(_, (from, to))| {
                    edge(
                        &format!("n{from}-n{to}"),
                        &format!("n{from}"),
                        "ready",
                        &format!("n{to}"),
                    )
                })
                .collect::<Vec<_>>();
            let document = graph_document("n0", steps.clone(), edges);
            let graph = parse_authorized_graph(&registry, &document).expect("bounded graph schema");
            if evaluate_graph(&graph).code() != "graph-valid" {
                continue;
            }

            for index in 0..node_count.saturating_sub(1) {
                assert!(
                    select_graph_transition(&graph, &step_id(&format!("n{index}")), "ready")
                        .target_step_id()
                        .is_some()
                );
            }
            let mut reversed_document = document;
            reversed_document["edges"]
                .as_array_mut()
                .expect("edges")
                .reverse();
            let reversed =
                parse_authorized_graph(&registry, &reversed_document).expect("reversed graph");
            assert_eq!(evaluate_graph(&reversed).code(), "graph-valid");
            for index in 0..node_count.saturating_sub(1) {
                let current = step_id(&format!("n{index}"));
                assert_eq!(
                    select_graph_transition(&graph, &current, "ready"),
                    select_graph_transition(&reversed, &current, "ready")
                );
            }
        }
    }
}

#[test]
fn maximum_contract_graph_is_valid() {
    let registry = ContractRegistry::embedded().expect("embedded registry");
    let mut steps = Vec::with_capacity(256);
    let mut edges = Vec::with_capacity(512);
    for index in 0..255 {
        let extra_outcome = if index < 2 { Some("alternate-2") } else { None };
        let mut outcomes = vec!["continue", "alternate"];
        if let Some(outcome) = extra_outcome {
            outcomes.push(outcome);
        }
        steps.push(calculation_step(&format!("n{index}"), &outcomes));
        let next = if index == 254 {
            "n255".to_owned()
        } else {
            format!("n{}", index + 1)
        };
        edges.push(edge(
            &format!("continue-{index}"),
            &format!("n{index}"),
            "continue",
            &next,
        ));
        edges.push(edge(
            &format!("alternate-{index}"),
            &format!("n{index}"),
            "alternate",
            "n255",
        ));
        if extra_outcome.is_some() {
            edges.push(edge(
                &format!("alternate-2-{index}"),
                &format!("n{index}"),
                "alternate-2",
                "n255",
            ));
        }
    }
    steps.push(terminal_step("n255"));
    assert_eq!(steps.len(), 256);
    assert_eq!(edges.len(), 512);

    let document = graph_document("n0", steps, edges);
    assert_eq!(decision_code(&registry, &document), "graph-valid");
}
