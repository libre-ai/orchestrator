use std::fmt::{self, Debug, Display, Formatter};

use libre_ai_contract_types::ContractRegistry;
use serde::Deserialize;
use serde_json::Value;

use super::AuthorizedExecutionRefusal;
use super::document::require_valid;

const EFFECT_ATTESTATION_SCHEMA: &str = "effect-attestation.v1.schema.json";

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum EffectObservation<'a> {
    Unavailable,
    Authoritative {
        expected_organization_id: &'a str,
        expected_run_id: &'a str,
        expected_attempt_id: &'a str,
        current_generation: u64,
        active_fencing: Option<u64>,
        expected_executor_profile_digest: &'a str,
        generation_consumed: bool,
        lineage_closed: bool,
        predecessor_effects_terminal: bool,
        executor_profile_qualified: bool,
        prior_emission: Option<(&'a str, &'a str)>,
        existing_attempt_emission_id: Option<&'a str>,
    },
}

impl Debug for EffectObservation<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Unavailable => "EffectObservation::Unavailable",
            Self::Authoritative { .. } => "EffectObservation::Authoritative(<redacted>)",
        })
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct EffectApplication {
    status: TerminalEffectStatus,
    effect_emission_id: String,
    attempt_id: String,
}

impl EffectApplication {
    pub const fn status(&self) -> &'static str {
        self.status.code()
    }

    pub fn effect_emission_id(&self) -> &str {
        &self.effect_emission_id
    }

    pub fn attempt_id(&self) -> &str {
        &self.attempt_id
    }
}

impl Debug for EffectApplication {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EffectApplication")
            .field("code", &"effect-valid")
            .finish()
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum TerminalEffectStatus {
    Committed,
    RejectedFinal,
    NotCommittedFinal,
}

impl TerminalEffectStatus {
    const fn code(self) -> &'static str {
        match self {
            Self::Committed => "committed",
            Self::RejectedFinal => "rejected-final",
            Self::NotCommittedFinal => "not-committed-final",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectRefusal {
    OrganizationMismatch,
    IdentityMismatch,
    AttemptMismatch,
    LineageAdministrativelyClosed,
    GenerationConsumed,
    GenerationStale,
    EmissionDivergent,
    SecondEmissionForAttempt,
    FencingStale,
    ExecutorUnqualified,
    PredecessorEffectsNonterminal,
}

impl EffectRefusal {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::OrganizationMismatch => "organization-mismatch",
            Self::IdentityMismatch => "identity-mismatch",
            Self::AttemptMismatch => "attempt-mismatch",
            Self::LineageAdministrativelyClosed => "lineage-administratively-closed",
            Self::GenerationConsumed => "generation-consumed",
            Self::GenerationStale => "generation-stale",
            Self::EmissionDivergent => "emission-divergent",
            Self::SecondEmissionForAttempt => "second-emission-for-attempt",
            Self::FencingStale => "fencing-stale",
            Self::ExecutorUnqualified => "executor-unqualified",
            Self::PredecessorEffectsNonterminal => "predecessor-effects-nonterminal",
        }
    }
}

impl Display for EffectRefusal {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EffectDecision {
    Apply(EffectApplication),
    Idempotent,
    ContinuityBarrier,
    Refused(EffectRefusal),
    BoundaryRefused(AuthorizedExecutionRefusal),
}

impl EffectDecision {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Apply(_) => "effect-valid",
            Self::Idempotent => "emission-duplicate",
            Self::ContinuityBarrier => "effect-state-unknown",
            Self::Refused(refusal) => refusal.code(),
            Self::BoundaryRefused(refusal) => refusal.code(),
        }
    }

    pub const fn application(&self) -> Option<&EffectApplication> {
        match self {
            Self::Apply(application) => Some(application),
            Self::Idempotent
            | Self::ContinuityBarrier
            | Self::Refused(_)
            | Self::BoundaryRefused(_) => None,
        }
    }
}

