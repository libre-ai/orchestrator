#![forbid(unsafe_code)]

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use libre_ai_contract_types::ContractRegistry;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter, Write as _};

pub const PROFILE_VERSION: &str = "libre-ai.agent-orchestrator.g2-simulator.v1";
pub const ORCHESTRATOR_ID: &str = "g2_simulator_v1";
pub const MAX_HANDOFF_BYTES: usize = 64 * 1024;
pub const MAX_EVENT_BYTES: usize = 16 * 1024;
pub const MAX_EVENT_LINES: usize = 6;
pub const MAX_ACCEPTED_EVENTS: usize = 3;
pub const MAX_EVENT_STREAM_BYTES: usize = MAX_EVENT_BYTES * MAX_EVENT_LINES;

const HANDOFF_SCHEMA: &str = "agent-handoff.v1.schema.json";
const EVENT_SCHEMA: &str = "orchestrator-event.v1.schema.json";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scenario {
    Complete,
    Blocked,
    Failed,
}

impl Scenario {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "complete" => Some(Self::Complete),
            "blocked" => Some(Self::Blocked),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkBudget {
    None,
    Allowlisted,
}

impl NetworkBudget {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "allowlisted" => Some(Self::Allowlisted),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MissionBudgets {
    pub max_duration_seconds: u64,
    pub max_tool_calls: u64,
    pub network: NetworkBudget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MissionContext {
    pub mission_id: String,
    pub started_at: DateTime<Utc>,
    pub budgets: MissionBudgets,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalReference {
    pub id: String,
    pub digest: String,
    pub media_type: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimulationRequest {
    pub context: MissionContext,
    pub scenario: Scenario,
    pub artifact: Option<CanonicalReference>,
    pub evidence: Option<CanonicalReference>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventStreamSummary {
    pub accepted_events: usize,
    pub replayed_events: usize,
    pub cursor: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCode {
    UsageInvalid,
    InputUnavailable,
    InputTooLarge,
    ContractInvalid,
    InternalContractUnavailable,
    HandoffWindowInvalid,
    HandoffExpired,
    BudgetInvalid,
    G2NetworkUnsupported,
    ResultReferenceMissing,
    ResultReferenceUnexpected,
    EventTypeForbidden,
    EventContextMismatch,
    EventIdInvalid,
    EventSequenceInvalid,
    EventTransitionInvalid,
    IdempotencyConflict,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UsageInvalid => "orchestrator.usage_invalid",
            Self::InputUnavailable => "orchestrator.input_unavailable",
            Self::InputTooLarge => "orchestrator.input_too_large",
            Self::ContractInvalid => "orchestrator.contract_invalid",
            Self::InternalContractUnavailable => "orchestrator.internal_contract_unavailable",
            Self::HandoffWindowInvalid => "orchestrator.handoff_window_invalid",
            Self::HandoffExpired => "orchestrator.handoff_expired",
            Self::BudgetInvalid => "orchestrator.budget_invalid",
            Self::G2NetworkUnsupported => "orchestrator.g2_network_unsupported",
            Self::ResultReferenceMissing => "orchestrator.result_reference_missing",
            Self::ResultReferenceUnexpected => "orchestrator.result_reference_unexpected",
            Self::EventTypeForbidden => "orchestrator.event_type_forbidden",
            Self::EventContextMismatch => "orchestrator.event_context_mismatch",
            Self::EventIdInvalid => "orchestrator.event_id_invalid",
            Self::EventSequenceInvalid => "orchestrator.event_sequence_invalid",
            Self::EventTransitionInvalid => "orchestrator.event_transition_invalid",
            Self::IdempotencyConflict => "orchestrator.idempotency_conflict",
        }
    }

    pub const fn exit_status(self) -> i32 {
        match self {
            Self::UsageInvalid => 64,
            Self::InputUnavailable | Self::InputTooLarge | Self::ContractInvalid => 65,
            Self::InternalContractUnavailable => 70,
            _ => 66,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OrchestratorError(ErrorCode);

impl OrchestratorError {
    pub const fn new(code: ErrorCode) -> Self {
        Self(code)
    }
    pub const fn code(self) -> ErrorCode {
        self.0
    }
    pub const fn exit_status(self) -> i32 {
        self.0.exit_status()
    }
}

impl Display for OrchestratorError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.as_str())
    }
}
impl Error for OrchestratorError {}

type Result<T> = std::result::Result<T, OrchestratorError>;

#[derive(Clone, Debug)]
struct HandoffContext {
    tenant_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Initial,
    Running,
    Blocked,
    Waiting,
    Terminal,
}

pub fn simulate_v1(handoff_json: &[u8], request: &SimulationRequest) -> Result<Vec<Value>> {
    let registry = registry()?;
    let handoff = validate_handoff(&registry, handoff_json, &request.context)?;
    validate_request(request)?;

    let mut events = vec![make_event(
        &handoff.tenant_id,
        &request.context,
        1,
        "started",
        0,
        json!({"progressPermille": 0}),
    )?];

    if matches!(request.scenario, Scenario::Complete | Scenario::Blocked)
        && request.context.budgets.max_duration_seconds < 2
    {
        events.push(make_event(
            &handoff.tenant_id,
            &request.context,
            2,
            "budget-exceeded",
            request.context.budgets.max_duration_seconds,
            json!({"reasonCode": "orchestrator.duration_budget_exceeded"}),
        )?);
        validate_values(&registry, &handoff, &request.context, &events)?;
        return Ok(events);
    }

    match request.scenario {
        Scenario::Complete => {
            let artifact = request
                .artifact
                .as_ref()
                .ok_or_else(|| err(ErrorCode::ResultReferenceMissing))?;
            let evidence = request
                .evidence
                .as_ref()
                .ok_or_else(|| err(ErrorCode::ResultReferenceMissing))?;
            events.push(make_event(
                &handoff.tenant_id,
                &request.context,
                2,
                "progressed",
                1,
                json!({"progressPermille": 500}),
            )?);
            events.push(make_event(
                &handoff.tenant_id,
                &request.context,
                3,
                "result-submitted",
                2,
                json!({
                    "artifact": reference_value(artifact),
                    "evidence": reference_value(evidence),
                    "progressPermille": 1000
                }),
            )?);
        }
        Scenario::Blocked => {
            events.push(make_event(
                &handoff.tenant_id,
                &request.context,
                2,
                "blocked",
                1,
                json!({"reasonCode": "orchestrator.simulated_block"}),
            )?);
            events.push(make_event(
                &handoff.tenant_id,
                &request.context,
                3,
                "decision-requested",
                2,
                json!({
                    "decisionRequestId": decision_id(&handoff.tenant_id, &request.context.mission_id, 3),
                    "reasonCode": "orchestrator.human_decision_required"
                }),
            )?);
        }
        Scenario::Failed => events.push(make_event(
            &handoff.tenant_id,
            &request.context,
            2,
            "failed",
            1,
            json!({"reasonCode": "orchestrator.simulated_failure"}),
        )?),
    }

    validate_values(&registry, &handoff, &request.context, &events)?;
    Ok(events)
}

pub fn validate_event_values_v1(
    handoff_json: &[u8],
    context: &MissionContext,
    events: &[Value],
) -> Result<EventStreamSummary> {
    let registry = registry()?;
    let handoff = validate_handoff(&registry, handoff_json, context)?;
    validate_values(&registry, &handoff, context, events)
}

pub fn validate_event_stream_v1(
    handoff_json: &[u8],
    context: &MissionContext,
    stream: &[u8],
) -> Result<EventStreamSummary> {
    if stream.len() > MAX_EVENT_STREAM_BYTES {
        return Err(err(ErrorCode::InputTooLarge));
    }
    let mut events = Vec::new();
    let mut lines = stream.split(|byte| *byte == b'\n').peekable();
    while let Some(line) = lines.next() {
        if line.is_empty() && lines.peek().is_none() {
            break;
        }
        if line.is_empty() {
            return Err(err(ErrorCode::ContractInvalid));
        }
        if line.len() > MAX_EVENT_BYTES || events.len() == MAX_EVENT_LINES {
            return Err(err(ErrorCode::InputTooLarge));
        }
        events.push(serde_json::from_slice(line).map_err(|_| err(ErrorCode::ContractInvalid))?);
    }
    validate_event_values_v1(handoff_json, context, &events)
}

pub fn encode_ndjson_v1(events: &[Value]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    for event in events {
        let encoded =
            serde_json::to_vec(event).map_err(|_| err(ErrorCode::InternalContractUnavailable))?;
        if encoded.len() > MAX_EVENT_BYTES {
            return Err(err(ErrorCode::InputTooLarge));
        }
        output.extend_from_slice(&encoded);
        output.push(b'\n');
    }
    Ok(output)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(&mut output, "{byte:02x}").expect("String writes cannot fail");
    }
    output
}

pub fn event_id(tenant: &str, mission: &str, sequence: u64, event_type: &str) -> String {
    let material = format!(
        "libre-ai.orchestrator-event.v1\n{tenant}\n{mission}\n{ORCHESTRATOR_ID}\n{sequence}\n{event_type}\n"
    );
    format!("urn:libre-ai:event:{}", sha256_hex(material.as_bytes()))
}

pub fn idempotency_key(id: &str) -> Option<String> {
    let digest = id.strip_prefix("urn:libre-ai:event:")?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    Some(format!("idem_{digest}"))
}

fn registry() -> Result<ContractRegistry> {
    ContractRegistry::embedded().map_err(|_| err(ErrorCode::InternalContractUnavailable))
}

fn validate_handoff(
    registry: &ContractRegistry,
    bytes: &[u8],
    context: &MissionContext,
) -> Result<HandoffContext> {
    if bytes.len() > MAX_HANDOFF_BYTES {
        return Err(err(ErrorCode::InputTooLarge));
    }
    validate_context(context)?;
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| err(ErrorCode::ContractInvalid))?;
    validate_contract(registry, HANDOFF_SCHEMA, &value)?;
    let created = parse_time(string(&value, "createdAt")?)?;
    let expires = parse_time(string(&value, "expiresAt")?)?;
    if created >= expires || context.started_at < created {
        return Err(err(ErrorCode::HandoffWindowInvalid));
    }
    if context.started_at >= expires {
        return Err(err(ErrorCode::HandoffExpired));
    }
    Ok(HandoffContext {
        tenant_id: string(&value, "tenantId")?.to_owned(),
    })
}

fn validate_context(context: &MissionContext) -> Result<()> {
    if context.started_at.timestamp_subsec_nanos() != 0
        || !(1..=604_800).contains(&context.budgets.max_duration_seconds)
        || context.budgets.max_tool_calls > 100_000
    {
        return Err(err(ErrorCode::BudgetInvalid));
    }
    if context.budgets.network != NetworkBudget::None {
        return Err(err(ErrorCode::G2NetworkUnsupported));
    }
    Ok(())
}

fn validate_request(request: &SimulationRequest) -> Result<()> {
    match request.scenario {
        Scenario::Complete if request.artifact.is_none() || request.evidence.is_none() => {
            Err(err(ErrorCode::ResultReferenceMissing))
        }
        Scenario::Blocked | Scenario::Failed
            if request.artifact.is_some() || request.evidence.is_some() =>
        {
            Err(err(ErrorCode::ResultReferenceUnexpected))
        }
        _ => Ok(()),
    }
}

fn validate_contract(registry: &ContractRegistry, schema: &str, value: &Value) -> Result<()> {
    let issues = registry
        .validate(schema, value)
        .map_err(|_| err(ErrorCode::InternalContractUnavailable))?;
    if issues.is_empty() {
        Ok(())
    } else {
        Err(err(ErrorCode::ContractInvalid))
    }
}

fn validate_values(
    registry: &ContractRegistry,
    handoff: &HandoffContext,
    context: &MissionContext,
    events: &[Value],
) -> Result<EventStreamSummary> {
    if events.is_empty() {
        return Err(err(ErrorCode::EventTransitionInvalid));
    }
    if events.len() > MAX_EVENT_LINES {
        return Err(err(ErrorCode::InputTooLarge));
    }

    let mut accepted = BTreeMap::<u64, Value>::new();
    let mut state = State::Initial;
    let mut progress = 0;
    let mut cursor = 0;
    let mut replayed = 0;

    for event in events {
        validate_contract(registry, EVENT_SCHEMA, event)?;
        let sequence = integer(event, "sequence")?;
        if sequence == 0 || sequence > MAX_ACCEPTED_EVENTS as u64 {
            return Err(err(ErrorCode::EventSequenceInvalid));
        }
        let event_type = string(event, "type")?;
        if string(event, "tenantId")? != handoff.tenant_id
            || string(event, "missionId")? != context.mission_id
            || string(event, "orchestratorId")? != ORCHESTRATOR_ID
        {
            return Err(err(ErrorCode::EventContextMismatch));
        }

        if let Some(previous) = accepted.get(&sequence) {
            if previous == event {
                replayed += 1;
                continue;
            }
            return Err(err(ErrorCode::IdempotencyConflict));
        }

        if string(event, "id")?
            != event_id(
                &handoff.tenant_id,
                &context.mission_id,
                sequence,
                event_type,
            )
        {
            return Err(err(ErrorCode::EventIdInvalid));
        }
        let elapsed = sequence - 1;
        if parse_time(string(event, "occurredAt")?)? != timestamp(context, elapsed)?
            || elapsed > context.budgets.max_duration_seconds
        {
            return Err(err(ErrorCode::EventTransitionInvalid));
        }
        if sequence != cursor + 1 {
            return Err(err(ErrorCode::EventSequenceInvalid));
        }

        let data = event
            .get("data")
            .and_then(Value::as_object)
            .ok_or_else(|| err(ErrorCode::ContractInvalid))?;
        (state, progress) = transition(
            state, progress, event_type, data, sequence, context, handoff,
        )?;
        cursor = sequence;
        accepted.insert(sequence, event.clone());
    }

    Ok(EventStreamSummary {
        accepted_events: accepted.len(),
        replayed_events: replayed,
        cursor,
    })
}

fn transition(
    state: State,
    progress: u64,
    kind: &str,
    data: &Map<String, Value>,
    sequence: u64,
    context: &MissionContext,
    handoff: &HandoffContext,
) -> Result<(State, u64)> {
    match kind {
        "started" if state == State::Initial => {
            keys(data, &["progressPermille"])?;
            require(data.get("progressPermille").and_then(Value::as_u64) == Some(0))?;
            Ok((State::Running, 0))
        }
        "progressed" if state == State::Running => {
            keys(data, &["progressPermille"])?;
            let next = data
                .get("progressPermille")
                .and_then(Value::as_u64)
                .ok_or_else(|| err(ErrorCode::EventTransitionInvalid))?;
            require(next > progress && next < 1000)?;
            Ok((State::Running, next))
        }
        "blocked" if state == State::Running => {
            reason(data, "orchestrator.simulated_block")?;
            Ok((State::Blocked, progress))
        }
        "decision-requested" if state == State::Blocked => {
            keys(data, &["decisionRequestId", "reasonCode"])?;
            let expected = decision_id(&handoff.tenant_id, &context.mission_id, sequence);
            require(
                data.get("reasonCode").and_then(Value::as_str)
                    == Some("orchestrator.human_decision_required")
                    && data.get("decisionRequestId").and_then(Value::as_str)
                        == Some(expected.as_str()),
            )?;
            Ok((State::Waiting, progress))
        }
        "budget-exceeded" if state == State::Running => {
            reason(data, "orchestrator.duration_budget_exceeded")?;
            require(sequence - 1 == context.budgets.max_duration_seconds)?;
            Ok((State::Terminal, progress))
        }
        "result-submitted" if state == State::Running => {
            keys(data, &["artifact", "evidence", "progressPermille"])?;
            require(
                progress > 0 && data.get("progressPermille").and_then(Value::as_u64) == Some(1000),
            )?;
            result_reference(data.get("artifact"), "urn:libre-ai:artifact:")?;
            result_reference(data.get("evidence"), "urn:libre-ai:evidence:")?;
            Ok((State::Terminal, 1000))
        }
        "failed" if state == State::Running => {
            reason(data, "orchestrator.simulated_failure")?;
            Ok((State::Terminal, progress))
        }
        "paused" | "resumed" | "cancelled" => Err(err(ErrorCode::EventTypeForbidden)),
        _ => Err(err(ErrorCode::EventTransitionInvalid)),
    }
}

fn keys(data: &Map<String, Value>, expected: &[&str]) -> Result<()> {
    let actual = data.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    require(actual == expected)
}

fn reason(data: &Map<String, Value>, expected: &str) -> Result<()> {
    keys(data, &["reasonCode"])?;
    require(data.get("reasonCode").and_then(Value::as_str) == Some(expected))
}

fn result_reference(value: Option<&Value>, prefix: &str) -> Result<()> {
    let object = value
        .and_then(Value::as_object)
        .ok_or_else(|| err(ErrorCode::EventTransitionInvalid))?;
    require(
        object
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id.starts_with(prefix))
            && object.get("mediaType").and_then(Value::as_str) == Some("application/json"),
    )
}

fn make_event(
    tenant: &str,
    context: &MissionContext,
    sequence: u64,
    kind: &str,
    elapsed: u64,
    data: Value,
) -> Result<Value> {
    Ok(json!({
        "schemaVersion": "libre-ai.orchestrator-event.v1",
        "id": event_id(tenant, &context.mission_id, sequence, kind),
        "tenantId": tenant,
        "missionId": context.mission_id,
        "orchestratorId": ORCHESTRATOR_ID,
        "sequence": sequence,
        "type": kind,
        "occurredAt": timestamp(context, elapsed)?.to_rfc3339_opts(SecondsFormat::Secs, true),
        "data": data
    }))
}

fn timestamp(context: &MissionContext, elapsed: u64) -> Result<DateTime<Utc>> {
    context
        .started_at
        .checked_add_signed(Duration::seconds(
            i64::try_from(elapsed).map_err(|_| err(ErrorCode::BudgetInvalid))?,
        ))
        .ok_or_else(|| err(ErrorCode::BudgetInvalid))
}

fn decision_id(tenant: &str, mission: &str, sequence: u64) -> String {
    let material = format!(
        "{PROFILE_VERSION}/decision-id\n{tenant}\n{mission}\n{ORCHESTRATOR_ID}\n{sequence}\n"
    );
    format!("urn:libre-ai:decision:{}", sha256_hex(material.as_bytes()))
}

fn reference_value(reference: &CanonicalReference) -> Value {
    json!({"id": reference.id, "digest": reference.digest, "mediaType": reference.media_type})
}

fn parse_time(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| err(ErrorCode::ContractInvalid))
}

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| err(ErrorCode::ContractInvalid))
}

fn integer(value: &Value, field: &str) -> Result<u64> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| err(ErrorCode::ContractInvalid))
}

fn require(condition: bool) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(err(ErrorCode::EventTransitionInvalid))
    }
}

const fn err(code: ErrorCode) -> OrchestratorError {
    OrchestratorError::new(code)
}
