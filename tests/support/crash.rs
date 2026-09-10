#![allow(dead_code)]
#![allow(clippy::enum_variant_names)]

use libre_ai_agent_orchestrator::{
    AuthorizedExecutionEvent, AuthorizedExecutionState, AuthorizedGraph, EffectDecision,
    parse_authorized_execution_event, parse_authorized_graph, replay_authorized_execution,
};
use libre_ai_contract_types::ContractRegistry;
use serde_json::{Value, json};

use super::authorized_execution::{EventFixture, event_document, schema_fixture};

pub const EFFECT_STEP: &str = "urn:libre-ai:step:synthetic-effect-1";
pub const TERMINAL_STEP: &str = "urn:libre-ai:step:synthetic-terminal-1";
pub const ATTEMPT: &str = "urn:libre-ai:attempt:synthetic-attempt-1";
pub const WORKER: &str = "urn:libre-ai:worker-invocation:synthetic-worker-1";
pub const EDGE: &str = "urn:libre-ai:edge:synthetic-committed-1";
pub const GRAPH_DIGEST: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CrashPoint {
    AfterStepAuthorized,
    AfterEffectReserved,
    AfterEffectStarted,
    AfterFakeExecutorCommit,
    AfterTerminalEffectEvent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FakeTerminalObservation {
    pub status: &'static str,
}

#[derive(Default)]
pub struct FakeExecutor {
    pub committed_effects: u8,
    pub terminal_observation: Option<FakeTerminalObservation>,
}

impl FakeExecutor {
    pub fn commit(&mut self, decision: &EffectDecision) -> Result<(), &'static str> {
        if decision.application().is_none() {
            return Err("effect-not-admitted");
        }
        self.committed_effects = self
            .committed_effects
            .checked_add(1)
            .ok_or("fake-commit-overflow")?;
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeJournal {
    documents: Vec<Value>,
}

impl FakeJournal {
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    pub fn append(
        &mut self,
        registry: &ContractRegistry,
        event_type: &str,
        step_id: Option<&str>,
        selected_edge_id: Option<&str>,
        outcome_code: Option<&str>,
        effect_status: Option<&str>,
    ) -> Result<(), &'static str> {
        let events = self.parsed_events(registry)?;
        let previous_event_digest = events.last().map(AuthorizedExecutionEvent::digest);
        let sequence = u64::try_from(self.documents.len())
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or("journal-sequence-overflow")?;
        let attempt_id = matches!(
            event_type,
            "effect-reserved" | "effect-started" | "effect-terminal" | "step-result-recorded"
        )
        .then_some(ATTEMPT);
        let worker_invocation_id = attempt_id.map(|_| WORKER);
        let document = event_document(&EventFixture {
            event_type,
            sequence,
            previous_event_digest,
            graph_digest: GRAPH_DIGEST,
            step_id,
            attempt_id,
            worker_invocation_id,
            selected_edge_id,
            outcome_code,
            effect_status,
            tool_calls_delta: 0,
            tool_calls_total: 0,
        });
        parse_authorized_execution_event(registry, &document)
            .map_err(|_| "journal-event-boundary")?;
        self.documents.push(document);
        Ok(())
    }

    pub fn replay(
        &self,
        registry: &ContractRegistry,
        graph: &AuthorizedGraph,
    ) -> Result<AuthorizedExecutionState, &'static str> {
        let events = self.parsed_events(registry)?;
        replay_authorized_execution(graph, &events).map_err(|_| "journal-replay")
    }

    fn parsed_events(
        &self,
        registry: &ContractRegistry,
    ) -> Result<Vec<AuthorizedExecutionEvent>, &'static str> {
        self.documents
            .iter()
            .map(|document| {
                parse_authorized_execution_event(registry, document)
                    .map_err(|_| "journal-event-boundary")
            })
            .collect()
    }
}

pub fn effect_graph(registry: &ContractRegistry) -> Result<AuthorizedGraph, &'static str> {
    let mut document = schema_fixture("execution-graph.v1.schema.json");
    document["entryStepId"] = Value::String(EFFECT_STEP.to_owned());
    document["steps"] = json!([
        {
            "stepId": EFFECT_STEP,
            "kind": "external-effect",
            "outcomeCodes": ["effect-committed"],
            "retryPolicy": {
                "maximumAttempts": 1,
                "retryableOutcomeCodes": []
            },
            "effectPolicy": {
                "executorProfileDigest": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                "reEmissionMode": "retry-after-terminal-status-with-fencing"
            }
        },
        {
            "stepId": TERMINAL_STEP,
            "kind": "terminal",
            "outcomeCodes": []
        }
    ]);
    document["edges"] = json!([{
        "edgeId": EDGE,
        "fromStepId": EFFECT_STEP,
        "outcomeCode": "effect-committed",
        "toStepId": TERMINAL_STEP
    }]);
    document["graphDigest"] = Value::String(GRAPH_DIGEST.to_owned());
    parse_authorized_graph(registry, &document).map_err(|_| "effect-graph-boundary")
}
