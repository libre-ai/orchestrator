use std::fmt::{self, Debug, Display, Formatter};

use chrono::DateTime;
use libre_ai_contract_types::ContractRegistry;
use serde::Deserialize;
use serde_json::Value;

use super::AuthorizedExecutionRefusal;
use super::document::require_valid;

const DECISION_REQUEST_SCHEMA: &str = "human-decision-request.v1.schema.json";
const DECISION_RESPONSE_SCHEMA: &str = "human-decision-response.v1.schema.json";

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum DecisionObservation<'a> {
    Unavailable,
    Authoritative {
        request_replaced: bool,
        request_consumed: bool,
        actor_roles: &'a [&'a str],
        revision: u64,
        prior_response: Option<(&'a str, &'a str)>,
    },
}

impl<'a> DecisionObservation<'a> {
    pub const fn unavailable() -> Self {
        Self::Unavailable
    }

    pub const fn authoritative(
        request_replaced: bool,
        request_consumed: bool,
        actor_roles: &'a [&'a str],
        revision: u64,
        prior_response: Option<(&'a str, &'a str)>,
    ) -> Self {
        Self::Authoritative {
            request_replaced,
            request_consumed,
            actor_roles,
            revision,
            prior_response,
        }
    }
}

impl Debug for DecisionObservation<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Unavailable => "DecisionObservation::Unavailable",
            Self::Authoritative { .. } => "DecisionObservation::Authoritative(<redacted>)",
        })
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct DecisionApplication {
    request_digest: String,
    outcome_code: String,
    expected_revision: u64,
}

impl DecisionApplication {
    pub fn request_digest(&self) -> &str {
        &self.request_digest
    }

    pub fn outcome_code(&self) -> &str {
        &self.outcome_code
    }

    pub const fn expected_revision(&self) -> u64 {
        self.expected_revision
    }
}

impl Debug for DecisionApplication {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DecisionApplication")
            .field("code", &"decision-valid")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionRefusal {
    OrganizationMismatch,
    AttemptMismatch,
    RequestReplaced,
    DuplicateDivergent,
    RequestExpired,
    RequestConsumed,
    ChoiceUnknown,
    ActorUnauthorized,
    RevisionStale,
}

impl DecisionRefusal {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::OrganizationMismatch => "organization-mismatch",
            Self::AttemptMismatch => "attempt-mismatch",
            Self::RequestReplaced => "request-replaced",
            Self::DuplicateDivergent => "duplicate-divergent",
            Self::RequestExpired => "request-expired",
            Self::RequestConsumed => "request-consumed",
            Self::ChoiceUnknown => "choice-unknown",
            Self::ActorUnauthorized => "actor-unauthorized",
            Self::RevisionStale => "revision-stale",
        }
    }
}

impl Display for DecisionRefusal {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecisionDecision {
    Apply(DecisionApplication),
    Idempotent,
    Refused(DecisionRefusal),
    BoundaryRefused(AuthorizedExecutionRefusal),
}

impl DecisionDecision {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Apply(_) => "decision-valid",
            Self::Idempotent => "idempotent-duplicate",
            Self::Refused(refusal) => refusal.code(),
            Self::BoundaryRefused(refusal) => refusal.code(),
        }
    }

    pub const fn application(&self) -> Option<&DecisionApplication> {
        match self {
            Self::Apply(application) => Some(application),
            Self::Idempotent | Self::Refused(_) | Self::BoundaryRefused(_) => None,
        }
    }
}

