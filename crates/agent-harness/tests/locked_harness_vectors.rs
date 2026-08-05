use libre_ai_agent_harness::{attestation_digest, profile_digest, verify_attestation};
use libre_ai_contract_types::ContractRegistry;
use serde_json::Value;

// The locked digest vectors resolve from the pinned contracts git-dep, same
// idiom as tests/locked_event_vectors.rs at the workspace root: bun install
// materialises the authority under node_modules before cargo runs.
const DIGEST_VECTORS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../node_modules/@libre-ai/contracts-authority/contracts/fixtures/agent-orchestration-v1/digest-vectors.v1.json"
));

fn vector(id: &str) -> (Value, String) {
    let document: Value = serde_json::from_str(DIGEST_VECTORS).expect("locked vectors must parse");
    let entry = document["vectors"]
        .as_array()
        .expect("vectors must be an array")
        .iter()
        .find(|entry| entry["id"] == id)
        .expect("the locked vector must exist");
    (
        entry["unsignedPayload"].clone(),
        entry["expectedDigest"]
            .as_str()
            .expect("expectedDigest must be a string")
            .to_owned(),
    )
}

#[test]
fn profile_digest_reproduces_the_locked_harness_profile_vector() {
    let (payload, expected) = vector("harness-profile");
    let digest = profile_digest(&payload).expect("the locked payload must digest");
    assert_eq!(digest, expected);
}

const SIGNATURE_VECTORS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../node_modules/@libre-ai/contracts-authority/contracts/fixtures/agent-orchestration-v1/signature-vectors.v1.json"
));

#[test]
fn attestation_digest_reproduces_the_locked_harness_attestation_vector() {
    let (payload, expected) = vector("harness-attestation");
    let digest = attestation_digest(&payload).expect("the locked payload must digest");
    assert_eq!(digest, expected);
}

#[test]
fn the_locked_harness_attestation_signature_verifies_and_a_flipped_one_does_not() {
    let document: Value =
        serde_json::from_str(SIGNATURE_VECTORS).expect("locked signature vectors must parse");
    let entry = document["vectors"]
        .as_array()
        .expect("vectors must be an array")
        .iter()
        .find(|entry| entry["id"] == "harness-attestation")
        .expect("the harness-attestation signature vector must exist");
    let public_key = entry["publicKey"]
        .as_str()
        .expect("publicKey must be a string");
    let mut signed = entry["unsignedPayload"].clone();
    signed["attestationDigest"] = entry["expectedDigest"].clone();
    signed["signature"] = entry["signature"].clone();

    let registry = ContractRegistry::embedded().expect("embedded contracts must compile");
    verify_attestation(&registry, &signed, public_key)
        .expect("the locked signature must verify independently of any run");

    let mut tampered = signed.clone();
    let original = entry["signature"].as_str().expect("signature is a string");
    let flipped = if original.starts_with('A') {
        format!("B{}", &original[1..])
    } else {
        format!("A{}", &original[1..])
    };
    tampered["signature"] = Value::String(flipped);
    let refusal = verify_attestation(&registry, &tampered, public_key)
        .expect_err("a flipped signature must not verify");
    assert_eq!(refusal.code(), "harness.attestation_unsigned");
}
