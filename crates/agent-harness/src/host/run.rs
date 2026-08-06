use crate::attestation::{ArtifactRef, AttestationInputs, sign_attestation};
use crate::canonical::sha256_hex;
use crate::confinement::{ConfinementPlan, plan_wrapper_chain};
use crate::controls::{HostFacts, resolve_controls};
use crate::host::binding::RunBinding;
use crate::host::fs::canonical_workspace;
use crate::host::process::{SpawnLimits, spawn_confined};
use crate::outputs::{OutputLedger, OutputScan, admit_scan};
use crate::profile::{effective_profile_digest, resolve_profile};
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
    /// The response carried no proof it belongs to this run.
    RunBindingUnproved,
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

/// The content address of the manifest this build carries. Public so a test
/// can hold the profile's pin to it: the chain manifest → pin → profile
/// address is maintained by hand and has drifted twice.
#[must_use]
pub fn engine_manifest_digest() -> String {
    sha256_hex(ENGINE_MANIFEST.as_bytes())
}

/// The engine manifest this build carries, embedded so the identity the
/// attestation binds is a property of the binary rather than of its caller.
pub const ENGINE_MANIFEST: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/profiles/engine-manifest.v1.json"
));

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
    identity: &RunIdentity,
    signing_seed: &[u8; 32],
    generated_at: &str,
) -> Result<Value, RunError> {
    let resolved = resolve_profile(registry, requested_id, requested_digest, profile_document)?;
    let profile = resolved.profile();
    let plan = host_plan(dedicated_uid, dedicated_gid);
    let facts = gather_host_facts();
    let effective = resolve_controls(profile, &facts)?;
    // The workspace is canonicalized and becomes the worker's starting
    // directory. It is NOT a boundary: nothing here bounds the worker's own
    // syscalls, which is precisely why filesystem_confinement is not an engine
    // capability and never reaches effectiveControls.
    let workspace = canonical_workspace(workspace_root)?;

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
    // What the run enforces, not what it was asked for: the two digests are
    // distinct fields precisely so a narrower confinement cannot pass for an
    // honoured one (K4 rounds 1 and 2).
    let effective_digest = effective_profile_digest(profile_document)?;

    // The engine identity is the harness's own fact, never the caller's: the
    // manifest travels with the crate and its digest is recomputed here, then
    // held to what the profile pinned (round 3 verdict on 0ab2a20).
    let pinned = profile.engine_manifest();
    let engine_digest = engine_manifest_digest();
    if pinned.digest != engine_digest {
        return Err(HarnessRefusal::ControlNotEnforceable.into());
    }
    let engine_manifest = ArtifactRef::new(&pinned.id, &engine_digest, &pinned.media_type);
    // The run's own token: the response must carry it, or it is not this
    // run's response (`runBoundToken`, const true in the locked profile).
    let binding = RunBinding::fresh()?;
    let outcome = spawn_confined(
        program, args, payload, &workspace, &limits, &plan, &chain, &binding,
    )?;

    // KNOWN LIMIT (xhigh review of f27b3c9): this ledger lives for one spawn
    // and receives one admit, so maxTotalBytes only ever meets a value the
    // per-tool bound already capped. It becomes a real accumulator when a run
    // spans several tool invocations — the surface that does not exist yet.
    // Left visible rather than removed: the bound is unreachable, not wrong.
    let mut ledger = OutputLedger::new(profile.max_bytes_per_tool(), profile.max_total_bytes());
    ledger.admit(outcome.output().len() as u64, outcome.truncated())?;
    // The scan can only vouch for what was captured: a transport failure
    // leaves the output short by an unknown amount, which the matrix already
    // treats as an unscanned output (xhigh review of f27b3c9).
    admit_scan(if outcome.capture_failed() {
        OutputScan::Interrupted
    } else {
        OutputScan::Complete
    })?;
    if !outcome.run_binding_proved() {
        // The worker ran inside honoured controls but its answer is not bound
        // to this run: a distinct outcome from a worker that simply failed,
        // and not a confinement refusal either.
        return Err(RunError::RunBindingUnproved);
    }
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
        &effective_digest,
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
