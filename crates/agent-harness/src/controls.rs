use crate::profile::HarnessProfile;
use crate::refusal::HarnessRefusal;

/// What the local-process engine of ADR-0018 D2 actually provides. Anything a
/// profile requires beyond this list is denied (`denyOnMissing` is const true
/// in the locked contract), never approximated.
///
/// `filesystem_confinement` is deliberately absent. Bounding a worker's own
/// syscalls to the workspace needs chroot or a mount namespace, neither of
/// which this stage opens; the path-handling machinery holds for accesses the
/// harness performs, not for the worker's direct ones. Listing it here would
/// put it in `effectiveControls` and make the attestation claim a boundary
/// that does not exist (K4 verdicts on 5bee6a3, blocking finding 1).
const ENGINE_CAPABILITIES: [&str; 4] = [
    "output_bounds",
    "process_isolation",
    "resource_limits",
    "worker_transport_isolation",
];

/// Transport kinds whose enforcement is closed at this stage: expressible in
/// the locked schema, refused by the runtime until their own package and
/// review exist (docs/apps/harness.md, non-goals).
const CLOSED_TRANSPORT_KINDS: [&str; 1] = ["private-network-namespace"];

/// Host observations the pure control resolution decides on. Gathered by the
/// host layer; constructed directly in tests.
#[derive(Clone, Debug)]
pub struct HostFacts {
    platform: String,
    euid_is_root: bool,
    setpriv_present: bool,
}

impl HostFacts {
    #[must_use]
    pub fn new(platform: &str, euid_is_root: bool, setpriv_present: bool) -> Self {
        Self {
            platform: platform.to_owned(),
            euid_is_root,
            setpriv_present,
        }
    }
}

/// The controls actually applied to a run — the attestation binds exactly
/// this set, distinct from what was requested, so a silently degraded
/// confinement stays impossible by construction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveControls {
    identifiers: Vec<String>,
}

impl EffectiveControls {
    #[must_use]
    pub fn identifiers(&self) -> &[String] {
        &self.identifiers
    }
}

const fn capability_enforceable(capability: &str, facts: &HostFacts) -> bool {
    match capability.as_bytes() {
        b"process_isolation" => facts.euid_is_root && facts.setpriv_present,
        b"resource_limits" => facts.setpriv_present,
        // output_bounds and worker_transport_isolation are enforced by the
        // harness itself, host privileges regardless.
        _ => true,
    }
}

/// Journey 2 of the specification: every control the profile prescribes is
/// either applied or the run is refused — a control that cannot be applied is
/// `harness.control_not_enforceable`, never a warning followed by execution.
pub fn resolve_controls(
    profile: &HarnessProfile,
    facts: &HostFacts,
) -> Result<EffectiveControls, HarnessRefusal> {
    if !profile
        .supported_platforms()
        .iter()
        .any(|platform| platform == &facts.platform)
    {
        return Err(HarnessRefusal::PlatformUnsupported);
    }

    if CLOSED_TRANSPORT_KINDS
        .iter()
        .any(|kind| *kind == profile.worker_transport_kind())
    {
        return Err(HarnessRefusal::CapabilityNotEnabled);
    }

    let mut identifiers: Vec<String> = Vec::with_capacity(profile.required_capabilities().len());
    for capability in profile.required_capabilities() {
        if !ENGINE_CAPABILITIES.contains(&capability.as_str()) {
            return Err(HarnessRefusal::ControlNotEnforceable);
        }
        if !capability_enforceable(capability, facts) {
            return Err(HarnessRefusal::ControlNotEnforceable);
        }
        identifiers.push(capability.clone());
    }
    identifiers.sort();
    Ok(EffectiveControls { identifiers })
}