impl Display for EffectDecision {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireEffectAttestation {
    organization_id: String,
    run_id: String,
    generation: u64,
    attempt_id: String,
    effect_emission_id: String,
    executor_profile_ref: WireArtifactReference,
    fencing_value: Option<u64>,
    status: WireEffectStatus,
    preimage_digest: String,
}

#[derive(Deserialize)]
struct WireArtifactReference {
    digest: String,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum WireEffectStatus {
    Reserved,
    Started,
    Committed,
    RejectedFinal,
    NotCommittedFinal,
    StateUnknown,
}

pub fn evaluate_effect_attestation(
    registry: &ContractRegistry,
    attestation_document: &Value,
    observation: EffectObservation<'_>,
) -> EffectDecision {
    if require_valid(registry, EFFECT_ATTESTATION_SCHEMA, attestation_document).is_err() {
        return boundary(AuthorizedExecutionRefusal::SchemaInvalid);
    }
    let attestation: WireEffectAttestation =
        match serde_json::from_value(attestation_document.clone()) {
            Ok(attestation) => attestation,
            Err(_) => return boundary(AuthorizedExecutionRefusal::SchemaInvalid),
        };
    let EffectObservation::Authoritative {
        expected_organization_id,
        expected_run_id,
        expected_attempt_id,
        current_generation,
        active_fencing,
        expected_executor_profile_digest,
        generation_consumed,
        lineage_closed,
        predecessor_effects_terminal,
        executor_profile_qualified,
        prior_emission,
        existing_attempt_emission_id,
    } = observation
    else {
        return boundary(AuthorizedExecutionRefusal::StoreUnavailable);
    };

    if expected_organization_id != attestation.organization_id {
        return refused(EffectRefusal::OrganizationMismatch);
    }
    if expected_run_id != attestation.run_id {
        return refused(EffectRefusal::IdentityMismatch);
    }
    if expected_attempt_id != attestation.attempt_id {
        return refused(EffectRefusal::AttemptMismatch);
    }
    if lineage_closed {
        return refused(EffectRefusal::LineageAdministrativelyClosed);
    }
    if generation_consumed {
        return refused(EffectRefusal::GenerationConsumed);
    }
    if current_generation != attestation.generation {
        return refused(EffectRefusal::GenerationStale);
    }
    if let Some((prior_id, prior_digest)) = prior_emission {
        if prior_id == attestation.effect_emission_id && prior_digest == attestation.preimage_digest
        {
            return EffectDecision::Idempotent;
        }
        return refused(EffectRefusal::EmissionDivergent);
    }
    if existing_attempt_emission_id.is_some() {
        return refused(EffectRefusal::SecondEmissionForAttempt);
    }
    if active_fencing != attestation.fencing_value {
        return refused(EffectRefusal::FencingStale);
    }
    if !executor_profile_qualified
        || expected_executor_profile_digest != attestation.executor_profile_ref.digest
    {
        return refused(EffectRefusal::ExecutorUnqualified);
    }
    if matches!(attestation.status, WireEffectStatus::StateUnknown) {
        return EffectDecision::ContinuityBarrier;
    }
    if !predecessor_effects_terminal {
        return refused(EffectRefusal::PredecessorEffectsNonterminal);
    }
    let status = match attestation.status {
        WireEffectStatus::Committed => TerminalEffectStatus::Committed,
        WireEffectStatus::RejectedFinal => TerminalEffectStatus::RejectedFinal,
        WireEffectStatus::NotCommittedFinal => TerminalEffectStatus::NotCommittedFinal,
        WireEffectStatus::Reserved | WireEffectStatus::Started => {
            return boundary(AuthorizedExecutionRefusal::TransitionForbidden);
        }
        WireEffectStatus::StateUnknown => return EffectDecision::ContinuityBarrier,
    };

    EffectDecision::Apply(EffectApplication {
        status,
        effect_emission_id: attestation.effect_emission_id,
        attempt_id: attestation.attempt_id,
    })
}

const fn refused(refusal: EffectRefusal) -> EffectDecision {
    EffectDecision::Refused(refusal)
}

const fn boundary(refusal: AuthorizedExecutionRefusal) -> EffectDecision {
    EffectDecision::BoundaryRefused(refusal)
}
