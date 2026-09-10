use std::collections::BTreeMap;
use std::fmt::{self, Debug, Formatter};

use super::AuthorizedExecutionRefusal;
use super::document::{
    AuthorizedExecutionEvent, AuthorizedGraph, BudgetCounters, EffectStatus, EventKind, StepKind,
};
use super::graph::{GraphTransitionDecision, select_graph_transition};

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum EventCollisionObservation<'a> {
    Absent,
    Existing {
        event_id: &'a str,
        sequence: u64,
        event_digest: &'a str,
    },
    Unavailable,
}

impl Debug for EventCollisionObservation<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Absent => "EventCollisionObservation::Absent",
            Self::Existing { .. } => "EventCollisionObservation::Existing(<redacted>)",
            Self::Unavailable => "EventCollisionObservation::Unavailable",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CausalRefusal {
    DuplicateDivergent,
    IdentityMismatch,
    GenerationStale,
    SequenceInvalid,
    PreviousDigestMismatch,
    BudgetDecreased,
    BudgetArithmeticInvalid,
}

impl CausalRefusal {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::DuplicateDivergent => "duplicate-divergent",
            Self::IdentityMismatch => "identity-mismatch",
            Self::GenerationStale => "generation-stale",
            Self::SequenceInvalid => "sequence-invalid",
            Self::PreviousDigestMismatch => "previous-digest-mismatch",
            Self::BudgetDecreased => "budget-decreased",
            Self::BudgetArithmeticInvalid => "budget-arithmetic-invalid",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CausalDecision {
    Valid,
    Idempotent,
    Refused(CausalRefusal),
    BoundaryRefused(AuthorizedExecutionRefusal),
}

impl CausalDecision {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Valid => "event-valid",
            Self::Idempotent => "idempotent-duplicate",
            Self::Refused(refusal) => refusal.code(),
            Self::BoundaryRefused(refusal) => refusal.code(),
        }
    }
}

pub fn evaluate_causal_transition(
    previous: Option<&AuthorizedExecutionEvent>,
    current: &AuthorizedExecutionEvent,
    collision: EventCollisionObservation<'_>,
) -> CausalDecision {
    match collision {
        EventCollisionObservation::Unavailable => {
            return CausalDecision::BoundaryRefused(AuthorizedExecutionRefusal::StoreUnavailable);
        }
        EventCollisionObservation::Existing {
            event_id,
            sequence,
            event_digest,
        } => {
            if event_id == current.id
                && sequence == current.sequence
                && event_digest == current.event_digest
            {
                return CausalDecision::Idempotent;
            }
            return CausalDecision::Refused(CausalRefusal::DuplicateDivergent);
        }
        EventCollisionObservation::Absent => {}
    }

    let Some(previous) = previous else {
        if current.sequence != 1 || current.previous_event_digest.is_some() {
            return CausalDecision::Refused(CausalRefusal::SequenceInvalid);
        }
        return match BudgetCounters::from_zero().checked_add(current.budget_delta) {
            None => CausalDecision::BoundaryRefused(AuthorizedExecutionRefusal::ArithmeticOverflow),
            Some(expected) if expected != current.budget_total => {
                CausalDecision::Refused(CausalRefusal::BudgetArithmeticInvalid)
            }
            Some(_) => CausalDecision::Valid,
        };
    };

    if !same_identity(previous, current) {
        return CausalDecision::Refused(CausalRefusal::IdentityMismatch);
    }
    if previous.generation != current.generation {
        return CausalDecision::Refused(CausalRefusal::GenerationStale);
    }
    let Some(expected_sequence) = previous.sequence.checked_add(1) else {
        return CausalDecision::BoundaryRefused(AuthorizedExecutionRefusal::ArithmeticOverflow);
    };
    if current.sequence != expected_sequence {
        return CausalDecision::Refused(CausalRefusal::SequenceInvalid);
    }
    if current.previous_event_digest.as_deref() != Some(previous.event_digest.as_str()) {
        return CausalDecision::Refused(CausalRefusal::PreviousDigestMismatch);
    }
    if current
        .budget_total
        .has_decreased_from(previous.budget_total)
    {
        return CausalDecision::Refused(CausalRefusal::BudgetDecreased);
    }
    match previous.budget_total.checked_add(current.budget_delta) {
        None => CausalDecision::BoundaryRefused(AuthorizedExecutionRefusal::ArithmeticOverflow),
        Some(expected) if expected != current.budget_total => {
            CausalDecision::Refused(CausalRefusal::BudgetArithmeticInvalid)
        }
        Some(_) => CausalDecision::Valid,
    }
}

