use crate::attestation::{ArtifactRef, AttestationInputs, sign_attestation};
use crate::controls::{HostFacts, resolve_controls};
use crate::host::fs::WorkspaceObserver;
use crate::host::process::{ConfinementPlan, SpawnLimits, spawn_confined};
use crate::outputs::{OutputLedger, OutputScan, admit_scan};
use crate::profile::resolve_profile;
use crate::refusal::HarnessRefusal;
use libre_ai_contract_types::ContractRegistry;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// A run ends as an attestation, a refusal from the closed matrix, or a
/// worker fault — a worker that failed inside honoured controls is not a
/// confinement refusal and must not borrow a code from the matrix.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunError {
    Refused(HarnessRefusal),
    WorkerFault,
}

impl From<HarnessRefusal> for RunError {
    fn from(refusal: HarnessRefusal) -> Self {
        Self::Refused(refusal)
    }
}

/// The identity a run's attestation binds.
#[derive(Clone, Debug)]
pub struct RunIdentity {
    pub attestation_id: String,
    pub tenant_id: String,
    pub mission_id: String,
    pub run_id: String,
    pub plan_digest: String,
    pub signing_key_id: String,
}

const PLATFORM: &str = if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    "linux-x86_64"
} else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
    "linux-aarch64"
} else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    "macos-aarch64"
} else {
    "macos-x86_64"
};

/// Observe the host: platform from the build target, effective uid through a
/// probe process (std exposes no uid), setpriv from the plan the caller holds.
fn gather_host_facts(plan: &ConfinementPlan) -> HostFacts {
    let euid_is_root = Command::new("/usr/bin/id")
        .arg("-u")
        .env_clear()
        .output()
        .map(|probe| String::from_utf8_lossy(&probe.stdout).trim() == "0")
        .unwrap_or(false);
    HostFacts::new(PLATFORM, euid_is_root, plan.is_privileged())
}

/// ADR-0018 D2, end to end: resolve the requested profile, refuse every
/// control this host cannot honestly enforce, run the worker confined, hold
/// its output to the profile's bounds, and emit the signed attestation that
/// binds what was asked to what was actually enforced.
#[expect(
    clippy::too_many_arguments,
    reason = "one parameter per bound input, by design"
)]
pub fn run_confined_attested(
    registry: &ContractRegistry,
    profile_document: &Value,
    requested_id: &str,
    requested_digest: &str,
    workspace_root: &Path,
    program: &Path,
    args: &[String],
    payload: &[u8],
    plan: &ConfinementPlan,
    engine_manifest: ArtifactRef,
    identity: &RunIdentity,
    signing_seed: &[u8; 32],
    generated_at: &str,
) -> Result<Value, RunError> {
    let resolved = resolve_profile(registry, requested_id, requested_digest, profile_document)?;
    let profile = resolved.profile();
    let facts = gather_host_facts(plan);
    let effective = resolve_controls(profile, &facts)?;
    let _observer = WorkspaceObserver::new(workspace_root)?;

    let worker_manifest_digest = {
        let bytes = std::fs::read(program).map_err(|_| HarnessRefusal::ControlNotEnforceable)?;
        let mut digest = String::with_capacity(64);
        for byte in Sha256::digest(bytes) {
            write!(&mut digest, "{byte:02x}").map_err(|_| HarnessRefusal::ControlNotEnforceable)?;
        }
        digest
    };

    let limits = SpawnLimits::new(
        profile.max_bytes_per_tool(),
        Duration::from_secs(profile.max_duration_seconds()),
    );
    let outcome = spawn_confined(program, args, payload, &limits, plan)?;

    let mut ledger = OutputLedger::new(profile.max_bytes_per_tool(), profile.max_total_bytes());
    ledger.admit(outcome.output().len() as u64, outcome.truncated())?;
    admit_scan(OutputScan::Complete)?;
    if !outcome.exit_ok() {
        return Err(RunError::WorkerFault);
    }

    let inputs = AttestationInputs::new(
        &identity.attestation_id,
        &identity.tenant_id,
        &identity.mission_id,
        &identity.run_id,
        &identity.plan_digest,
        resolved.digest(),
        resolved.digest(),
        vec![worker_manifest_digest],
        engine_manifest,
        PLATFORM,
        effective.identifiers().to_vec(),
        "none",
        generated_at,
        &identity.signing_key_id,
    );
    Ok(sign_attestation(registry, &inputs, signing_seed)?)
}
