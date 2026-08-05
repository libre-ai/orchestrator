/// The closed refusal matrix of `docs/apps/harness.md` — thirteen stable
/// codes, one variant each. A refusal names the failing invariant, never the
/// payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessRefusal {
    ProfileUnresolved,
    ProfileDigestMismatch,
    PlatformUnsupported,
    ControlNotEnforceable,
    PathEscapesWorkspace,
    SymlinkPolicyViolation,
    WriteOutsideWritableSet,
    DeniedPathTouched,
    OutputLimitExceeded,
    OutputScanIncomplete,
    CapabilityNotEnabled,
    AttestationBindingIncomplete,
    AttestationUnsigned,
}

impl HarnessRefusal {
    /// Every code of the matrix, in specification order — lets a test assert
    /// mechanically that the matrix stays closed and fully covered.
    pub const ALL: [Self; 13] = [
        Self::ProfileUnresolved,
        Self::ProfileDigestMismatch,
        Self::PlatformUnsupported,
        Self::ControlNotEnforceable,
        Self::PathEscapesWorkspace,
        Self::SymlinkPolicyViolation,
        Self::WriteOutsideWritableSet,
        Self::DeniedPathTouched,
        Self::OutputLimitExceeded,
        Self::OutputScanIncomplete,
        Self::CapabilityNotEnabled,
        Self::AttestationBindingIncomplete,
        Self::AttestationUnsigned,
    ];

    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ProfileUnresolved => "harness.profile_unresolved",
            Self::ProfileDigestMismatch => "harness.profile_digest_mismatch",
            Self::PlatformUnsupported => "harness.platform_unsupported",
            Self::ControlNotEnforceable => "harness.control_not_enforceable",
            Self::PathEscapesWorkspace => "harness.path_escapes_workspace",
            Self::SymlinkPolicyViolation => "harness.symlink_policy_violation",
            Self::WriteOutsideWritableSet => "harness.write_outside_writable_set",
            Self::DeniedPathTouched => "harness.denied_path_touched",
            Self::OutputLimitExceeded => "harness.output_limit_exceeded",
            Self::OutputScanIncomplete => "harness.output_scan_incomplete",
            Self::CapabilityNotEnabled => "harness.capability_not_enabled",
            Self::AttestationBindingIncomplete => "harness.attestation_binding_incomplete",
            Self::AttestationUnsigned => "harness.attestation_unsigned",
        }
    }
}
