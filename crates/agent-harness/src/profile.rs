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

/// The profile surface this engine actually applies, in the order the
/// projection keeps it. Public because an operator must be able to recompute
/// the effective digest from the requested profile alone: the projection is a
/// documented rule, not a secret of the harness.
///
/// Absent on purpose, because nothing in this engine acts on them at this
/// stage: `filesystem` (the worker's own syscalls are bounded by the
/// dedicated identity's DAC, not by the path sets), `providerGateway`,
/// `privilegedToolBroker`, `operationalLogs` and `attestation`. Of
/// `workerTransport` the whole block survives: `kind` selects the mechanism,
/// `runBoundToken` is a per-run token the response must return, and
/// `verifyOsPeer` holds by construction — the transport is an anonymous pair
/// handed to exactly one child, and the capability guard keeps every named
/// socket out of the crate so no other peer can exist.
pub const APPLIED_PROFILE_SURFACE: &[&str] = &[
    "schemaVersion",
    "id",
    "version",
    "enforcement",
    "supportedPlatforms",
    "process",
    "sandboxEngine",
    "outputs",
    "workerTransport",
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
    let mut projected = serde_json::Map::new();
    for key in APPLIED_PROFILE_SURFACE {
        if let Some(value) = document.get(*key) {
            projected.insert((*key).to_owned(), value.clone());
        }
    }
    profile_digest(&Value::Object(projected))
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