fn same_identity(left: &AuthorizedExecutionEvent, right: &AuthorizedExecutionEvent) -> bool {
    left.organization_id == right.organization_id
        && left.mission_id == right.mission_id
        && left.run_id == right.run_id
        && left.orchestrator_id == right.orchestrator_id
        && left.plan_digest == right.plan_digest
        && left.authorization_digest == right.authorization_digest
        && left.graph_digest == right.graph_digest
}

impl BudgetCounters {
    const fn from_zero() -> Self {
        Self { values: [0; 7] }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct AuthorizedExecutionState {
    sequence: u64,
    event_digest: String,
    ready_step_id: Option<String>,
    budget_total: BudgetCounters,
    phase: ReplayPhase,
    completed: bool,
    quarantined: bool,
}

impl AuthorizedExecutionState {
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn event_digest(&self) -> &str {
        &self.event_digest
    }

    pub fn ready_step_id(&self) -> Option<&str> {
        self.ready_step_id.as_deref()
    }

    pub const fn tool_calls_total(&self) -> u64 {
        self.budget_total.tool_calls()
    }

    pub const fn is_completed(&self) -> bool {
        self.completed
    }

    pub const fn is_quarantined(&self) -> bool {
        self.quarantined
    }
}

impl Debug for AuthorizedExecutionState {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthorizedExecutionState")
            .field("sequence", &self.sequence)
            .field("completed", &self.completed)
            .field("quarantined", &self.quarantined)
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
enum ReplayPhase {
    Ready,
    Authorized,
    InvocationStarted {
        step_id: String,
        attempt_id: String,
        worker_invocation_id: String,
    },
    DecisionRequested {
        step_id: String,
        attempt_id: String,
    },
    EffectReserved {
        step_id: String,
        attempt_id: String,
        worker_invocation_id: String,
    },
    EffectStarted {
        step_id: String,
        attempt_id: String,
        worker_invocation_id: String,
    },
    EffectTerminal {
        step_id: String,
        attempt_id: String,
        worker_invocation_id: String,
        selected_edge_id: String,
        outcome_code: &'static str,
    },
    Sealed,
    Transferred,
    Blocked,
    Completed,
    Quarantined,
}

pub fn replay_authorized_execution(
    graph: &AuthorizedGraph,
    events: &[AuthorizedExecutionEvent],
) -> Result<AuthorizedExecutionState, AuthorizedExecutionRefusal> {
    let Some(first) = events.first() else {
        return Err(AuthorizedExecutionRefusal::TransitionForbidden);
    };
    let mut state = initial_state(graph, first)?;
    let mut by_id = BTreeMap::<&str, (&str, u64)>::new();
    let mut by_sequence = BTreeMap::<u64, (&str, &str)>::new();
    let mut previous_index = 0_usize;
    by_id.insert(first.id(), (first.digest(), first.sequence()));
    by_sequence.insert(first.sequence(), (first.id(), first.digest()));

    for (event_index, event) in events.iter().enumerate().skip(1) {
        let collision = collision_observation(event, &by_id, &by_sequence);
        match evaluate_causal_transition(Some(&events[previous_index]), event, collision) {
            CausalDecision::Idempotent => continue,
            CausalDecision::Refused(CausalRefusal::DuplicateDivergent) => {
                state.quarantined = true;
                state.phase = ReplayPhase::Quarantined;
                return Ok(state);
            }
            CausalDecision::Valid => {}
            CausalDecision::Refused(_) => {
                return Err(AuthorizedExecutionRefusal::TransitionForbidden);
            }
            CausalDecision::BoundaryRefused(refusal) => return Err(refusal),
        }

        if apply_event(graph, &mut state, event).is_err() {
            state.sequence = event.sequence;
            state.event_digest.clone_from(&event.event_digest);
            state.budget_total = event.budget_total;
            state.ready_step_id = None;
            state.quarantined = true;
            state.phase = ReplayPhase::Quarantined;
            return Ok(state);
        }
        by_id.insert(event.id(), (event.digest(), event.sequence()));
        by_sequence.insert(event.sequence(), (event.id(), event.digest()));
        previous_index = event_index;
    }
    Ok(state)
}

fn initial_state(
    graph: &AuthorizedGraph,
    event: &AuthorizedExecutionEvent,
) -> Result<AuthorizedExecutionState, AuthorizedExecutionRefusal> {
    match evaluate_causal_transition(None, event, EventCollisionObservation::Absent) {
        CausalDecision::Valid => {}
        CausalDecision::BoundaryRefused(refusal) => return Err(refusal),
        CausalDecision::Idempotent | CausalDecision::Refused(_) => {
            return Err(AuthorizedExecutionRefusal::TransitionForbidden);
        }
    }
    let EventKind::GraphActivated { graph_id } = &event.kind else {
        return Err(AuthorizedExecutionRefusal::TransitionForbidden);
    };
    if graph_id != &graph.id || event.graph_digest != graph.graph_digest {
        return Err(AuthorizedExecutionRefusal::TransitionForbidden);
    }
    Ok(AuthorizedExecutionState {
        sequence: event.sequence,
        event_digest: event.event_digest.clone(),
        ready_step_id: Some(graph.entry_step_id.clone()),
        budget_total: event.budget_total,
        phase: ReplayPhase::Ready,
        completed: false,
        quarantined: false,
    })
}

fn collision_observation<'a>(
    event: &'a AuthorizedExecutionEvent,
    by_id: &BTreeMap<&'a str, (&'a str, u64)>,
    by_sequence: &BTreeMap<u64, (&'a str, &'a str)>,
) -> EventCollisionObservation<'a> {
    if let Some((digest, sequence)) = by_id.get(event.id()) {
        EventCollisionObservation::Existing {
            event_id: event.id(),
            sequence: *sequence,
            event_digest: digest,
        }
    } else if let Some((id, digest)) = by_sequence.get(&event.sequence()) {
        EventCollisionObservation::Existing {
            event_id: id,
            sequence: event.sequence(),
            event_digest: digest,
        }
    } else {
        EventCollisionObservation::Absent
    }
}

