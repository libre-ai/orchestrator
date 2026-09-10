use std::fmt::{self, Debug, Display, Formatter};

use chrono::DateTime;
use libre_ai_contract_types::ContractRegistry;
use serde::Deserialize;
use serde_json::Value;

use super::AuthorizedExecutionRefusal;
use super::document::require_valid;

const EXECUTION_TRANSFER_SCHEMA: &str = "execution-transfer.v1.schema.json";

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum TransferObservation<'a> {
    Unavailable,
    Authoritative {
        organization_id: &'a str,
        mission_id: &'a str,
        predecessor_run_id: &'a str,
        predecessor_plan_digest: &'a str,
        successor_plan_digest: &'a str,
        current_generation: u64,
        revision: u64,
        generation_consumed: bool,
        prior_transfer: Option<(&'a str, u64, &'a str)>,
    },
}

impl<'a> TransferObservation<'a> {
    pub const fn unavailable() -> Self {
        Self::Unavailable
    }
}

impl Debug for TransferObservation<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Unavailable => "TransferObservation::Unavailable",
            Self::Authoritative { .. } => "TransferObservation::Authoritative(<redacted>)",
        })
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct TransferApplication {
    expected_seal_revision: u64,
    successor_generation: u64,
}

impl TransferApplication {
    pub const fn expected_seal_revision(&self) -> u64 {
        self.expected_seal_revision
    }

    pub const fn successor_generation(&self) -> u64 {
        self.successor_generation
    }
}

impl Debug for TransferApplication {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TransferApplication")
            .field("code", &"transfer-valid")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferRefusal {
    DuplicateDivergent,
    TransferExpired,
    GenerationConsumed,
    IdentityMismatch,
    GenerationStale,
    RevisionStale,
}

impl TransferRefusal {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::DuplicateDivergent => "duplicate-divergent",
            Self::TransferExpired => "transfer-expired",
            Self::GenerationConsumed => "generation-consumed",
            Self::IdentityMismatch => "identity-mismatch",
            Self::GenerationStale => "generation-stale",
            Self::RevisionStale => "revision-stale",
        }
    }
}

impl Display for TransferRefusal {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransferDecision {
    Apply(TransferApplication),
    Idempotent,
    Refused(TransferRefusal),
    BoundaryRefused(AuthorizedExecutionRefusal),
}

impl TransferDecision {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Apply(_) => "transfer-valid",
            Self::Idempotent => "idempotent-duplicate",
            Self::Refused(refusal) => refusal.code(),
            Self::BoundaryRefused(refusal) => refusal.code(),
        }
    }

    pub const fn application(&self) -> Option<&TransferApplication> {
        match self {
            Self::Apply(application) => Some(application),
            Self::Idempotent | Self::Refused(_) | Self::BoundaryRefused(_) => None,
        }
    }
}

impl Display for TransferDecision {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireTransfer {
    id: String,
    organization_id: String,
    mission_id: String,
    predecessor_run_id: String,
    predecessor_plan_digest: String,
    current_generation: u64,
    expected_revision: u64,
    successor_plan_digest: String,
    issued_at: String,
    expires_at: String,
    transfer_digest: String,
}

pub fn evaluate_execution_transfer(
    registry: &ContractRegistry,
    transfer_document: &Value,
    observation: TransferObservation<'_>,
    evaluation_time: &str,
) -> TransferDecision {
    if require_valid(registry, EXECUTION_TRANSFER_SCHEMA, transfer_document).is_err() {
        return boundary(AuthorizedExecutionRefusal::SchemaInvalid);
    }
    let transfer: WireTransfer = match serde_json::from_value(transfer_document.clone()) {
        Ok(transfer) => transfer,
        Err(_) => return boundary(AuthorizedExecutionRefusal::SchemaInvalid),
    };
    let now = match DateTime::parse_from_rfc3339(evaluation_time) {
        Ok(now) => now,
        Err(_) => return boundary(AuthorizedExecutionRefusal::SchemaInvalid),
    };
    let issued_at = match DateTime::parse_from_rfc3339(&transfer.issued_at) {
        Ok(issued_at) => issued_at,
        Err(_) => return boundary(AuthorizedExecutionRefusal::SchemaInvalid),
    };
    let expires_at = match DateTime::parse_from_rfc3339(&transfer.expires_at) {
        Ok(expires_at) if issued_at < expires_at => expires_at,
        Ok(_) | Err(_) => return boundary(AuthorizedExecutionRefusal::SchemaInvalid),
    };
    if now < issued_at {
        return boundary(AuthorizedExecutionRefusal::SchemaInvalid);
    }
    let TransferObservation::Authoritative {
        organization_id,
        mission_id,
        predecessor_run_id,
        predecessor_plan_digest,
        successor_plan_digest,
        current_generation,
        revision,
        generation_consumed,
        prior_transfer,
    } = observation
    else {
        return boundary(AuthorizedExecutionRefusal::StoreUnavailable);
    };

    if let Some((prior_id, prior_generation, prior_digest)) = prior_transfer {
        if prior_id == transfer.id
            && prior_generation == transfer.current_generation
            && prior_digest == transfer.transfer_digest
        {
            return TransferDecision::Idempotent;
        }
        return refused(TransferRefusal::DuplicateDivergent);
    }
    if now >= expires_at {
        return refused(TransferRefusal::TransferExpired);
    }
    if generation_consumed {
        return refused(TransferRefusal::GenerationConsumed);
    }
    if organization_id != transfer.organization_id
        || mission_id != transfer.mission_id
        || predecessor_run_id != transfer.predecessor_run_id
        || predecessor_plan_digest != transfer.predecessor_plan_digest
        || successor_plan_digest != transfer.successor_plan_digest
    {
        return refused(TransferRefusal::IdentityMismatch);
    }
    if current_generation != transfer.current_generation {
        return refused(TransferRefusal::GenerationStale);
    }
    if revision != transfer.expected_revision {
        return refused(TransferRefusal::RevisionStale);
    }
    let Some(successor_generation) = current_generation.checked_add(1) else {
        return boundary(AuthorizedExecutionRefusal::ArithmeticOverflow);
    };

    TransferDecision::Apply(TransferApplication {
        expected_seal_revision: revision,
        successor_generation,
    })
}

const fn refused(refusal: TransferRefusal) -> TransferDecision {
    TransferDecision::Refused(refusal)
}

const fn boundary(refusal: AuthorizedExecutionRefusal) -> TransferDecision {
    TransferDecision::BoundaryRefused(refusal)
}
