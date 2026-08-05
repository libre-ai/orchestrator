#![forbid(unsafe_code)]

//! Attested execution confinement (WP-G3-H01, ADR-0018 D2).
//!
//! The harness takes a requested profile, applies the controls that profile
//! prescribes, runs the work inside them, and emits a signed attestation
//! binding what was asked to what was actually enforced. Its value is not
//! that it restricts — it is that it proves it restricted
//! (`docs/apps/harness.md`).
//!
//! Everything outside `host` is pure and hostless; `host` holds exactly one
//! OS capability at this stage — spawning a confined local process. The
//! boundary is mechanically enforced by `verification/agent-harness/`.

mod attestation;
mod canonical;
mod confinement;
mod controls;
mod fs_policy;
mod host;
mod outputs;
mod profile;
mod refusal;

pub use host::{
    ConfinedOutcome, RunError, RunIdentity, SpawnLimits, WorkspaceObserver, canonical_workspace,
    run_confined_attested, spawn_confined,
};

pub use attestation::{
    ArtifactRef, AttestationInputs, VerificationError, attestation_digest, public_key_base64url,
    sign_attestation, verify_attestation,
};
pub use confinement::{ConfinementPlan, ProcessPrescription, WrapperChain, plan_wrapper_chain};
pub use controls::{EffectiveControls, HostFacts, resolve_controls};
pub use fs_policy::{PathAccess, PathAccessKind, evaluate_path_access};
pub use outputs::{OutputLedger, OutputScan, admit_scan};
pub use profile::{
    HarnessProfile, ResolvedProfile, parse_profile, profile_digest, resolve_profile,
};
pub use refusal::HarnessRefusal;