fn apply_event(
    graph: &AuthorizedGraph,
    state: &mut AuthorizedExecutionState,
    event: &AuthorizedExecutionEvent,
) -> Result<(), AuthorizedExecutionRefusal> {
    match &event.kind {
        EventKind::GraphActivated { .. } => return forbidden(),
        EventKind::StepAuthorized => {
            if state.phase != ReplayPhase::Ready
                || state.ready_step_id.as_deref() != event.step_id.as_deref()
            {
                return forbidden();
            }
            state.ready_step_id = None;
            state.phase = ReplayPhase::Authorized;
        }
        EventKind::InvocationStarted => {
            if state.phase != ReplayPhase::Authorized {
                return forbidden();
            }
            state.phase = ReplayPhase::InvocationStarted {
                step_id: required(&event.step_id)?.to_owned(),
                attempt_id: required(&event.attempt_id)?.to_owned(),
                worker_invocation_id: required(&event.worker_invocation_id)?.to_owned(),
            };
        }
        EventKind::StepResultRecorded { outcome_code } => {
            match &state.phase {
                ReplayPhase::InvocationStarted { .. } => {
                    require_invocation(&state.phase, event)?;
                }
                ReplayPhase::EffectTerminal {
                    step_id,
                    attempt_id,
                    worker_invocation_id,
                    selected_edge_id,
                    outcome_code: terminal_outcome,
                } if Some(step_id.as_str()) == event.step_id.as_deref()
                    && Some(attempt_id.as_str()) == event.attempt_id.as_deref()
                    && Some(worker_invocation_id.as_str())
                        == event.worker_invocation_id.as_deref()
                    && Some(selected_edge_id.as_str()) == event.selected_edge_id.as_deref()
                    && *terminal_outcome == outcome_code => {}
                _ => return forbidden(),
            }
            route(graph, state, event, outcome_code)?;
        }
        EventKind::DecisionRequested => {
            if state.phase != ReplayPhase::Authorized {
                return forbidden();
            }
            state.phase = ReplayPhase::DecisionRequested {
                step_id: required(&event.step_id)?.to_owned(),
                attempt_id: required(&event.attempt_id)?.to_owned(),
            };
        }
        EventKind::DecisionConsumed { outcome_code } => {
            require_decision(&state.phase, event)?;
            route(graph, state, event, outcome_code)?;
        }
        EventKind::EffectReserved { status } => {
            if state.phase != ReplayPhase::Authorized || *status != EffectStatus::Reserved {
                return forbidden();
            }
            state.phase = ReplayPhase::EffectReserved {
                step_id: required(&event.step_id)?.to_owned(),
                attempt_id: required(&event.attempt_id)?.to_owned(),
                worker_invocation_id: required(&event.worker_invocation_id)?.to_owned(),
            };
        }
        EventKind::EffectStarted { status } => {
            if *status != EffectStatus::Started {
                return forbidden();
            }
            require_effect(&state.phase, event, false)?;
            state.phase = ReplayPhase::EffectStarted {
                step_id: required(&event.step_id)?.to_owned(),
                attempt_id: required(&event.attempt_id)?.to_owned(),
                worker_invocation_id: required(&event.worker_invocation_id)?.to_owned(),
            };
        }
        EventKind::EffectTerminal { status } => {
            require_effect(&state.phase, event, true)?;
            let Some(outcome_code) = status.outcome_code() else {
                return forbidden();
            };
            validate_route(graph, event, outcome_code)?;
            state.phase = ReplayPhase::EffectTerminal {
                step_id: required(&event.step_id)?.to_owned(),
                attempt_id: required(&event.attempt_id)?.to_owned(),
                worker_invocation_id: required(&event.worker_invocation_id)?.to_owned(),
                selected_edge_id: required(&event.selected_edge_id)?.to_owned(),
                outcome_code,
            };
        }
        EventKind::EffectUnknown { status } => {
            if *status != EffectStatus::StateUnknown {
                return forbidden();
            }
            require_effect(&state.phase, event, true)?;
            state.ready_step_id = None;
            state.quarantined = true;
            state.phase = ReplayPhase::Quarantined;
        }
        EventKind::RunBlocked => {
            state.ready_step_id = None;
            state.phase = ReplayPhase::Blocked;
        }
        EventKind::Quarantined => {
            state.ready_step_id = None;
            state.quarantined = true;
            state.phase = ReplayPhase::Quarantined;
        }
        EventKind::RunCompleted => {
            let Some(ready_step_id) = state.ready_step_id.as_deref() else {
                return forbidden();
            };
            let terminal = graph
                .steps
                .iter()
                .any(|step| step.step_id == ready_step_id && step.kind == StepKind::Terminal);
            if state.phase != ReplayPhase::Ready || !terminal {
                return forbidden();
            }
            state.completed = true;
            state.phase = ReplayPhase::Completed;
        }
        EventKind::PredecessorSealed => {
            if !matches!(state.phase, ReplayPhase::Completed | ReplayPhase::Blocked) {
                return forbidden();
            }
            state.phase = ReplayPhase::Sealed;
        }
        EventKind::GenerationTransferred => {
            if state.phase != ReplayPhase::Sealed {
                return forbidden();
            }
            state.phase = ReplayPhase::Transferred;
        }
    }
    state.sequence = event.sequence;
    state.event_digest.clone_from(&event.event_digest);
    state.budget_total = event.budget_total;
    Ok(())
}

