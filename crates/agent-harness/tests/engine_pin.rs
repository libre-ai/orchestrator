use libre_ai_agent_harness::{engine_manifest_digest, profile_digest};
use serde_json::Value;

const CANONICAL_PROFILE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/profiles/local-process.v1.json"
));

fn canonical_document() -> Value {
    serde_json::from_str(CANONICAL_PROFILE).expect("the canonical profile fixture must parse")
}

/// The chain manifest content → profile pin → profile address is maintained
/// by hand across two files. It has now drifted twice, each time surfacing as
/// a runtime refusal on CI rather than as a red test (round 4 architecture
/// verdict, major finding on the ungated cascade). This is that gate.
#[test]
fn the_profile_pins_the_engine_manifest_this_build_carries() {
    let document = canonical_document();
    assert_eq!(
        document["sandboxEngine"]["manifest"]["digest"].as_str(),
        Some(engine_manifest_digest().as_str()),
        "the profile's engine pin must address the manifest embedded in this build"
    );
}

#[test]
fn the_profile_carries_its_own_content_address() {
    let document = canonical_document();
    let recomputed = profile_digest(&document).expect("the canonical profile must digest");
    assert_eq!(
        document["profileDigest"].as_str(),
        Some(recomputed.as_str()),
        "a change to the profile must carry its recomputed address"
    );
}

/// The engine's capability list exists in two places: the Rust constant the
/// resolver consults and the manifest a consumer resolves from the attested
/// digest. They disagreed once, and the manifest is the one an operator
/// reads.
#[test]
fn the_manifest_advertises_exactly_what_the_engine_offers() {
    let manifest: Value = serde_json::from_str(libre_ai_agent_harness::ENGINE_MANIFEST)
        .expect("the embedded manifest must parse");
    let advertised: Vec<&str> = manifest["capabilities"]
        .as_array()
        .expect("capabilities is an array")
        .iter()
        .map(|value| value.as_str().expect("a capability is a string"))
        .collect();
    assert_eq!(
        advertised,
        libre_ai_agent_harness::engine_capabilities(),
        "the published self-description must match what the resolver offers"
    );
}
