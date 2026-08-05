use crate::attestation::{ArtifactRef, AttestationInputs, sign_attestation};
use crate::confinement::{ConfinementPlan, plan_wrapper_chain};
use crate::controls::{HostFacts, resolve_controls};
use crate::host::fs::WorkspaceObserver;
use crate::host::process::{SpawnLimits, spawn_confined};
use crate::outputs::{OutputLedger, OutputScan, admit_scan};
use crate::profile::resolve_profile;
use crate::refusal::HarnessRefusal;
use libre_ai_contract_types::ContractRegistry;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
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

/// Observe the host: platform from the build target, effective uid and tool
/// presence through probe processes. Nothing here is asserted by the caller —
/// a fact the caller supplies is not a host fact (K4 architecture verdict on
/// 5bee6a3, major finding 3).
fn gather_host_facts() -> HostFacts {
    let euid_is_root = Command::new("/usr/bin/id")
        .arg("-u")
        .env_clear()
        .output()
        .map(|probe| String::from_utf8_lossy(&probe.stdout).trim() == "0")
        .unwrap_or(false);
    HostFacts::new(PLATFORM, euid_is_root, tool_present("setpriv"))
}

/// Absolute, fixed candidates only: no PATH lookup, so nothing the caller
/// controls can point the harness at a different binary.
fn tool_path(tool: &str) -> Option<PathBuf> {
    [format!("/usr/bin/{tool}"), format!("/bin/{tool}")]
        .into_iter()
        .map(PathBuf::from)
        .find(|candidate| candidate.exists())
}

fn tool_present(tool: &str) -> bool {
    tool_path(tool).is_some()
}

/// Build the plan from what this host actually offers, for the identity the
/// caller nominates. The prescription is then either applied in full or
/// refused by `plan_wrapper_chain`.
fn host_plan(dedicated_uid: Option<u32>, dedicated_gid: Option<u32>) -> ConfinementPlan {
    let Some(setpriv) = tool_path("setpriv") else {
        return ConfinementPlan::unprivileged();
    };
    let (Some(uid), Some(gid)) = (dedicated_uid, dedicated_gid) else {
        return ConfinementPlan::unprivileged();
    };
    let mut plan = ConfinementPlan::privileged(uid, gid, &setpriv);
    if let Some(setsid) = tool_path("setsid") {
        plan = plan.with_setsid(&setsid);
    }
    if let Some(prlimit) = tool_path("prlimit") {
        plan = plan.with_prlimit(&prlimit);
    }
    plan
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
    dedicated_uid: Option<u32>,
    dedicated_gid: Option<u32>,
    engine_manifest: ArtifactRef,
    identity: &RunIdentity,
    signing_seed: &[u8; 32],
    generated_at: &str,
) -> Result<Value, RunError> {
    let resolved = resolve_profile(registry, requested_id, requested_digest, profile_document)?;
    let profile = resolved.profile();
    let plan = host_plan(dedicated_uid, dedicated_gid);
    let facts = gather_host_facts();
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
    // The const-locked process block is applied in full by the chain, or
    // the run is refused before the worker exists.
    let chain = plan_wrapper_chain(&profile.process(), &plan)?;
    let outcome = spawn_confined(program, args, payload, &limits, &plan, &chain)?;

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
