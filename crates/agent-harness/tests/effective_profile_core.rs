use libre_ai_agent_harness::{APPLIED_PROFILE_SURFACE, effective_profile_digest, profile_digest};
use serde_json::Value;

const CANONICAL_PROFILE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/profiles/local-process.v1.json"
));

fn canonical_document() -> Value {
    serde_json::from_str(CANONICAL_PROFILE).expect("the canonical profile fixture must parse")
}

/// The two digests are distinct fields on purpose: a confinement that applies
/// less than the profile prescribes must stay distinguishable from one that
/// honoured it. Echoing the requested digest made them identical by
/// construction (K4 rounds 1 and 2 on 5bee6a3 and f27b3c9).
#[test]
fn the_effective_digest_differs_from_the_requested_one_when_blocks_are_unapplied() {
    let document = canonical_document();
    let requested = profile_digest(&document).expect("the canonical profile must digest");
    let effective = effective_profile_digest(&document).expect("the applied surface must digest");
    assert_ne!(
        requested, effective,
        "this engine applies less than the profile prescribes; the digests must say so"
    );
}

/// An operator holding the requested profile recomputes the effective digest
/// by keeping exactly the documented surface — the projection is a rule, not
/// a secret of the harness.
#[test]
fn the_projection_is_reproducible_from_the_requested_profile_alone() {
    let document = canonical_document();
    let mut projected = Value::Object(serde_json::Map::new());
    for pointer in APPLIED_PROFILE_SURFACE {
        let value = document
            .pointer(pointer)
            .expect("the surface names a present field");
        let segments: Vec<&str> = pointer.trim_start_matches('/').split('/').collect();
        let (leaf, parents) = segments.split_last().expect("a pointer has a leaf");
        let mut cursor = &mut projected;
        for segment in parents {
            cursor = cursor
                .as_object_mut()
                .expect("an object")
                .entry((*segment).to_owned())
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
        }
        cursor
            .as_object_mut()
            .expect("an object")
            .insert((*leaf).to_owned(), value.clone());
    }
    let recomputed = profile_digest(&projected).expect("the projection must digest");
    assert_eq!(
        recomputed,
        effective_profile_digest(&document).expect("the applied surface must digest")
    );
}

/// The surface is what the engine applies, and nothing else: a block it does
/// not act on must not travel into the effective digest, or the attestation
/// asserts it again through the back door.
#[test]
fn the_unapplied_blocks_do_not_move_the_effective_digest() {
    let mut widened = canonical_document();
    widened["operationalLogs"]["maxRetentionHours"] = serde_json::json!(1);
    widened["enforcement"] = serde_json::json!("required");
    widened["outputs"]["privateByDefault"] = serde_json::json!(true);
    widened["providerGateway"]["bindExactOrigins"] = serde_json::json!(true);
    widened["filesystem"]["denied"] = serde_json::json!([".env", "**/.git/**", "secrets/**"]);

    assert_eq!(
        effective_profile_digest(&canonical_document()).expect("digest"),
        effective_profile_digest(&widened).expect("digest"),
        "changing a block the engine never applies cannot change what it attests as effective"
    );
}

/// Conversely, a change to something the engine DOES apply must move it.
#[test]
fn a_change_to_the_applied_surface_moves_the_effective_digest() {
    let mut narrowed = canonical_document();
    narrowed["process"]["maxProcesses"] = serde_json::json!(4);
    assert_ne!(
        effective_profile_digest(&canonical_document()).expect("digest"),
        effective_profile_digest(&narrowed).expect("digest")
    );
}
