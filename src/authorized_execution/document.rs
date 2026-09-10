use std::fmt::{self, Debug, Formatter};

use libre_ai_contract_types::ContractRegistry;
use serde::Deserialize;
use serde_json::Value;

use super::AuthorizedExecutionRefusal;

const EXECUTION_GRAPH_SCHEMA: &str = "execution-graph.v1.schema.json";

#[derive(Clone, Eq, PartialEq)]
pub struct AuthorizedGraph {
    pub(super) id: String,
    pub(super) organization_id: String,
    pub(super) entry_step_id: String,
    pub(super) graph_digest: String,
    pub(super) steps: Vec<AuthorizedStep>,
    pub(super) edges: Vec<AuthorizedEdge>,
}

impl Debug for AuthorizedGraph {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthorizedGraph")
            .field("step_count", &self.steps.len())
            .field("edge_count", &self.edges.len())
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct AuthorizedStep {
    pub(super) step_id: String,
    pub(super) kind: StepKind,
    pub(super) outcome_codes: Vec<String>,
    pub(super) retry_policy: Option<RetryPolicy>,
    pub(super) decision_policy: Option<DecisionPolicy>,
    pub(super) effect_policy: Option<EffectPolicy>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum StepKind {
    Calculation,
    HumanDecision,
    ExternalEffect,
    Terminal,
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct RetryPolicy {
    pub(super) maximum_attempts: u64,
    pub(super) retryable_outcome_codes: Vec<String>,
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct DecisionPolicy {
    pub(super) choices: Vec<DecisionChoice>,
    pub(super) no_response_outcome_code: String,
    pub(super) request_schema_digest: String,
    pub(super) response_schema_digest: String,
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct DecisionChoice {
    pub(super) choice_id: String,
    pub(super) outcome_code: String,
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct EffectPolicy {
    pub(super) executor_profile_digest: String,
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct AuthorizedEdge {
    pub(super) edge_id: String,
    pub(super) from_step_id: String,
    pub(super) outcome_code: String,
    pub(super) to_step_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireGraph {
    id: String,
    organization_id: String,
    entry_step_id: String,
    graph_digest: String,
    steps: Vec<WireStep>,
    edges: Vec<WireEdge>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireStep {
    step_id: String,
    kind: WireStepKind,
    outcome_codes: Vec<String>,
    retry_policy: Option<WireRetryPolicy>,
    decision_policy: Option<WireDecisionPolicy>,
    effect_policy: Option<WireEffectPolicy>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum WireStepKind {
    Calculation,
    HumanDecision,
    ExternalEffect,
    Terminal,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireRetryPolicy {
    maximum_attempts: u64,
    retryable_outcome_codes: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireDecisionPolicy {
    choices: Vec<WireDecisionChoice>,
    no_response_outcome_code: String,
    request_schema_ref: WireArtifactReference,
    response_schema_ref: WireArtifactReference,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireDecisionChoice {
    choice_id: String,
    outcome_code: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireEffectPolicy {
    executor_profile_digest: String,
}

#[derive(Deserialize)]
struct WireArtifactReference {
    digest: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireEdge {
    edge_id: String,
    from_step_id: String,
    outcome_code: String,
    to_step_id: String,
}

pub(super) fn require_valid(
    registry: &ContractRegistry,
    schema_name: &'static str,
    value: &Value,
) -> Result<(), AuthorizedExecutionRefusal> {
    match registry.is_valid(schema_name, value) {
        Ok(true) => Ok(()),
        Ok(false) | Err(_) => Err(AuthorizedExecutionRefusal::SchemaInvalid),
    }
}

pub fn parse_authorized_graph(
    registry: &ContractRegistry,
    document: &Value,
) -> Result<AuthorizedGraph, AuthorizedExecutionRefusal> {
    require_valid(registry, EXECUTION_GRAPH_SCHEMA, document)?;
    let wire: WireGraph = serde_json::from_value(document.clone())
        .map_err(|_| AuthorizedExecutionRefusal::SchemaInvalid)?;

    Ok(AuthorizedGraph {
        id: wire.id,
        organization_id: wire.organization_id,
        entry_step_id: wire.entry_step_id,
        graph_digest: wire.graph_digest,
        steps: wire.steps.into_iter().map(AuthorizedStep::from).collect(),
        edges: wire.edges.into_iter().map(AuthorizedEdge::from).collect(),
    })
}

impl From<WireStep> for AuthorizedStep {
    fn from(wire: WireStep) -> Self {
        Self {
            step_id: wire.step_id,
            kind: match wire.kind {
                WireStepKind::Calculation => StepKind::Calculation,
                WireStepKind::HumanDecision => StepKind::HumanDecision,
                WireStepKind::ExternalEffect => StepKind::ExternalEffect,
                WireStepKind::Terminal => StepKind::Terminal,
            },
            outcome_codes: wire.outcome_codes,
            retry_policy: wire.retry_policy.map(|policy| RetryPolicy {
                maximum_attempts: policy.maximum_attempts,
                retryable_outcome_codes: policy.retryable_outcome_codes,
            }),
            decision_policy: wire.decision_policy.map(|policy| DecisionPolicy {
                choices: policy
                    .choices
                    .into_iter()
                    .map(|choice| DecisionChoice {
                        choice_id: choice.choice_id,
                        outcome_code: choice.outcome_code,
                    })
                    .collect(),
                no_response_outcome_code: policy.no_response_outcome_code,
                request_schema_digest: policy.request_schema_ref.digest,
                response_schema_digest: policy.response_schema_ref.digest,
            }),
            effect_policy: wire.effect_policy.map(|policy| EffectPolicy {
                executor_profile_digest: policy.executor_profile_digest,
            }),
        }
    }
}

impl From<WireEdge> for AuthorizedEdge {
    fn from(wire: WireEdge) -> Self {
        Self {
            edge_id: wire.edge_id,
            from_step_id: wire.from_step_id,
            outcome_code: wire.outcome_code,
            to_step_id: wire.to_step_id,
        }
    }
}
