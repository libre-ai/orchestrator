use libre_ai_agent_harness::{
    HarnessRefusal, RunError, RunIdentity, profile_digest, public_key_base64url,
    run_confined_attested, verify_attestation,
};
use libre_ai_contract_types::ContractRegistry;
use serde_json::Value;
use std::path::Path;
use std::process::Command;

const CANONICAL_PROFILE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/profiles/local-process.v1.json"
));
const PROFILE_ID: &str = "urn:libre-ai:profile:local-process-1";
const SIGNING_SEED: [u8; 32] = [11; 32];

fn run_identity() -> RunIdentity {
    RunIdentity {
        attestation_id: "urn:libre-ai:attestation:harness-run-e2e-1".to_owned(),
        tenant_id: "ten_1234567890abcdef".to_owned(),
        mission_id: "urn:libre-ai:mission:bootstrap-e2e".to_owned(),
        run_id: "urn:libre-ai:run:bootstrap-e2e-1".to_owned(),
        plan_digest: "a".repeat(64),
        signing_key_id: "harness_bootstrap_key_1".to_owned(),
    }
}

/// The dedicated worker identity is arranged by CI (useradd) and looked up
/// here; the harness resolves its own tools, the test only nominates the id.
fn dedicated_identity() -> Option<(u32, u32)> {
    let uid = Command::new("/usr/bin/id")
        .args(["-u", "harness-worker"])
        .output()
        .ok()
        .filter(|probe| probe.status.success())
        .and_then(|probe| {
            String::from_utf8_lossy(&probe.stdout)
                .trim()
                .parse::<u32>()
                .ok()
        })?;
    Some((uid, uid))
}

#[test]
fn the_first_confined_execution_is_attested_or_exactly_refused() {
    let registry = ContractRegistry::embedded().expect("embedded contracts must compile");
    let document: Value =
        serde_json::from_str(CANONICAL_PROFILE).expect("the canonical profile must parse");
    let digest = profile_digest(&document).expect("the canonical profile must digest");

    let workspace = std::env::temp_dir().join(format!("harness-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(workspace.join("out")).expect("the workspace must build");

    let identity = dedicated_identity();
    let result = run_confined_attested(
        &registry,
        &document,
        PROFILE_ID,
        &digest,
        &workspace,
        Path::new("/bin/cat"),
        &[],
        b"bootstrap-payload",
        identity.map(|(uid, _)| uid),
        identity.map(|(_, gid)| gid),
        &run_identity(),
        &SIGNING_SEED,
        "2026-08-05T12:00:00Z",
    );

    if cfg!(target_os = "linux") {
        if identity.is_some() {
            // The bootstrap path: a real confined run, attested and
            // independently verifiable (ADR-0018 D2).
            let attestation = result.expect("the fully equipped host attests the run");
            let public_key = public_key_base64url(&SIGNING_SEED);
            verify_attestation(&registry, &attestation, &public_key)
                .expect("the attestation verifies without the run that produced it");
            // Compared to what this build targets, never to the architecture
            // the CI runner happens to be (K4 architecture verdict, minor 3).
            let expected = if cfg!(target_arch = "x86_64") {
                "linux-x86_64"
            } else {
                "linux-aarch64"
            };
            assert_eq!(attestation["platform"], expected);
            assert_eq!(attestation["networkMode"], "none");
            // The attestation states a narrower effective profile than the
            // one requested, because this engine applies less than the
            // locked contract prescribes.
            assert_ne!(
                attestation["requestedProfileDigest"], attestation["effectiveProfileDigest"],
                "an unapplied block must not travel as effective"
            );
            assert_eq!(
                attestation["effectiveProfileDigest"],
                libre_ai_agent_harness::effective_profile_digest(&document)
                    .expect("the applied surface must digest")
            );
            // The ledger is exact: what the engine applies, and nothing more.
            assert_eq!(
                attestation["effectiveControls"],
                serde_json::json!([
                    "output_bounds",
                    "process_isolation",
                    "resource_limits",
                    "worker_transport_isolation"
                ])
            );
        } else {
            // Linux without the arranged identity: refused, never degraded.
            assert_eq!(
                result.expect_err("missing privileges must refuse"),
                RunError::Refused(HarnessRefusal::ControlNotEnforceable)
            );
        }
    } else {
        // The canonical increment profile names Linux only: every other
        // platform is refused before anything starts.
        assert_eq!(
            result.expect_err("a platform outside the profile must refuse"),
            RunError::Refused(HarnessRefusal::PlatformUnsupported)
        );
    }

    let _ = std::fs::remove_dir_all(&workspace);
}