fn route(
    graph: &AuthorizedGraph,
    state: &mut AuthorizedExecutionState,
    event: &AuthorizedExecutionEvent,
    outcome_code: &str,
) -> Result<(), AuthorizedExecutionRefusal> {
    state.ready_step_id = Some(validate_route(graph, event, outcome_code)?);
    state.phase = ReplayPhase::Ready;
    Ok(())
}

fn validate_route(
    graph: &AuthorizedGraph,
    event: &AuthorizedExecutionEvent,
    outcome_code: &str,
) -> Result<String, AuthorizedExecutionRefusal> {
    let step_id = required(&event.step_id)?;
    match select_graph_transition(graph, step_id, outcome_code) {
        GraphTransitionDecision::Selected(transition)
            if event.selected_edge_id.as_deref() == Some(transition.edge_id()) =>
        {
            Ok(transition.target_step_id().to_owned())
        }
        GraphTransitionDecision::Selected(_) | GraphTransitionDecision::Refused(_) => forbidden(),
    }
}

fn require_invocation(
    phase: &ReplayPhase,
    event: &AuthorizedExecutionEvent,
) -> Result<(), AuthorizedExecutionRefusal> {
    match phase {
        ReplayPhase::InvocationStarted {
            step_id,
            attempt_id,
            worker_invocation_id,
        } if Some(step_id.as_str()) == event.step_id.as_deref()
            && Some(attempt_id.as_str()) == event.attempt_id.as_deref()
            && Some(worker_invocation_id.as_str()) == event.worker_invocation_id.as_deref() =>
        {
            Ok(())
        }
        _ => forbidden(),
    }
}

