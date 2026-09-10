use std::fmt::{self, Display, Formatter};

mod decision;
mod document;
mod effect;
mod graph;
mod replay;
mod transfer;

pub use decision::{
    DecisionApplication, DecisionDecision, DecisionObservation, DecisionRefusal,
    evaluate_human_decision,
};
pub use document::{
    AuthorizedExecutionEvent, AuthorizedGraph, parse_authorized_execution_event,
    parse_authorized_graph,
};
pub use effect::{
    EffectApplication, EffectDecision, EffectObservation, EffectRefusal,
    evaluate_effect_attestation,
};
pub use graph::{
    AuthorityDecision, AuthorityRefusal, GraphDecision, GraphRefusal, GraphTransition,
    GraphTransitionDecision, evaluate_graph, evaluate_graph_authority, select_graph_transition,
};
pub use replay::{
    AuthorizedExecutionState, CausalDecision, CausalRefusal, EventCollisionObservation,
    evaluate_causal_transition, replay_authorized_execution,
};
pub use transfer::{
    TransferApplication, TransferDecision, TransferObservation, TransferRefusal,
    evaluate_execution_transfer,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorizedExecutionRefusal {
    SchemaInvalid,
    StoreUnavailable,
    TransitionForbidden,
    BudgetExceeded,
    ArithmeticOverflow,
}

impl AuthorizedExecutionRefusal {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::SchemaInvalid => "orchestrator.authorized-execution.schema-invalid",
            Self::StoreUnavailable => "orchestrator.authorized-execution.store-unavailable",
            Self::TransitionForbidden => "orchestrator.authorized-execution.transition-forbidden",
            Self::BudgetExceeded => "orchestrator.authorized-execution.budget-exceeded",
            Self::ArithmeticOverflow => "orchestrator.authorized-execution.arithmetic-overflow",
        }
    }
}

impl Display for AuthorizedExecutionRefusal {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}