impl Display for DecisionDecision {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireDecisionRequest {
    id: String,
    organization_id: String,
    mission_id: String,
    run_id: String,
    step_id: String,
    attempt_id: String,
    choices: Vec<WireDecisionChoice>,
    required_role: String,
    expected_revision: u64,
    expires_at: String,
    request_digest: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireDecisionChoice {
    choice_id: String,
    consequence_code: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireDecisionResponse {
    id: String,
    organization_id: String,
    mission_id: String,
    run_id: String,
    step_id: String,
    attempt_id: String,
    request_id: String,
    request_digest: String,
    choice_id: String,
    actor_authorization: WireActorAuthorization,
    expected_revision: u64,
    response_digest: String,
}

#[derive(Deserialize)]
struct WireActorAuthorization {
    role: String,
}

pub fn evaluate_human_decision(
    registry: &ContractRegistry,
    request_document: &Value,
    response_document: &Value,
    observation: DecisionObservation<'_>,
    evaluation_time: &str,
) -> DecisionDecision {
    if require_valid(registry, DECISION_REQUEST_SCHEMA, request_document).is_err()
        || require_valid(registry, DECISION_RESPONSE_SCHEMA, response_document).is_err()
    {
        return boundary(AuthorizedExecutionRefusal::SchemaInvalid);
    }
    let request: WireDecisionRequest = match serde_json::from_value(request_document.clone()) {
        Ok(request) => request,
        Err(_) => return boundary(AuthorizedExecutionRefusal::SchemaInvalid),
    };
    let response: WireDecisionResponse = match serde_json::from_value(response_document.clone()) {
        Ok(response) => response,
        Err(_) => return boundary(AuthorizedExecutionRefusal::SchemaInvalid),
    };
    let now = match DateTime::parse_from_rfc3339(evaluation_time) {
        Ok(now) => now,
        Err(_) => return boundary(AuthorizedExecutionRefusal::SchemaInvalid),
    };
    let expires_at = match DateTime::parse_from_rfc3339(&request.expires_at) {
        Ok(expires_at) => expires_at,
        Err(_) => return boundary(AuthorizedExecutionRefusal::SchemaInvalid),
    };
    let DecisionObservation::Authoritative {
        request_replaced,
        request_consumed,
        actor_roles,
        revision,
        prior_response,
    } = observation
    else {
        return boundary(AuthorizedExecutionRefusal::StoreUnavailable);
    };

    if request.organization_id != response.organization_id {
        return refused(DecisionRefusal::OrganizationMismatch);
    }
    if request.mission_id != response.mission_id
        || request.run_id != response.run_id
        || request.step_id != response.step_id
        || request.attempt_id != response.attempt_id
    {
        return refused(DecisionRefusal::AttemptMismatch);
    }
    if request_replaced
        || request.id != response.request_id
        || request.request_digest != response.request_digest
    {
        return refused(DecisionRefusal::RequestReplaced);
    }
    if let Some((prior_id, prior_digest)) = prior_response {
        if prior_id == response.id && prior_digest == response.response_digest {
            return DecisionDecision::Idempotent;
        }
        return refused(DecisionRefusal::DuplicateDivergent);
    }
    if now >= expires_at {
        return refused(DecisionRefusal::RequestExpired);
    }
    if request_consumed {
        return refused(DecisionRefusal::RequestConsumed);
    }
    let Some(choice) = request
        .choices
        .iter()
        .find(|choice| choice.choice_id == response.choice_id)
    else {
        return refused(DecisionRefusal::ChoiceUnknown);
    };
    if response.actor_authorization.role != request.required_role
        || !actor_roles.contains(&request.required_role.as_str())
    {
        return refused(DecisionRefusal::ActorUnauthorized);
    }
    if request.expected_revision != revision || response.expected_revision != revision {
        return refused(DecisionRefusal::RevisionStale);
    }

    DecisionDecision::Apply(DecisionApplication {
        request_digest: request.request_digest,
        outcome_code: choice.consequence_code.clone(),
        expected_revision: revision,
    })
}

const fn refused(refusal: DecisionRefusal) -> DecisionDecision {
    DecisionDecision::Refused(refusal)
}

const fn boundary(refusal: AuthorizedExecutionRefusal) -> DecisionDecision {
    DecisionDecision::BoundaryRefused(refusal)
}
