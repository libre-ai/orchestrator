use crate::canonical::{base64url_decode, base64url_encode, canonical_sha256, hex_to_bytes32};
use crate::controls::is_engine_capability;
use crate::refusal::HarnessRefusal;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use libre_ai_contract_types::ContractRegistry;
use serde_json::{Value, json};

const ATTESTATION_SCHEMA: &str = "harness-attestation.v1.schema.json";
const SCHEMA_VERSION: &str = "libre-ai.harness-attestation.v1";
const DIGEST_FIELD: &str = "attestationDigest";
const SIGNATURE_FIELD: &str = "signature";

/// A content-addressed artifact reference (`common.v1` `artifactReference`).
#[derive(Clone, Debug)]
pub struct ArtifactRef {
    id: String,
    digest: String,
    media_type: String,
}

impl ArtifactRef {
    #[must_use]
    pub fn new(id: &str, digest: &str, media_type: &str) -> Self {
        Self {
            id: id.to_owned(),
            digest: digest.to_owned(),
            media_type: media_type.to_owned(),
        }
    }
}

/// Everything an attestation binds. Requested and effective profile digests
/// are distinct fields on purpose: a silently degraded confinement must stay
/// distinguishable from an honoured one.
#[derive(Clone, Debug)]
pub struct AttestationInputs {
    id: String,
    tenant_id: String,
    mission_id: String,
    run_id: String,
    plan_digest: String,
    requested_profile_digest: String,
    effective_profile_digest: String,
    worker_manifest_digests: Vec<String>,
    sandbox_engine_manifest: ArtifactRef,
    platform: String,
    effective_controls: Vec<String>,
    network_mode: String,
    generated_at: String,
    signing_key_id: String,
}

impl AttestationInputs {
    #[expect(
        clippy::too_many_arguments,
        reason = "one parameter per bound field, by design"
    )]
    #[must_use]
    pub fn new(
        id: &str,
        tenant_id: &str,
        mission_id: &str,
        run_id: &str,
        plan_digest: &str,
        requested_profile_digest: &str,
        effective_profile_digest: &str,
        worker_manifest_digests: Vec<String>,
        sandbox_engine_manifest: ArtifactRef,
        platform: &str,
        effective_controls: Vec<String>,
        network_mode: &str,
        generated_at: &str,
        signing_key_id: &str,
    ) -> Self {
        Self {
            id: id.to_owned(),
            tenant_id: tenant_id.to_owned(),
            mission_id: mission_id.to_owned(),
            run_id: run_id.to_owned(),
            plan_digest: plan_digest.to_owned(),
            requested_profile_digest: requested_profile_digest.to_owned(),
            effective_profile_digest: effective_profile_digest.to_owned(),
            worker_manifest_digests,
            sandbox_engine_manifest,
            platform: platform.to_owned(),
            effective_controls,
            network_mode: network_mode.to_owned(),
            generated_at: generated_at.to_owned(),
            signing_key_id: signing_key_id.to_owned(),
        }
    }
}

/// The attestation content address: SHA-256 of the JCS canonical form with
/// `attestationDigest` and `signature` excluded (SEMANTICS.md).
pub fn attestation_digest(document: &Value) -> Result<String, HarnessRefusal> {
    let mut unsigned = document.clone();
    if let Some(object) = unsigned.as_object_mut() {
        object.remove(DIGEST_FIELD);
        object.remove(SIGNATURE_FIELD);
    }
    canonical_sha256(&unsigned).ok_or(HarnessRefusal::AttestationBindingIncomplete)
}

fn signed_message(digest: &str) -> Result<Vec<u8>, HarnessRefusal> {
    let raw = hex_to_bytes32(digest).ok_or(HarnessRefusal::AttestationBindingIncomplete)?;
    let mut message = Vec::with_capacity(SCHEMA_VERSION.len() + 1 + raw.len());
    message.extend_from_slice(SCHEMA_VERSION.as_bytes());
    message.push(0);
    message.extend_from_slice(&raw);
    Ok(message)
}

fn assemble(inputs: &AttestationInputs) -> Result<Value, HarnessRefusal> {
    // An attestation that binds less than it claims is invalid, not partial:
    // an empty binding never reaches the signature.
    if inputs.worker_manifest_digests.is_empty() || inputs.effective_controls.is_empty() {
        return Err(HarnessRefusal::AttestationBindingIncomplete);
    }
    // A control the engine does not offer has no mechanism behind it: signing
    // it would be the overclaim this crate exists to make impossible.
    if !inputs
        .effective_controls
        .iter()
        .all(|control| is_engine_capability(control))
    {
        return Err(HarnessRefusal::AttestationBindingIncomplete);
    }
    Ok(json!({
        "schemaVersion": SCHEMA_VERSION,
        "id": inputs.id,
        "tenantId": inputs.tenant_id,
        "missionId": inputs.mission_id,
        "runId": inputs.run_id,
        "planDigest": inputs.plan_digest,
        "requestedProfileDigest": inputs.requested_profile_digest,
        "effectiveProfileDigest": inputs.effective_profile_digest,
        "workerManifestDigests": inputs.worker_manifest_digests,
        "sandboxEngineManifest": {
            "id": inputs.sandbox_engine_manifest.id,
            "digest": inputs.sandbox_engine_manifest.digest,
            "mediaType": inputs.sandbox_engine_manifest.media_type,
        },
        "platform": inputs.platform,
        "effectiveControls": inputs.effective_controls,
        "networkMode": inputs.network_mode,
        "generatedAt": inputs.generated_at,
        "signingKeyId": inputs.signing_key_id,
    }))
}

