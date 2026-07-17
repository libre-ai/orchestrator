use chrono::DateTime;
use libre_ai_contract_types::ContractRegistry;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write;

const CONTROL_SCHEMA: &str = "orchestrator-control.v1.schema.json";
const CONTROL_VERSION: &str = "libre-ai.orchestrator-control.v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ControlAction {
    Start,
    Pause,
    Resume,
    Cancel,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlCommand {
    schema_version: String,
    id: String,
    tenant_id: String,
    mission_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    run_id: Option<String>,
    plan_digest: String,
    authorization_digest: String,
    action: ControlAction,
    expected_revision: u64,
    idempotency_key: String,
    reason_code: String,
    issued_at: String,
    expires_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ControlCommandWire {
    schema_version: String,
    id: String,
    tenant_id: String,
    mission_id: String,
    run_id: Option<String>,
    plan_digest: String,
    authorization_digest: String,
    action: ControlAction,
    expected_revision: u64,
    idempotency_key: String,
    reason_code: String,
    issued_at: String,
    expires_at: String,
}

impl From<ControlCommandWire> for ControlCommand {
    fn from(wire: ControlCommandWire) -> Self {
        Self {
            schema_version: wire.schema_version,
            id: wire.id,
            tenant_id: wire.tenant_id,
            mission_id: wire.mission_id,
            run_id: wire.run_id,
            plan_digest: wire.plan_digest,
            authorization_digest: wire.authorization_digest,
            action: wire.action,
            expected_revision: wire.expected_revision,
            idempotency_key: wire.idempotency_key,
            reason_code: wire.reason_code,
            issued_at: wire.issued_at,
            expires_at: wire.expires_at,
        }
    }
}

impl ControlCommand {
    #[must_use]
    pub const fn action(&self) -> ControlAction {
        self.action
    }

    #[must_use]
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlPhase {
    Running,
    Blocked,
    Paused,
    Cancelled,
    ResultSubmitted,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunControlState {
    pub tenant_id: String,
    pub mission_id: String,
    pub run_id: String,
    pub plan_digest: String,
    pub authorization_digest: String,
    pub authorization_active: bool,
    pub authorization_store_available: bool,
    pub revision: u64,
    pub phase: ControlPhase,
}

/// Authenticated, caller-supplied facts produced by Missions and specialized verifiers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartPreflight {
    pub tenant_id: String,
    pub mission_id: String,
    pub plan_digest: String,
    pub authorization_digest: String,
    pub authoritative_revision: u64,
    pub plan_valid: bool,
    pub authorization_valid: bool,
    pub authorization_active: bool,
    pub quorum_valid: bool,
    pub biscuit_allowed: bool,
    pub harness_attestation_valid: bool,
    pub required_controls_effective: bool,
    pub authorization_store_available: bool,
    pub causal_store_available: bool,
    pub key_registry_available: bool,
}

impl StartPreflight {
    fn ready(&self) -> bool {
        self.plan_valid
            && self.authorization_valid
            && self.authorization_active
            && self.quorum_valid
            && self.biscuit_allowed
            && self.harness_attestation_valid
            && self.required_controls_effective
            && self.authorization_store_available
            && self.causal_store_available
            && self.key_registry_available
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlEffect {
    AllocateRun,
    Pause,
    Resume,
    Cancel,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlApplication {
    pub effect: ControlEffect,
    pub next_revision: u64,
}

/// Trusted observation returned by the caller's idempotency store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandReceipt {
    pub idempotency_key: String,
    pub command_fingerprint: String,
    pub application: ControlApplication,
}

#[derive(Clone, Copy, Debug)]
pub enum CommandCollisionObservation<'a> {
    NoCollision,
    Existing(&'a CommandReceipt),
    StoreUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SimulatedEffectDecision {
    Allow,
    Refuse,
}

impl SimulatedEffectDecision {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Allow => "orchestrator.effect.allow",
            Self::Refuse => "orchestrator.effect.refuse",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlRefusal {
    SchemaInvalid,
    FingerprintInvalid,
    TimeInvalid,
    Expired,
    IdempotencyStoreInvalid,
    IdempotencyStoreUnavailable,
    IdempotencyConflict,
    AuthorizationStoreUnavailable,
    AuthorizationRevoked,
    RunIdInvalid,
    StateMissing,
    IdentityMismatch,
    StaleRevision,
    PreflightMissing,
    PreflightFailed,
    TransitionForbidden,
    RevisionOverflow,
}

impl ControlRefusal {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::SchemaInvalid => "orchestrator.control.schema-invalid",
            Self::FingerprintInvalid => "orchestrator.control.fingerprint-invalid",
            Self::TimeInvalid => "orchestrator.control.time-invalid",
            Self::Expired => "orchestrator.control.expired",
            Self::IdempotencyStoreInvalid => "orchestrator.control.idempotency-store-invalid",
            Self::IdempotencyStoreUnavailable => {
                "orchestrator.control.idempotency-store-unavailable"
            }
            Self::IdempotencyConflict => "orchestrator.control.idempotency-conflict",
            Self::AuthorizationStoreUnavailable => {
                "orchestrator.control.authorization-store-unavailable"
            }
            Self::AuthorizationRevoked => "orchestrator.control.authorization-revoked",
            Self::RunIdInvalid => "orchestrator.control.run-id-invalid",
            Self::StateMissing => "orchestrator.control.state-missing",
            Self::IdentityMismatch => "orchestrator.control.identity-mismatch",
            Self::StaleRevision => "orchestrator.control.stale-revision",
            Self::PreflightMissing => "orchestrator.control.preflight-missing",
            Self::PreflightFailed => "orchestrator.control.preflight-failed",
            Self::TransitionForbidden => "orchestrator.control.transition-forbidden",
            Self::RevisionOverflow => "orchestrator.control.revision-overflow",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControlDecision {
    Apply(ControlApplication),
    Idempotent { recorded_next_revision: u64 },
    Refuse(ControlRefusal),
}

impl ControlDecision {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Apply(_) => "orchestrator.control.apply",
            Self::Idempotent { .. } => "orchestrator.control.idempotent",
            Self::Refuse(refusal) => refusal.code(),
        }
    }
}

pub fn parse_control_document(
    registry: &ContractRegistry,
    document: &Value,
) -> Result<ControlCommand, ControlRefusal> {
    let issues = registry
        .validate(CONTROL_SCHEMA, document)
        .map_err(|_| ControlRefusal::SchemaInvalid)?;
    if !issues.is_empty() {
        return Err(ControlRefusal::SchemaInvalid);
    }
    let wire: ControlCommandWire =
        serde_json::from_value(document.clone()).map_err(|_| ControlRefusal::SchemaInvalid)?;
    Ok(wire.into())
}

pub fn command_fingerprint(command: &ControlCommand) -> Result<String, ControlRefusal> {
    let canonical = serde_jcs::to_vec(command).map_err(|_| ControlRefusal::FingerprintInvalid)?;
    let mut fingerprint = String::with_capacity(64);
    for byte in Sha256::digest(canonical) {
        write!(&mut fingerprint, "{byte:02x}").map_err(|_| ControlRefusal::FingerprintInvalid)?;
    }
    Ok(fingerprint)
}

fn command_time_valid(
    command: &ControlCommand,
    evaluation_time: &str,
) -> Result<(), ControlRefusal> {
    let evaluation =
        DateTime::parse_from_rfc3339(evaluation_time).map_err(|_| ControlRefusal::TimeInvalid)?;
    let issued = DateTime::parse_from_rfc3339(&command.issued_at)
        .map_err(|_| ControlRefusal::TimeInvalid)?;
    let expires = DateTime::parse_from_rfc3339(&command.expires_at)
        .map_err(|_| ControlRefusal::TimeInvalid)?;
    if issued >= expires || evaluation < issued {
        return Err(ControlRefusal::TimeInvalid);
    }
    if evaluation >= expires {
        return Err(ControlRefusal::Expired);
    }
    Ok(())
}

fn same_preflight_authority(preflight: &StartPreflight, command: &ControlCommand) -> bool {
    command.tenant_id == preflight.tenant_id
        && command.mission_id == preflight.mission_id
        && command.plan_digest == preflight.plan_digest
        && command.authorization_digest == preflight.authorization_digest
}

fn same_authority(state: &RunControlState, command: &ControlCommand) -> bool {
    command.tenant_id == state.tenant_id
        && command.mission_id == state.mission_id
        && command.run_id.as_deref() == Some(state.run_id.as_str())
        && command.plan_digest == state.plan_digest
        && command.authorization_digest == state.authorization_digest
}

fn next_revision(revision: u64) -> Result<u64, ControlRefusal> {
    revision
        .checked_add(1)
        .ok_or(ControlRefusal::RevisionOverflow)
}

/// Allows a simulated effect only for an active authorization in a running state.
#[must_use]
pub const fn evaluate_simulated_effect(state: &RunControlState) -> SimulatedEffectDecision {
    if matches!(state.phase, ControlPhase::Running)
        && state.authorization_store_available
        && state.authorization_active
    {
        SimulatedEffectDecision::Allow
    } else {
        SimulatedEffectDecision::Refuse
    }
}

/// Evaluates a schema-validated command against authenticated facts without performing effects.
/// The caller owns receipt persistence, run-ID allocation and every external capability.
#[must_use]
pub fn evaluate_control(
    state: Option<&RunControlState>,
    command: &ControlCommand,
    evaluation_time: &str,
    preflight: Option<&StartPreflight>,
    collision: CommandCollisionObservation<'_>,
) -> ControlDecision {
    if command.schema_version != CONTROL_VERSION {
        return ControlDecision::Refuse(ControlRefusal::SchemaInvalid);
    }

    let fingerprint = match command_fingerprint(command) {
        Ok(fingerprint) => fingerprint,
        Err(refusal) => return ControlDecision::Refuse(refusal),
    };
    match collision {
        CommandCollisionObservation::NoCollision => {}
        CommandCollisionObservation::StoreUnavailable => {
            return ControlDecision::Refuse(ControlRefusal::IdempotencyStoreUnavailable);
        }
        CommandCollisionObservation::Existing(receipt) => {
            if receipt.idempotency_key != command.idempotency_key {
                return ControlDecision::Refuse(ControlRefusal::IdempotencyStoreInvalid);
            }
            return if receipt.command_fingerprint == fingerprint {
                ControlDecision::Idempotent {
                    recorded_next_revision: receipt.application.next_revision,
                }
            } else {
                ControlDecision::Refuse(ControlRefusal::IdempotencyConflict)
            };
        }
    }

    if let Err(refusal) = command_time_valid(command, evaluation_time) {
        return ControlDecision::Refuse(refusal);
    }

    if command.action == ControlAction::Start {
        if command.run_id.is_some() || state.is_some() {
            return ControlDecision::Refuse(ControlRefusal::RunIdInvalid);
        }
        let Some(preflight) = preflight else {
            return ControlDecision::Refuse(ControlRefusal::PreflightMissing);
        };
        if !same_preflight_authority(preflight, command) {
            return ControlDecision::Refuse(ControlRefusal::IdentityMismatch);
        }
        if !preflight.ready() {
            return ControlDecision::Refuse(ControlRefusal::PreflightFailed);
        }
        if command.expected_revision != preflight.authoritative_revision {
            return ControlDecision::Refuse(ControlRefusal::StaleRevision);
        }
        return match next_revision(preflight.authoritative_revision) {
            Ok(next_revision) => ControlDecision::Apply(ControlApplication {
                effect: ControlEffect::AllocateRun,
                next_revision,
            }),
            Err(refusal) => ControlDecision::Refuse(refusal),
        };
    }

    let Some(state) = state else {
        return ControlDecision::Refuse(ControlRefusal::StateMissing);
    };
    if !same_authority(state, command) {
        return ControlDecision::Refuse(ControlRefusal::IdentityMismatch);
    }
    if !state.authorization_store_available {
        return ControlDecision::Refuse(ControlRefusal::AuthorizationStoreUnavailable);
    }
    if !state.authorization_active {
        return ControlDecision::Refuse(ControlRefusal::AuthorizationRevoked);
    }

    let effect = match command.action {
        ControlAction::Start => unreachable!("start returned before state evaluation"),
        ControlAction::Pause => {
            if command.expected_revision != state.revision {
                return ControlDecision::Refuse(ControlRefusal::StaleRevision);
            }
            if !matches!(state.phase, ControlPhase::Running | ControlPhase::Blocked) {
                return ControlDecision::Refuse(ControlRefusal::TransitionForbidden);
            }
            ControlEffect::Pause
        }
        ControlAction::Resume => {
            if command.expected_revision != state.revision {
                return ControlDecision::Refuse(ControlRefusal::StaleRevision);
            }
            if !matches!(state.phase, ControlPhase::Paused | ControlPhase::Blocked) {
                return ControlDecision::Refuse(ControlRefusal::TransitionForbidden);
            }
            ControlEffect::Resume
        }
        ControlAction::Cancel => {
            if !matches!(
                state.phase,
                ControlPhase::Running | ControlPhase::Blocked | ControlPhase::Paused
            ) {
                return ControlDecision::Refuse(ControlRefusal::TransitionForbidden);
            }
            ControlEffect::Cancel
        }
    };

    match next_revision(state.revision) {
        Ok(next_revision) => ControlDecision::Apply(ControlApplication {
            effect,
            next_revision,
        }),
        Err(refusal) => ControlDecision::Refuse(refusal),
    }
}
