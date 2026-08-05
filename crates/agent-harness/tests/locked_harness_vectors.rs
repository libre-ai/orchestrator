use libre_ai_agent_harness::profile_digest;
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
