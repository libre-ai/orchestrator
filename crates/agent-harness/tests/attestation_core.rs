use libre_ai_agent_harness::{
    ArtifactRef, AttestationInputs, public_key_base64url, sign_attestation, verify_attestation,
};
use libre_ai_contract_types::ContractRegistry;

const SIGNING_SEED: [u8; 32] = [7; 32];
const TENANT: &str = "ten_1234567890abcdef";

fn registry() -> ContractRegistry {
    ContractRegistry::embedded().expect("embedded contracts must compile")
}

fn inputs_with(worker_manifests: Vec<String>, controls: Vec<String>) -> AttestationInputs {
    AttestationInputs::new(
        "urn:libre-ai:attestation:harness-run-test",
        TENANT,
        "urn:libre-ai:mission:mission-test",
        "urn:libre-ai:run:run-test",
        &"a".repeat(64),
        &"b".repeat(64),
        &"b".repeat(64),
        worker_manifests,
        ArtifactRef::new(
            "urn:libre-ai:manifest:agent-harness-host-engine-1",
            &"d".repeat(64),
            "application/json",
        ),
        "linux-x86_64",
        controls,
        "none",
        "2026-08-05T10:00:00Z",
        "harness_test_key_1",
    )
}

fn inputs() -> AttestationInputs {
    inputs_with(
        vec!["c".repeat(64)],
        vec![
            "output_bounds".to_owned(),
            "process_isolation".to_owned(),
            "resource_limits".to_owned(),
        ],
    )
}

#[test]
fn a_signed_attestation_verifies_independently_of_the_run_that_produced_it() {
    let signed =
        sign_attestation(&registry(), &inputs(), &SIGNING_SEED).expect("a complete binding signs");
    let public_key = public_key_base64url(&SIGNING_SEED);
    verify_attestation(&registry(), &signed, &public_key)
        .expect("the emitted attestation must verify with digest and public key alone");

    assert_eq!(signed["networkMode"], "none");
    assert_eq!(
        signed["requestedProfileDigest"],
        signed["effectiveProfileDigest"]
    );
}

#[test]
fn a_wrong_public_key_refuses_the_signature() {
    let signed =
        sign_attestation(&registry(), &inputs(), &SIGNING_SEED).expect("a complete binding signs");
    let other = public_key_base64url(&[9; 32]);
    let refusal = verify_attestation(&registry(), &signed, &other)
        .expect_err("someone else's key must not verify this attestation");
    assert_eq!(refusal.code(), "harness.attestation_unsigned");
}

#[test]
fn content_tampered_after_signing_is_refused_as_unsigned() {
    let mut signed =
        sign_attestation(&registry(), &inputs(), &SIGNING_SEED).expect("a complete binding signs");
    signed["platform"] = serde_json::Value::String("linux-aarch64".to_owned());
    let public_key = public_key_base64url(&SIGNING_SEED);
    let refusal = verify_attestation(&registry(), &signed, &public_key)
        .expect_err("the signature no longer attests the tampered content");
    assert_eq!(refusal.code(), "harness.attestation_unsigned");
}

#[test]
fn an_attestation_that_binds_less_than_it_claims_is_invalid_not_partial() {
    let no_manifests = inputs_with(vec![], vec!["output_bounds".to_owned()]);
    let refusal = sign_attestation(&registry(), &no_manifests, &SIGNING_SEED)
        .expect_err("an empty worker manifest binding must not sign");
    assert_eq!(refusal.code(), "harness.attestation_binding_incomplete");

    let no_controls = inputs_with(vec!["c".repeat(64)], vec![]);
    let refusal = sign_attestation(&registry(), &no_controls, &SIGNING_SEED)
        .expect_err("an empty effective-controls binding must not sign");
    assert_eq!(refusal.code(), "harness.attestation_binding_incomplete");
}

/// The exactness the narrowed capability list bought inside the run must hold
/// at the API that produces the signature, or a caller signs a claim the
/// engine has no mechanism for (xhigh review of f27b3c9).
#[test]
fn a_control_the_engine_does_not_offer_is_never_signed() {
    let claiming = inputs_with(
        vec!["c".repeat(64)],
        vec!["filesystem_confinement".to_owned()],
    );
    let refusal = sign_attestation(&registry(), &claiming, &SIGNING_SEED)
        .expect_err("the engine offers no filesystem_confinement mechanism");
    assert_eq!(refusal.code(), "harness.attestation_binding_incomplete");
}

/// A malformed verification key is the operator's input being wrong, not the
/// attestation being forged: the two must not share a verdict.
#[test]
fn a_malformed_verifying_key_is_not_a_forged_attestation() {
    let signed =
        sign_attestation(&registry(), &inputs(), &SIGNING_SEED).expect("a complete binding signs");
    let padded = format!("{}=", public_key_base64url(&SIGNING_SEED));
    let error = verify_attestation(&registry(), &signed, &padded)
        .expect_err("a padded key does not decode");
    assert!(
        error.is_malformed_key(),
        "a key-encoding mistake must not read as an unsigned attestation"
    );

    let other = public_key_base64url(&[9; 32]);
    let refused = verify_attestation(&registry(), &signed, &other)
        .expect_err("someone else's key must not verify this attestation");
    assert!(!refused.is_malformed_key());
    assert_eq!(refused.code(), "harness.attestation_unsigned");
}
