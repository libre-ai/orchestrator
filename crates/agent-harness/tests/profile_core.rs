use libre_ai_agent_harness::{HarnessRefusal, parse_profile, profile_digest, resolve_profile};
use libre_ai_contract_types::ContractRegistry;
use serde_json::Value;

const DIGEST_VECTORS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../node_modules/@libre-ai/contracts-authority/contracts/fixtures/agent-orchestration-v1/digest-vectors.v1.json"
));

const PROFILE_ID: &str = "urn:libre-ai:profile:harness-1";

/// The locked vector payload, completed with its own recomputed digest — the
/// only self-consistent full profile document the contracts publish.
fn canonical_profile() -> Value {
    let document: Value = serde_json::from_str(DIGEST_VECTORS).expect("locked vectors must parse");
    let mut payload = document["vectors"]
        .as_array()
        .expect("vectors must be an array")
        .iter()
        .find(|entry| entry["id"] == "harness-profile")
        .expect("the harness-profile vector must exist")["unsignedPayload"]
        .clone();
    let digest = profile_digest(&payload).expect("the locked payload must digest");
    payload["profileDigest"] = Value::String(digest);
    payload
}

fn registry() -> ContractRegistry {
    ContractRegistry::embedded().expect("embedded contracts must compile")
}

#[test]
fn parses_the_canonical_profile_and_exposes_its_identity() {
    let profile =
        parse_profile(&registry(), &canonical_profile()).expect("the canonical profile must parse");
    assert_eq!(profile.id(), PROFILE_ID);
    assert_eq!(profile.version(), "1.0.0");
    assert_eq!(profile.supported_platforms(), &["linux-x86_64".to_owned()]);
    assert_eq!(profile.max_bytes_per_tool(), 1_048_576);
    assert_eq!(profile.max_total_bytes(), 10_485_760);
}

#[test]
fn rejects_a_schema_violation_without_reflecting_the_value() {
    let mut document = canonical_profile();
    document["enforcement"] = Value::String("optional-must-not-appear".to_owned());
    let refusal =
        parse_profile(&registry(), &document).expect_err("a fail-open profile is refused");
    assert_eq!(refusal.code(), "harness.profile_unresolved");
    assert!(!format!("{refusal:?}").contains("must-not-appear"));
}

#[test]
fn rejects_an_embedded_digest_that_does_not_match_the_content() {
    let mut document = canonical_profile();
    document["profileDigest"] = Value::String("d".repeat(64));
    let refusal = parse_profile(&registry(), &document).expect_err("a lying digest is refused");
    assert_eq!(refusal.code(), "harness.profile_digest_mismatch");
}

#[test]
fn resolves_the_profile_the_caller_actually_requested() {
    let document = canonical_profile();
    let digest = profile_digest(&document).expect("the canonical profile must digest");

    let resolved = resolve_profile(&registry(), PROFILE_ID, &digest, &document)
        .expect("the requested profile must resolve");
    assert_eq!(resolved.digest(), digest);

    let wrong_id = resolve_profile(
        &registry(),
        "urn:libre-ai:profile:someone-else",
        &digest,
        &document,
    )
    .expect_err("another profile id must not resolve to this content");
    assert_eq!(wrong_id.code(), "harness.profile_unresolved");

    let wrong_digest = resolve_profile(&registry(), PROFILE_ID, &"e".repeat(64), &document)
        .expect_err("content that does not hash to the requested digest is refused");
    assert_eq!(wrong_digest.code(), "harness.profile_digest_mismatch");
}

#[test]
fn every_matrix_code_is_a_stable_dotted_identifier() {
    for refusal in HarnessRefusal::ALL {
        let code = refusal.code();
        assert!(
            code.starts_with("harness.") && code.len() > "harness.".len(),
            "{code} must carry the harness prefix"
        );
    }
    assert_eq!(HarnessRefusal::ALL.len(), 13);
}
