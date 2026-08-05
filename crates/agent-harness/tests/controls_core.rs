use libre_ai_agent_harness::{HostFacts, parse_profile, profile_digest, resolve_controls};
use libre_ai_contract_types::ContractRegistry;
use serde_json::Value;

const CANONICAL_PROFILE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/profiles/local-process.v1.json"
));

const DIGEST_VECTORS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../node_modules/@libre-ai/contracts-authority/contracts/fixtures/agent-orchestration-v1/digest-vectors.v1.json"
));

fn registry() -> ContractRegistry {
    ContractRegistry::embedded().expect("embedded contracts must compile")
}

fn canonical_document() -> Value {
    serde_json::from_str(CANONICAL_PROFILE).expect("the canonical profile fixture must parse")
}

/// Mutate a profile document and restore its self-consistency, so tests
/// exercise the intended refusal instead of tripping the digest check.
fn with_digest_recomputed(mut document: Value) -> Value {
    let digest = profile_digest(&document).expect("the mutated document must digest");
    document["profileDigest"] = Value::String(digest);
    document
}

fn full_facts() -> HostFacts {
    HostFacts::new("linux-x86_64", true, true)
}

#[test]
fn the_canonical_increment_profile_is_self_consistent() {
    let document = canonical_document();
    let computed = profile_digest(&document).expect("the canonical profile must digest");
    assert_eq!(
        Some(computed.as_str()),
        document["profileDigest"].as_str(),
        "the versioned fixture must carry its own content address"
    );
    parse_profile(&registry(), &document).expect("the canonical profile must parse");
}

#[test]
fn resolves_every_prescribed_control_on_a_fully_equipped_linux_host() {
    let profile = parse_profile(&registry(), &canonical_document())
        .expect("the canonical profile must parse");
    let effective =
        resolve_controls(&profile, &full_facts()).expect("every prescribed control is enforceable");
    assert_eq!(
        effective.identifiers(),
        &[
            "output_bounds".to_owned(),
            "process_isolation".to_owned(),
            "resource_limits".to_owned(),
            "worker_transport_isolation".to_owned(),
        ],
        "the ledger lists what is enforced, and filesystem_confinement is not"
    );
}

#[test]
fn a_platform_outside_the_profile_is_refused_before_anything_starts() {
    let profile = parse_profile(&registry(), &canonical_document())
        .expect("the canonical profile must parse");
    let refusal = resolve_controls(&profile, &HostFacts::new("macos-aarch64", true, true))
        .expect_err("macOS is not in the canonical profile");
    assert_eq!(refusal.code(), "harness.platform_unsupported");
}

#[test]
fn missing_host_privileges_refuse_rather_than_degrade() {
    let profile = parse_profile(&registry(), &canonical_document())
        .expect("the canonical profile must parse");

    let unprivileged = resolve_controls(&profile, &HostFacts::new("linux-x86_64", false, true))
        .expect_err("process isolation without root is a refusal, never best effort");
    assert_eq!(unprivileged.code(), "harness.control_not_enforceable");

    let no_setpriv = resolve_controls(&profile, &HostFacts::new("linux-x86_64", true, false))
        .expect_err("resource limits without setpriv are a refusal, never best effort");
    assert_eq!(no_setpriv.code(), "harness.control_not_enforceable");
}

/// The engine cannot bound a worker's own syscalls without chroot or a mount
/// namespace, so it does not offer filesystem_confinement: a profile that
/// requires it is refused, never attested with a boundary that is absent
/// (K4 verdicts on 5bee6a3, blocking finding 1).
#[test]
fn a_profile_requiring_filesystem_confinement_is_refused() {
    let mut document = canonical_document();
    document["sandboxEngine"]["requiredCapabilities"] = serde_json::json!([
        "filesystem_confinement",
        "output_bounds",
        "process_isolation",
        "resource_limits"
    ]);
    let profile = parse_profile(&registry(), &with_digest_recomputed(document))
        .expect("the mutated profile still satisfies the locked schema");
    let refusal = resolve_controls(&profile, &full_facts())
        .expect_err("a boundary this engine cannot apply is refused");
    assert_eq!(refusal.code(), "harness.control_not_enforceable");
}

#[test]
fn a_capability_the_engine_does_not_provide_is_refused_deny_on_missing() {
    let vectors: Value = serde_json::from_str(DIGEST_VECTORS).expect("locked vectors must parse");
    let payload = vectors["vectors"]
        .as_array()
        .expect("vectors must be an array")
        .iter()
        .find(|entry| entry["id"] == "harness-profile")
        .expect("the harness-profile vector must exist")["unsignedPayload"]
        .clone();
    // The locked vector profile requires `filesystem_mounts`, which the local
    // process engine deliberately does not provide at this stage.
    let profile = parse_profile(&registry(), &with_digest_recomputed(payload))
        .expect("the locked vector profile must parse");
    let refusal = resolve_controls(&profile, &HostFacts::new("linux-x86_64", true, true))
        .expect_err("a missing engine capability denies the run");
    assert_eq!(refusal.code(), "harness.control_not_enforceable");
}

#[test]
fn a_closed_transport_capability_is_refused_as_not_enabled() {
    let mut document = canonical_document();
    document["workerTransport"]["kind"] = Value::String("private-network-namespace".to_owned());
    let profile = parse_profile(&registry(), &with_digest_recomputed(document))
        .expect("the mutated profile still satisfies the locked schema");
    let refusal = resolve_controls(&profile, &full_facts())
        .expect_err("network namespaces are closed at this stage (ADR-0018 D2)");
    assert_eq!(refusal.code(), "harness.capability_not_enabled");
}
