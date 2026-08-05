use crate::canonical::canonical_sha256;
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
    filesystem: FilesystemWire,
    worker_transport: WorkerTransportWire,
    sandbox_engine: SandboxEngineWire,
    profile_digest: String,
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
