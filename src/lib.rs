#![forbid(unsafe_code)]

mod authorized_execution;
mod budget;
mod control;

pub use authorized_execution::{
    AuthorityDecision, AuthorityRefusal, AuthorizedExecutionEvent, AuthorizedExecutionRefusal,
    AuthorizedExecutionState, AuthorizedGraph, CausalDecision, CausalRefusal,
    EventCollisionObservation, GraphDecision, GraphRefusal, GraphTransition,
    GraphTransitionDecision, evaluate_causal_transition, evaluate_graph, evaluate_graph_authority,
    parse_authorized_execution_event, parse_authorized_graph, replay_authorized_execution,
    select_graph_transition,
};
pub use budget::{BudgetDecision, EventStoreObservation, PlanBudgetLimits, evaluate_budget_event};
pub use control::{
    CommandCollisionObservation, CommandReceipt, ControlAction, ControlApplication, ControlCommand,
    ControlDecision, ControlEffect, ControlPhase, ControlRefusal, RunControlState,
    SimulatedEffectDecision, StartPreflight, command_fingerprint, evaluate_control,
    evaluate_simulated_effect, parse_control_document,
};
