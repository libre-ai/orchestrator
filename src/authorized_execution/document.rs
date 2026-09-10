use std::fmt::{self, Debug, Formatter, Write};

use libre_ai_contract_types::ContractRegistry;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::AuthorizedExecutionRefusal;

const EXECUTION_GRAPH_SCHEMA: &str = "execution-graph.v1.schema.json";
const ORCHESTRATOR_EVENT_SCHEMA: &str = "orchestrator-event.v3.schema.json";

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

#[derive(Clone, Eq, PartialEq)]
pub struct AuthorizedExecutionEvent {
    pub(super) id: String,
    pub(super) organization_id: String,
    pub(super) mission_id: String,
    pub(super) run_id: String,
    pub(super) orchestrator_id: String,
    pub(super) plan_digest: String,
    pub(super) authorization_digest: String,
    pub(super) graph_digest: String,
    pub(super) generation: u64,
    pub(super) sequence: u64,
    pub(super) previous_event_digest: Option<String>,
    pub(super) event_digest: String,
    pub(super) step_id: Option<String>,
    pub(super) attempt_id: Option<String>,
    pub(super) worker_invocation_id: Option<String>,
    pub(super) selected_edge_id: Option<String>,
    pub(super) kind: EventKind,
    pub(super) budget_delta: BudgetCounters,
    pub(super) budget_total: BudgetCounters,
}

impl AuthorizedExecutionEvent {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn digest(&self) -> &str {
        &self.event_digest
    }
}

