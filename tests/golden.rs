use libre_ai_agent_orchestrator::{
    CanonicalReference, MissionBudgets, MissionContext, NetworkBudget, Scenario, SimulationRequest,
    encode_ndjson_v1, sha256_hex, simulate_v1,
};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

fn text<'a>(value: &'a Value, field: &str) -> &'a str {
    value[field].as_str().expect("string field")
}

fn reference(kind: &str, digest: char) -> CanonicalReference {
    let name = if kind == "evidence" { "report" } else { kind };
    CanonicalReference {
        id: format!("urn:libre-ai:{kind}:{name}-1"),
        digest: digest.to_string().repeat(64),
        media_type: "application/json".to_owned(),
    }
}

#[test]
fn rust_matches_every_shared_golden() {
    let metadata: Value =
        serde_json::from_slice(&fs::read(fixture("golden.v1.json")).unwrap()).expect("metadata");
    let handoff = fs::read(fixture("handoff.valid.json")).unwrap();
    for case in metadata["cases"].as_array().expect("cases") {
        let scenario = Scenario::parse(text(case, "scenario")).expect("scenario");
        let request = SimulationRequest {
            context: MissionContext {
                mission_id: "urn:libre-ai:mission:mission-1".to_owned(),
                started_at: "2026-07-16T00:00:00Z".parse().unwrap(),
                budgets: MissionBudgets {
                    max_duration_seconds: case["maxDurationSeconds"].as_u64().unwrap(),
                    max_tool_calls: 0,
                    network: NetworkBudget::None,
                },
            },
            scenario,
            artifact: (scenario == Scenario::Complete).then(|| reference("artifact", 'b')),
            evidence: (scenario == Scenario::Complete).then(|| reference("evidence", 'c')),
        };
        let events = simulate_v1(&handoff, &request).expect("simulation");
        let actual = encode_ndjson_v1(&events).expect("NDJSON");
        let expected = fs::read(fixture(text(case, "fixture"))).unwrap();
        let expected_types = case["types"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        let actual_types = events
            .iter()
            .map(|value| value["type"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "{} bytes", text(case, "name"));
        assert_eq!(actual_types, expected_types, "{} types", text(case, "name"));
        assert_eq!(
            sha256_hex(&actual),
            text(case, "streamSha256"),
            "{} checksum",
            text(case, "name")
        );
    }
}
