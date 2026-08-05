use crate::canonical::canonical_sha256;
use crate::confinement::ProcessPrescription;
use crate::refusal::HarnessRefusal;
use libre_ai_contract_types::ContractRegistry;
use serde::Deserialize;
use serde_json::Value;

const PROFILE_SCHEMA: &str = "harness-profile.v1.schema.json";
const DIGEST_FIELD: &str = "profileDigest";

/// A validated harness profile. Fields stay private; the crate exposes only
/// what confinement decisions actually consume.
#[derive(Clone, Debug)]
pub struct HarnessProfile {
    id: String,
    version: String,
    supported_platforms: Vec<String>,
    max_bytes_per_tool: u64,
    max_total_bytes: u64,
    max_duration_seconds: u64,
    process: ProcessPrescription,
    worker_transport_kind: String,
    required_capabilities: Vec<String>,
    engine_manifest: ArtifactWire,
    deny_on_missing: bool,
    read_only_paths: Vec<String>,
    writable_paths: Vec<String>,
    denied_paths: Vec<String>,
    declared_digest: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfileWire {
    id: String,
    version: String,
    supported_platforms: Vec<String>,
    outputs: OutputsWire,
    process: ProcessWire,
    filesystem: FilesystemWire,
    worker_transport: WorkerTransportWire,
    sandbox_engine: SandboxEngineWire,
    profile_digest: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProcessWire {
    dedicated_identity: bool,
    deny_privilege_escalation: bool,
    drop_ambient_capabilities: bool,
    kill_process_group: bool,
    max_processes: u32,
    max_duration_seconds: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FilesystemWire {
    read_only: Vec<String>,
    writable: Vec<String>,
    denied: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OutputsWire {
    max_bytes_per_tool: u64,
    max_total_bytes: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkerTransportWire {
    kind: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SandboxEngineWire {
    required_capabilities: Vec<String>,
    manifest: ArtifactWire,
    deny_on_missing: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactWire {
    pub id: String,
    pub digest: String,
    pub media_type: String,
}

impl From<ProfileWire> for HarnessProfile {
    fn from(wire: ProfileWire) -> Self {
        Self {
            id: wire.id,
            version: wire.version,
            supported_platforms: wire.supported_platforms,
            max_bytes_per_tool: wire.outputs.max_bytes_per_tool,
            max_total_bytes: wire.outputs.max_total_bytes,
            max_duration_seconds: wire.process.max_duration_seconds,
            process: ProcessPrescription::new(
                wire.process.dedicated_identity,
                wire.process.deny_privilege_escalation,
                wire.process.drop_ambient_capabilities,
                wire.process.kill_process_group,
                wire.process.max_processes,
            ),
            worker_transport_kind: wire.worker_transport.kind,
            required_capabilities: wire.sandbox_engine.required_capabilities,
            engine_manifest: wire.sandbox_engine.manifest,
            deny_on_missing: wire.sandbox_engine.deny_on_missing,
            read_only_paths: wire.filesystem.read_only,
            writable_paths: wire.filesystem.writable,
            denied_paths: wire.filesystem.denied,
            declared_digest: wire.profile_digest,
        }
    }
}

impl HarnessProfile {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    #[must_use]
    pub fn supported_platforms(&self) -> &[String] {
        &self.supported_platforms
    }

    #[must_use]
    pub const fn max_bytes_per_tool(&self) -> u64 {
        self.max_bytes_per_tool
    }

    #[must_use]
    pub const fn max_total_bytes(&self) -> u64 {
        self.max_total_bytes
    }

    #[must_use]
    pub const fn max_duration_seconds(&self) -> u64 {
        self.max_duration_seconds
    }

    /// The whole const-locked process block, never a subset: every field is
    /// applied by the wrapper chain or the run is refused.
    #[must_use]
    pub const fn process(&self) -> ProcessPrescription {
        self.process
    }

    #[must_use]
    pub fn worker_transport_kind(&self) -> &str {
        &self.worker_transport_kind
    }

    #[must_use]
    pub fn required_capabilities(&self) -> &[String] {
        &self.required_capabilities
    }

    /// The engine the profile pins. The harness holds itself to it rather
    /// than letting the caller assert which engine ran (round 3 verdict).
    #[must_use]
    pub fn engine_manifest(&self) -> &ArtifactWire {
        &self.engine_manifest
    }

    #[must_use]
    pub const fn deny_on_missing(&self) -> bool {
        self.deny_on_missing
    }

    #[must_use]
    pub fn read_only_paths(&self) -> &[String] {
        &self.read_only_paths
    }

    #[must_use]
    pub fn writable_paths(&self) -> &[String] {
        &self.writable_paths
    }

    #[must_use]
    pub fn denied_paths(&self) -> &[String] {
        &self.denied_paths
    }
}

/// The content address of a profile: SHA-256 of the JCS canonical form with
/// `profileDigest` excluded (SEMANTICS.md exclusion table).
pub fn profile_digest(document: &Value) -> Result<String, HarnessRefusal> {
    let mut unsigned = document.clone();
    if let Some(object) = unsigned.as_object_mut() {
        object.remove(DIGEST_FIELD);
    }
    canonical_sha256(&unsigned).ok_or(HarnessRefusal::ProfileUnresolved)
}

/// The profile surface this engine actually applies, as JSON pointers.
///
/// Field granularity, not block granularity: every block of this contract
/// mixes fields the engine acts on with fields it does not, so a block-level
/// projection is necessarily wrong in one direction or the other — it was
/// over-claiming five prescriptions and under-claiming four when round 3
/// measured it. A pointer enters this list only when a code path reads the
/// field AND acts on it.
///
/// Public because an operator must be able to recompute the effective digest
/// from the requested profile alone: the projection is a documented rule, not
/// a secret of the harness.
pub const APPLIED_PROFILE_SURFACE: &[&str] = &[
    // Identity of the contract and of the profile the run resolved.
    "/schemaVersion",
    "/id",
    // Checked against the running platform before anything starts.
    "/supportedPlatforms",
    // Every field below is applied by the wrapper chain.
    "/process/dedicatedIdentity",
    "/process/denyPrivilegeEscalation",
    "/process/dropAmbientCapabilities",
    "/process/killProcessGroup",
    "/process/maxProcesses",
    "/process/maxDurationSeconds",
    // The engine identity the harness recomputes and holds itself to, and the
    // deny-on-missing rule it applies to required capabilities.
    "/sandboxEngine/manifest",
    "/sandboxEngine/requiredCapabilities",
    "/sandboxEngine/denyOnMissing",
    // The byte bounds the output ledger enforces.
    "/outputs/maxBytesPerTool",
    "/outputs/maxTotalBytes",
    // The transport: kind selects the mechanism, the token is required back,
    // the peer is named by the kernel, and no loopback exists to allow —
    // the capability guard keeps every network type out of the crate.
    "/workerTransport/kind",
    "/workerTransport/runBoundToken",
    "/workerTransport/verifyOsPeer",
    "/workerTransport/hostLoopbackAllowed",
    // What the attestation itself binds, which this crate does bind.
    "/attestation/signed",
    "/attestation/bindRequestedProfile",
    "/attestation/bindEffectiveControls",
    "/attestation/bindWorkerManifests",
];

/// The content address of what was actually applied.
///
/// `requestedProfileDigest` names the profile that was asked for;
/// `effectiveProfileDigest` must name what the run enforced, so that a
/// confinement applying less than its profile prescribes stays
/// distinguishable from one that honoured it (`docs/apps/harness.md`,
/// domain protocol). Echoing the requested digest made the two identical by
/// construction and let the attestation assert every inert block of the
/// document — the defect both K4 rounds named on 5bee6a3 and f27b3c9.
pub fn effective_profile_digest(document: &Value) -> Result<String, HarnessRefusal> {
    let mut projected = Value::Object(serde_json::Map::new());
    for pointer in APPLIED_PROFILE_SURFACE {
        let Some(value) = document.pointer(pointer) else {
            continue;
        };
        let mut cursor = &mut projected;
        let segments: Vec<&str> = pointer.trim_start_matches('/').split('/').collect();
        let Some((leaf, parents)) = segments.split_last() else {
            continue;
        };
        for segment in parents {
            cursor = cursor
                .as_object_mut()
                .ok_or(HarnessRefusal::ProfileUnresolved)?
                .entry((*segment).to_owned())
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
        }
        cursor
            .as_object_mut()
            .ok_or(HarnessRefusal::ProfileUnresolved)?
            .insert((*leaf).to_owned(), value.clone());
    }
    profile_digest(&projected)
}

/// Validate against the locked contract, then hold the document to its own
/// word: the embedded `profileDigest` must match the recomputed content
/// address — a profile that lies about its digest is refused, not repaired.
pub fn parse_profile(
    registry: &ContractRegistry,
    document: &Value,
) -> Result<HarnessProfile, HarnessRefusal> {
    let issues = registry
        .validate(PROFILE_SCHEMA, document)
        .map_err(|_| HarnessRefusal::ProfileUnresolved)?;
    if !issues.is_empty() {
        return Err(HarnessRefusal::ProfileUnresolved);
    }
    let computed = profile_digest(document)?;
    let wire: ProfileWire =
        serde_json::from_value(document.clone()).map_err(|_| HarnessRefusal::ProfileUnresolved)?;
    if wire.profile_digest != computed {
        return Err(HarnessRefusal::ProfileDigestMismatch);
    }
    Ok(wire.into())
}

/// A profile resolved for a specific request — identity and content address
/// both verified against what the caller actually asked for.
#[derive(Clone, Debug)]
pub struct ResolvedProfile {
    profile: HarnessProfile,
    digest: String,
}

impl ResolvedProfile {
    #[must_use]
    pub fn profile(&self) -> &HarnessProfile {
        &self.profile
    }

    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }
}

/// Journey 1 of the specification: the caller names a profile and its digest;
/// the harness refuses when the requested identifier and the resolved content
/// disagree.
pub fn resolve_profile(
    registry: &ContractRegistry,
    requested_id: &str,
    requested_digest: &str,
    document: &Value,
) -> Result<ResolvedProfile, HarnessRefusal> {
    let profile = parse_profile(registry, document)?;
    if profile.id() != requested_id {
        return Err(HarnessRefusal::ProfileUnresolved);
    }
    if profile.declared_digest != requested_digest {
        return Err(HarnessRefusal::ProfileDigestMismatch);
    }
    let digest = profile.declared_digest.clone();
    Ok(ResolvedProfile { profile, digest })
}