fn require_decision(
    phase: &ReplayPhase,
    event: &AuthorizedExecutionEvent,
) -> Result<(), AuthorizedExecutionRefusal> {
    match phase {
        ReplayPhase::DecisionRequested {
            step_id,
            attempt_id,
        } if Some(step_id.as_str()) == event.step_id.as_deref()
            && Some(attempt_id.as_str()) == event.attempt_id.as_deref() =>
        {
            Ok(())
        }
        _ => forbidden(),
    }
}

fn require_effect(
    phase: &ReplayPhase,
    event: &AuthorizedExecutionEvent,
    started: bool,
) -> Result<(), AuthorizedExecutionRefusal> {
    let parts = match phase {
        ReplayPhase::EffectReserved {
            step_id,
            attempt_id,
            worker_invocation_id,
        } if !started => (step_id, attempt_id, worker_invocation_id),
        ReplayPhase::EffectStarted {
            step_id,
            attempt_id,
            worker_invocation_id,
        } if started => (step_id, attempt_id, worker_invocation_id),
        _ => return forbidden(),
    };
    if Some(parts.0.as_str()) == event.step_id.as_deref()
        && Some(parts.1.as_str()) == event.attempt_id.as_deref()
        && Some(parts.2.as_str()) == event.worker_invocation_id.as_deref()
    {
        Ok(())
    } else {
        forbidden()
    }
}

fn required(value: &Option<String>) -> Result<&str, AuthorizedExecutionRefusal> {
    value
        .as_deref()
        .ok_or(AuthorizedExecutionRefusal::TransitionForbidden)
}

fn forbidden<T>() -> Result<T, AuthorizedExecutionRefusal> {
    Err(AuthorizedExecutionRefusal::TransitionForbidden)
}

#[cfg(test)]
mod tests {
    use super::BudgetCounters;

    #[test]
    fn checked_budget_addition_refuses_u64_overflow() {
        let maximum = BudgetCounters {
            values: [u64::MAX; 7],
        };
        let delta = BudgetCounters { values: [1; 7] };

        assert!(maximum.checked_add(delta).is_none());
    }
}
