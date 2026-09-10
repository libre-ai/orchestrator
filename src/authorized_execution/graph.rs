use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::{self, Debug, Display, Formatter};

use libre_ai_contract_types::ContractRegistry;
use serde::Deserialize;
use serde_json::Value;

use super::AuthorizedExecutionRefusal;
use super::document::{AuthorizedGraph, StepKind, require_valid};

const EXECUTION_PLAN_SCHEMA: &str = "execution-plan-body.v2.schema.json";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphRefusal {
    DuplicateStep,
    DuplicateEdge,
    EntryMissing,
    DanglingEdge,
    TerminalHasOutgoingEdge,
    RouteMissing,
    RouteAmbiguous,
    TerminalUnreachable,
    UnreachableStep,
    CycleForbidden,
}

impl GraphRefusal {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::DuplicateStep => "duplicate-step",
            Self::DuplicateEdge => "duplicate-edge",
            Self::EntryMissing => "entry-missing",
            Self::DanglingEdge => "dangling-edge",
            Self::TerminalHasOutgoingEdge => "terminal-has-outgoing-edge",
            Self::RouteMissing => "route-missing",
            Self::RouteAmbiguous => "route-ambiguous",
            Self::TerminalUnreachable => "terminal-unreachable",
            Self::UnreachableStep => "unreachable-step",
            Self::CycleForbidden => "cycle-forbidden",
        }
    }
}

