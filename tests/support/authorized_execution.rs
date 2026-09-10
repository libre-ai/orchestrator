use std::fs;
use std::path::PathBuf;

use serde_json::Value;

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

pub fn valid_plan_document() -> Value {
    schema_fixture("execution-plan-body.v2.schema.json")
}
