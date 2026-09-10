use std::fmt::{self, Display, Formatter};

mod document;
mod graph;

pub use document::{AuthorizedGraph, parse_authorized_graph};
pub use graph::{
    AuthorityDecision, AuthorityRefusal, GraphDecision, GraphRefusal, GraphTransition,
    GraphTransitionDecision, evaluate_graph, evaluate_graph_authority, select_graph_transition,
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
