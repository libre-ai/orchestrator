use libre_ai_contract_types::event_chain::{
    AcceptedEventCollision, OrchestratorCausalEventFacts, OrchestratorEventChainResult,
    evaluate_orchestrator_event_chain,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanBudgetLimits {
    pub plan_digest: String,
    pub max_duration_seconds: u64,
    pub max_tool_calls: u64,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_processes: u64,
    pub max_files_changed: u64,
    pub max_changed_bytes: u64,
}

impl PlanBudgetLimits {
    fn contains(&self, event: &OrchestratorCausalEventFacts) -> bool {
        let total = &event.budget_total;
        total.duration_seconds <= self.max_duration_seconds
            && total.tool_calls <= self.max_tool_calls
            && total.input_tokens <= self.max_input_tokens
            && total.output_tokens <= self.max_output_tokens
            && total.processes_started <= self.max_processes
            && total.files_changed <= self.max_files_changed
            && total.changed_bytes <= self.max_changed_bytes
    }
}

#[derive(Clone, Copy, Debug)]
pub enum EventStoreObservation<'a> {
    Available {
        previous: Option<&'a OrchestratorCausalEventFacts>,
        collision: Option<&'a AcceptedEventCollision>,
    },
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetDecision {
    Chain(OrchestratorEventChainResult),
    CausalStoreUnavailable,
    PlanIdentityMismatch,
    PlanBudgetExceeded,
}

impl BudgetDecision {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Chain(result) => result.code(),
            Self::CausalStoreUnavailable => "causal-store-unavailable",
            Self::PlanIdentityMismatch => "plan-identity-mismatch",
            Self::PlanBudgetExceeded => "plan-budget-exceeded",
        }
    }
}

/// Validates one event against caller-supplied accepted state. No ledger or effect is owned here.
#[must_use]
pub fn evaluate_budget_event(
    observation: EventStoreObservation<'_>,
    current: &OrchestratorCausalEventFacts,
    limits: &PlanBudgetLimits,
) -> BudgetDecision {
    let EventStoreObservation::Available {
        previous,
        collision,
    } = observation
    else {
        return BudgetDecision::CausalStoreUnavailable;
    };
    if current.plan_digest != limits.plan_digest {
        return BudgetDecision::PlanIdentityMismatch;
    }
    let chain = evaluate_orchestrator_event_chain(previous, current, collision);
    if chain != OrchestratorEventChainResult::Valid {
        return BudgetDecision::Chain(chain);
    }
    if !limits.contains(current) {
        return BudgetDecision::PlanBudgetExceeded;
    }
    BudgetDecision::Chain(OrchestratorEventChainResult::Valid)
}