impl Display for GraphRefusal {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphDecision {
    Valid,
    Refused(GraphRefusal),
}

impl GraphDecision {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Valid => "graph-valid",
            Self::Refused(refusal) => refusal.code(),
        }
    }

    pub const fn is_valid(&self) -> bool {
        matches!(self, Self::Valid)
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct GraphTransition {
    target_step_id: String,
}

impl GraphTransition {
    pub fn target_step_id(&self) -> &str {
        &self.target_step_id
    }
}

impl Debug for GraphTransition {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GraphTransition")
            .field("code", &"route-selected")
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphTransitionDecision {
    Selected(GraphTransition),
    Refused(GraphRefusal),
}

impl GraphTransitionDecision {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Selected(_) => "route-selected",
            Self::Refused(refusal) => refusal.code(),
        }
    }

    pub fn target_step_id(&self) -> Option<&str> {
        match self {
            Self::Selected(transition) => Some(transition.target_step_id()),
            Self::Refused(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorityRefusal {
    GraphPolicyInvalid,
    AuthorityBindingMismatch,
}

impl AuthorityRefusal {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::GraphPolicyInvalid => "graph-policy-invalid",
            Self::AuthorityBindingMismatch => "authority-binding-mismatch",
        }
    }
}

impl Display for AuthorityRefusal {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorityDecision {
    Valid,
    Refused(AuthorityRefusal),
    BoundaryRefused(AuthorizedExecutionRefusal),
}

impl AuthorityDecision {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Valid => "authority-valid",
            Self::Refused(refusal) => refusal.code(),
            Self::BoundaryRefused(refusal) => refusal.code(),
        }
    }

    pub const fn is_valid(&self) -> bool {
        matches!(self, Self::Valid)
    }
}

pub fn evaluate_graph(graph: &AuthorizedGraph) -> GraphDecision {
    if has_duplicate(graph.steps.iter().map(|step| step.step_id.as_str())) {
        return GraphDecision::Refused(GraphRefusal::DuplicateStep);
    }
    if has_duplicate(graph.edges.iter().map(|edge| edge.edge_id.as_str())) {
        return GraphDecision::Refused(GraphRefusal::DuplicateEdge);
    }

    let steps = graph
        .steps
        .iter()
        .map(|step| (step.step_id.as_str(), step))
        .collect::<BTreeMap<_, _>>();
    if !steps.contains_key(graph.entry_step_id.as_str()) {
        return GraphDecision::Refused(GraphRefusal::EntryMissing);
    }
    if graph.edges.iter().any(|edge| {
        !steps.contains_key(edge.from_step_id.as_str())
            || !steps.contains_key(edge.to_step_id.as_str())
    }) {
        return GraphDecision::Refused(GraphRefusal::DanglingEdge);
    }

    let mut outgoing = graph
        .steps
        .iter()
        .map(|step| (step.step_id.as_str(), Vec::new()))
        .collect::<BTreeMap<_, Vec<_>>>();
    let mut incoming = graph
        .steps
        .iter()
        .map(|step| (step.step_id.as_str(), Vec::new()))
        .collect::<BTreeMap<_, Vec<_>>>();
    for edge in &graph.edges {
        if let Some(routes) = outgoing.get_mut(edge.from_step_id.as_str()) {
            routes.push(edge);
        }
        if let Some(routes) = incoming.get_mut(edge.to_step_id.as_str()) {
            routes.push(edge);
        }
    }

    for step in &graph.steps {
        let routes = outgoing
            .get(step.step_id.as_str())
            .map(Vec::as_slice)
            .unwrap_or_default();
        if step.kind == StepKind::Terminal && !routes.is_empty() {
            return GraphDecision::Refused(GraphRefusal::TerminalHasOutgoingEdge);
        }
        if step.kind == StepKind::Terminal {
            continue;
        }
        for outcome in &step.outcome_codes {
            let route_count = routes
                .iter()
                .filter(|edge| edge.outcome_code == *outcome)
                .count();
            if route_count == 0 {
                return GraphDecision::Refused(GraphRefusal::RouteMissing);
            }
            if route_count > 1 {
                return GraphDecision::Refused(GraphRefusal::RouteAmbiguous);
            }
        }
        if routes
            .iter()
            .any(|edge| !step.outcome_codes.contains(&edge.outcome_code))
        {
            return GraphDecision::Refused(GraphRefusal::RouteAmbiguous);
        }
    }

    let terminal_ids = graph
        .steps
        .iter()
        .filter(|step| step.kind == StepKind::Terminal)
        .map(|step| step.step_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut can_reach_terminal = terminal_ids.clone();
    let mut reverse_queue = terminal_ids.into_iter().collect::<VecDeque<_>>();
    while let Some(current) = reverse_queue.pop_front() {
        if let Some(routes) = incoming.get(current) {
            for edge in routes {
                if can_reach_terminal.insert(edge.from_step_id.as_str()) {
                    reverse_queue.push_back(edge.from_step_id.as_str());
                }
            }
        }
    }
    if graph
        .steps
        .iter()
        .any(|step| !can_reach_terminal.contains(step.step_id.as_str()))
    {
        return GraphDecision::Refused(GraphRefusal::TerminalUnreachable);
    }

    let mut reachable = BTreeSet::from([graph.entry_step_id.as_str()]);
    let mut queue = VecDeque::from([graph.entry_step_id.as_str()]);
    while let Some(current) = queue.pop_front() {
        if let Some(routes) = outgoing.get(current) {
            for edge in routes {
                if reachable.insert(edge.to_step_id.as_str()) {
                    queue.push_back(edge.to_step_id.as_str());
                }
            }
        }
    }
    if graph
        .steps
        .iter()
        .any(|step| !reachable.contains(step.step_id.as_str()))
    {
        return GraphDecision::Refused(GraphRefusal::UnreachableStep);
    }

    let mut indegree = graph
        .steps
        .iter()
        .map(|step| (step.step_id.as_str(), 0_usize))
        .collect::<BTreeMap<_, _>>();
    for edge in &graph.edges {
        if let Some(count) = indegree.get_mut(edge.to_step_id.as_str()) {
            *count += 1;
        }
    }
    let mut roots = indegree
        .iter()
        .filter_map(|(step_id, count)| (*count == 0).then_some(*step_id))
        .collect::<VecDeque<_>>();
    let mut visited = 0_usize;
    while let Some(current) = roots.pop_front() {
        visited += 1;
        if let Some(routes) = outgoing.get(current) {
            for edge in routes {
                if let Some(count) = indegree.get_mut(edge.to_step_id.as_str()) {
                    *count -= 1;
                    if *count == 0 {
                        roots.push_back(edge.to_step_id.as_str());
                    }
                }
            }
        }
    }
    if visited != graph.steps.len() {
        return GraphDecision::Refused(GraphRefusal::CycleForbidden);
    }

    GraphDecision::Valid
}

pub fn select_graph_transition(
    graph: &AuthorizedGraph,
    current_step_id: &str,
    outcome_code: &str,
) -> GraphTransitionDecision {
    let mut routes = graph
        .edges
        .iter()
        .filter(|edge| edge.from_step_id == current_step_id && edge.outcome_code == outcome_code);
    let Some(route) = routes.next() else {
        return GraphTransitionDecision::Refused(GraphRefusal::RouteMissing);
    };
    if routes.next().is_some() {
        return GraphTransitionDecision::Refused(GraphRefusal::RouteAmbiguous);
    }
    GraphTransitionDecision::Selected(GraphTransition {
        target_step_id: route.to_step_id.clone(),
    })
}

pub fn evaluate_graph_authority(
    graph: &AuthorizedGraph,
    plan_document: &Value,
    registry: &ContractRegistry,
) -> AuthorityDecision {
    if !evaluate_graph(graph).is_valid() {
        return AuthorityDecision::Refused(AuthorityRefusal::GraphPolicyInvalid);
    }
    if let Err(refusal) = require_valid(registry, EXECUTION_PLAN_SCHEMA, plan_document) {
        return AuthorityDecision::BoundaryRefused(refusal);
    }
    let plan: WirePlan = match serde_json::from_value(plan_document.clone()) {
        Ok(plan) => plan,
        Err(_) => {
            return AuthorityDecision::BoundaryRefused(AuthorizedExecutionRefusal::SchemaInvalid);
        }
    };

    let mut decision_schema_digests = Vec::new();
    let mut executor_profile_digests = Vec::new();
    for step in &graph.steps {
        if step.kind == StepKind::Terminal {
            continue;
        }
        let Some(retry_policy) = &step.retry_policy else {
            return AuthorityDecision::Refused(AuthorityRefusal::GraphPolicyInvalid);
        };
        if !(1..=32).contains(&retry_policy.maximum_attempts)
            || retry_policy
                .retryable_outcome_codes
                .iter()
                .any(|outcome| !step.outcome_codes.contains(outcome))
        {
            return AuthorityDecision::Refused(AuthorityRefusal::GraphPolicyInvalid);
        }

        if step.kind == StepKind::HumanDecision {
            let Some(policy) = &step.decision_policy else {
                return AuthorityDecision::Refused(AuthorityRefusal::GraphPolicyInvalid);
            };
            if has_duplicate(
                policy
                    .choices
                    .iter()
                    .map(|choice| choice.choice_id.as_str()),
            ) || policy
                .choices
                .iter()
                .any(|choice| !step.outcome_codes.contains(&choice.outcome_code))
                || !step
                    .outcome_codes
                    .contains(&policy.no_response_outcome_code)
            {
                return AuthorityDecision::Refused(AuthorityRefusal::GraphPolicyInvalid);
            }
            decision_schema_digests.push(policy.request_schema_digest.as_str());
            decision_schema_digests.push(policy.response_schema_digest.as_str());
        }
        if step.kind == StepKind::ExternalEffect {
            let Some(policy) = &step.effect_policy else {
                return AuthorityDecision::Refused(AuthorityRefusal::GraphPolicyInvalid);
            };
            executor_profile_digests.push(policy.executor_profile_digest.as_str());
        }
    }

    let plan_decision_digests = plan
        .decision_schema_refs
        .iter()
        .map(|reference| reference.digest.as_str())
        .collect::<Vec<_>>();
    let plan_executor_digests = plan
        .executor_profile_refs
        .iter()
        .map(|reference| reference.digest.as_str())
        .collect::<Vec<_>>();
    if graph.organization_id != plan.organization_id
        || graph.id != plan.execution_graph.id
        || graph.graph_digest != plan.execution_graph.digest
        || !same_string_set(&decision_schema_digests, &plan_decision_digests)
        || !same_string_set(&executor_profile_digests, &plan_executor_digests)
    {
        return AuthorityDecision::Refused(AuthorityRefusal::AuthorityBindingMismatch);
    }

    AuthorityDecision::Valid
}

fn has_duplicate<'a>(mut values: impl Iterator<Item = &'a str>) -> bool {
    let mut seen = BTreeSet::new();
    values.any(|value| !seen.insert(value))
}

fn same_string_set(left: &[&str], right: &[&str]) -> bool {
    left.len() == right.len()
        && !has_duplicate(left.iter().copied())
        && !has_duplicate(right.iter().copied())
        && left.iter().all(|value| right.contains(value))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WirePlan {
    organization_id: String,
    execution_graph: WireArtifactReference,
    decision_schema_refs: Vec<WireArtifactReference>,
    executor_profile_refs: Vec<WireArtifactReference>,
}

#[derive(Deserialize)]
struct WireArtifactReference {
    id: String,
    digest: String,
}