/// Journey 5: assemble, digest, sign — and refuse to emit anything that does
/// not validate against the locked contract. The signing key is a parameter;
/// the production key ceremony is a deferred owner act.
pub fn sign_attestation(
    registry: &ContractRegistry,
    inputs: &AttestationInputs,
    signing_seed: &[u8; 32],
) -> Result<Value, HarnessRefusal> {
    let mut document = assemble(inputs)?;
    let digest = attestation_digest(&document)?;
    let message = signed_message(&digest)?;
    let signing_key = SigningKey::from_bytes(signing_seed);
    let signature = signing_key.sign(&message);
    document[DIGEST_FIELD] = Value::String(digest);
    document[SIGNATURE_FIELD] = Value::String(base64url_encode(&signature.to_bytes()));
    let issues = registry
        .validate(ATTESTATION_SCHEMA, &document)
        .map_err(|_| HarnessRefusal::AttestationBindingIncomplete)?;
    if !issues.is_empty() {
        return Err(HarnessRefusal::AttestationBindingIncomplete);
    }
    Ok(document)
}

/// What went wrong when an attestation did not verify.
///
/// A key that does not decode is the verifier's own input being malformed —
/// the operator mistyped or padded it. Reporting that with the refusal that
/// means "this attestation carries no valid signature" tells an operator
/// their genuine attestation is forged (xhigh review of f27b3c9), so the two
/// stay distinct. Only `Refused` carries a code of the closed matrix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerificationError {
    MalformedVerifyingKey,
    Refused(HarnessRefusal),
}

impl VerificationError {
    #[must_use]
    pub const fn is_malformed_key(self) -> bool {
        matches!(self, Self::MalformedVerifyingKey)
    }

    /// The matrix code of a refusal about the attestation. A malformed key is
    /// not a verdict about the attestation and carries no matrix code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::MalformedVerifyingKey => "harness.verifying_key_malformed",
            Self::Refused(refusal) => refusal.code(),
        }
    }
}

impl From<HarnessRefusal> for VerificationError {
    fn from(refusal: HarnessRefusal) -> Self {
        Self::Refused(refusal)
    }
}

/// Journey 6: an operator holding only the attestation and a public key
/// re-verifies the binding without access to the run, the worker or the
/// harness that produced it.
pub fn verify_attestation(
    registry: &ContractRegistry,
    document: &Value,
    public_key_base64url: &str,
) -> Result<(), VerificationError> {
    let issues = registry
        .validate(ATTESTATION_SCHEMA, document)
        .map_err(|_| VerificationError::from(HarnessRefusal::AttestationBindingIncomplete))?;
    if !issues.is_empty() {
        return Err(HarnessRefusal::AttestationBindingIncomplete.into());
    }
    let declared = document[DIGEST_FIELD]
        .as_str()
        .ok_or(VerificationError::from(
            HarnessRefusal::AttestationBindingIncomplete,
        ))?;
    let recomputed = attestation_digest(document)?;
    if declared != recomputed {
        // The signature may be intact, but it attests different content:
        // this document is not validly signed.
        return Err(HarnessRefusal::AttestationUnsigned.into());
    }
    let signature_text = document[SIGNATURE_FIELD]
        .as_str()
        .ok_or(VerificationError::from(HarnessRefusal::AttestationUnsigned))?;
    let signature_bytes = base64url_decode(signature_text)
        .ok_or(VerificationError::from(HarnessRefusal::AttestationUnsigned))?;
    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|_| VerificationError::from(HarnessRefusal::AttestationUnsigned))?;
    // The key is the verifier's input, not part of the attestation.
    let key_bytes =
        base64url_decode(public_key_base64url).ok_or(VerificationError::MalformedVerifyingKey)?;
    let key_array: [u8; 32] = key_bytes
        .try_into()
        .map_err(|_| VerificationError::MalformedVerifyingKey)?;
    let verifying_key = VerifyingKey::from_bytes(&key_array)
        .map_err(|_| VerificationError::MalformedVerifyingKey)?;
    let message = signed_message(declared)?;
    verifying_key
        .verify(&message, &signature)
        .map_err(|_| VerificationError::Refused(HarnessRefusal::AttestationUnsigned))
}

/// The unpadded base64url form of the public key belonging to a signing seed
/// — the identity operators verify against.
#[must_use]
pub fn public_key_base64url(signing_seed: &[u8; 32]) -> String {
    base64url_encode(
        SigningKey::from_bytes(signing_seed)
            .verifying_key()
            .as_bytes(),
    )
}