impl Debug for AuthorizedExecutionEvent {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthorizedExecutionEvent")
            .field("sequence", &self.sequence)
            .field("kind", &self.kind.code())
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(super) enum EventKind {
    GraphActivated { graph_id: String },
    StepAuthorized,
    InvocationStarted,
    StepResultRecorded { outcome_code: String },
    DecisionRequested,
    DecisionConsumed { outcome_code: String },
    EffectReserved { status: EffectStatus },
    EffectStarted { status: EffectStatus },
    EffectTerminal { status: EffectStatus },
    EffectUnknown { status: EffectStatus },
    PredecessorSealed,
    GenerationTransferred,
    RunBlocked,
    Quarantined,
    RunCompleted,
}

impl EventKind {
    fn code(&self) -> &'static str {
        match self {
            Self::GraphActivated { .. } => "graph-activated",
            Self::StepAuthorized => "step-authorized",
            Self::InvocationStarted => "invocation-started",
            Self::StepResultRecorded { .. } => "step-result-recorded",
            Self::DecisionRequested => "decision-requested",
            Self::DecisionConsumed { .. } => "decision-consumed",
            Self::EffectReserved { .. } => "effect-reserved",
            Self::EffectStarted { .. } => "effect-started",
            Self::EffectTerminal { .. } => "effect-terminal",
            Self::EffectUnknown { .. } => "effect-unknown",
            Self::PredecessorSealed => "predecessor-sealed",
            Self::GenerationTransferred => "generation-transferred",
            Self::RunBlocked => "run-blocked",
            Self::Quarantined => "quarantined",
            Self::RunCompleted => "run-completed",
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum EffectStatus {
    Reserved,
    Started,
    Committed,
    RejectedFinal,
    NotCommittedFinal,
    StateUnknown,
}

impl EffectStatus {
    pub(super) const fn outcome_code(self) -> Option<&'static str> {
        match self {
            Self::Committed => Some("effect-committed"),
            Self::RejectedFinal | Self::NotCommittedFinal => Some("effect-refused"),
            Self::Reserved | Self::Started | Self::StateUnknown => None,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) struct BudgetCounters {
    pub(super) values: [u64; 7],
}

impl BudgetCounters {
    pub(super) const fn tool_calls(self) -> u64 {
        self.values[1]
    }

    pub(super) fn checked_add(self, delta: Self) -> Option<Self> {
        let mut values = [0_u64; 7];
        for (index, value) in values.iter_mut().enumerate() {
            *value = self.values[index].checked_add(delta.values[index])?;
        }
        Some(Self { values })
    }

    pub(super) fn has_decreased_from(self, previous: Self) -> bool {
        self.values
            .iter()
            .zip(previous.values)
            .any(|(current, previous)| *current < previous)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireEvent {
    id: String,
    organization_id: String,
    mission_id: String,
    run_id: String,
    orchestrator_id: String,
    plan_digest: String,
    authorization_digest: String,
    graph_digest: String,
    generation: u64,
    sequence: u64,
    previous_event_digest: Option<String>,
    step_id: Option<String>,
    attempt_id: Option<String>,
    worker_invocation_id: Option<String>,
    selected_edge_id: Option<String>,
    #[serde(rename = "type")]
    kind: WireEventKind,
    budget_delta: WireBudgetCounters,
    budget_total: WireBudgetCounters,
    data: WireEventData,
    event_digest: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum WireEventKind {
    GraphActivated,
    StepAuthorized,
    InvocationStarted,
    StepResultRecorded,
    DecisionRequested,
    DecisionConsumed,
    EffectReserved,
    EffectStarted,
    EffectTerminal,
    EffectUnknown,
    PredecessorSealed,
    GenerationTransferred,
    RunBlocked,
    Quarantined,
    RunCompleted,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireBudgetCounters {
    duration_seconds: u64,
    tool_calls: u64,
    input_tokens: u64,
    output_tokens: u64,
    processes_started: u64,
    files_changed: u64,
    changed_bytes: u64,
}

impl From<WireBudgetCounters> for BudgetCounters {
    fn from(wire: WireBudgetCounters) -> Self {
        Self {
            values: [
                wire.duration_seconds,
                wire.tool_calls,
                wire.input_tokens,
                wire.output_tokens,
                wire.processes_started,
                wire.files_changed,
                wire.changed_bytes,
            ],
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireEventData {
    graph_ref: Option<WireEventArtifactReference>,
    outcome_code: Option<String>,
    effect_status: Option<WireEffectStatus>,
}

#[derive(Deserialize)]
struct WireEventArtifactReference {
    id: String,
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

pub fn parse_authorized_execution_event(
    registry: &ContractRegistry,
    document: &Value,
) -> Result<AuthorizedExecutionEvent, AuthorizedExecutionRefusal> {
    require_valid(registry, ORCHESTRATOR_EVENT_SCHEMA, document)?;
    let wire: WireEvent = serde_json::from_value(document.clone())
        .map_err(|_| AuthorizedExecutionRefusal::SchemaInvalid)?;
    let computed_digest = canonical_event_digest(document)?;
    if computed_digest != wire.event_digest {
        return Err(AuthorizedExecutionRefusal::SchemaInvalid);
    }

    let kind = normalize_event_kind(wire.kind, wire.data)?;
    Ok(AuthorizedExecutionEvent {
        id: wire.id,
        organization_id: wire.organization_id,
        mission_id: wire.mission_id,
        run_id: wire.run_id,
        orchestrator_id: wire.orchestrator_id,
        plan_digest: wire.plan_digest,
        authorization_digest: wire.authorization_digest,
        graph_digest: wire.graph_digest,
        generation: wire.generation,
        sequence: wire.sequence,
        previous_event_digest: wire.previous_event_digest,
        event_digest: wire.event_digest,
        step_id: wire.step_id,
        attempt_id: wire.attempt_id,
        worker_invocation_id: wire.worker_invocation_id,
        selected_edge_id: wire.selected_edge_id,
        kind,
        budget_delta: wire.budget_delta.into(),
        budget_total: wire.budget_total.into(),
    })
}

fn canonical_event_digest(document: &Value) -> Result<String, AuthorizedExecutionRefusal> {
    let mut unsigned = document.clone();
    let Some(object) = unsigned.as_object_mut() else {
        return Err(AuthorizedExecutionRefusal::SchemaInvalid);
    };
    object.remove("eventDigest");
    let canonical =
        serde_jcs::to_vec(&unsigned).map_err(|_| AuthorizedExecutionRefusal::SchemaInvalid)?;
    let mut digest = String::with_capacity(64);
    for byte in Sha256::digest(canonical) {
        write!(&mut digest, "{byte:02x}").map_err(|_| AuthorizedExecutionRefusal::SchemaInvalid)?;
    }
    Ok(digest)
}

fn normalize_event_kind(
    kind: WireEventKind,
    data: WireEventData,
) -> Result<EventKind, AuthorizedExecutionRefusal> {
    let missing = || AuthorizedExecutionRefusal::SchemaInvalid;
    Ok(match kind {
        WireEventKind::GraphActivated => EventKind::GraphActivated {
            graph_id: data.graph_ref.ok_or_else(missing)?.id,
        },
        WireEventKind::StepAuthorized => EventKind::StepAuthorized,
        WireEventKind::InvocationStarted => EventKind::InvocationStarted,
        WireEventKind::StepResultRecorded => EventKind::StepResultRecorded {
            outcome_code: data.outcome_code.ok_or_else(missing)?,
        },
        WireEventKind::DecisionRequested => EventKind::DecisionRequested,
        WireEventKind::DecisionConsumed => EventKind::DecisionConsumed {
            outcome_code: data.outcome_code.ok_or_else(missing)?,
        },
        WireEventKind::EffectReserved => EventKind::EffectReserved {
            status: normalize_effect_status(data.effect_status.ok_or_else(missing)?),
        },
        WireEventKind::EffectStarted => EventKind::EffectStarted {
            status: normalize_effect_status(data.effect_status.ok_or_else(missing)?),
        },
        WireEventKind::EffectTerminal => EventKind::EffectTerminal {
            status: normalize_effect_status(data.effect_status.ok_or_else(missing)?),
        },
        WireEventKind::EffectUnknown => EventKind::EffectUnknown {
            status: normalize_effect_status(data.effect_status.ok_or_else(missing)?),
        },
        WireEventKind::PredecessorSealed => EventKind::PredecessorSealed,
        WireEventKind::GenerationTransferred => EventKind::GenerationTransferred,
        WireEventKind::RunBlocked => EventKind::RunBlocked,
        WireEventKind::Quarantined => EventKind::Quarantined,
        WireEventKind::RunCompleted => EventKind::RunCompleted,
    })
}

const fn normalize_effect_status(status: WireEffectStatus) -> EffectStatus {
    match status {
        WireEffectStatus::Reserved => EffectStatus::Reserved,
        WireEffectStatus::Started => EffectStatus::Started,
        WireEffectStatus::Committed => EffectStatus::Committed,
        WireEffectStatus::RejectedFinal => EffectStatus::RejectedFinal,
        WireEffectStatus::NotCommittedFinal => EffectStatus::NotCommittedFinal,
        WireEffectStatus::StateUnknown => EffectStatus::StateUnknown,
    }
}
