mod support;

use libre_ai_agent_orchestrator::{
    EffectObservation, EventCollisionObservation, TransferObservation, evaluate_causal_transition,
    evaluate_effect_attestation, evaluate_execution_transfer, parse_authorized_execution_event,
};
use libre_ai_contract_types::ContractRegistry;
use serde_json::Value;
use support::authorized_execution::{
    EventFixture, event_document, valid_effect_attestation, valid_execution_transfer,
};
use support::crash::{
    ATTEMPT, CrashPoint, EDGE, EFFECT_STEP, FakeExecutor, FakeJournal, FakeTerminalObservation,
    TERMINAL_STEP, effect_graph,
};

const ORGANIZATION: &str = "ten_1234567890abcdef";
const RUN: &str = "urn:libre-ai:run:synthetic-run-1";
const EMISSION: &str = "urn:libre-ai:emission:synthetic-emission-1";
const EMISSION_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const EXECUTOR_PROFILE: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

struct RecoveryReport {
    committed_effects: u8,
    committed_before_recovery: u8,
    journal_len_before_recovery: usize,
    journal_len_after_recovery: usize,
    recovery_code: &'static str,
    ready_step_id: Option<String>,
}

#[test]
fn recovery_at_every_cut_point_commits_at_most_once() {
    for crash_point in [
        CrashPoint::AfterStepAuthorized,
        CrashPoint::AfterEffectReserved,
        CrashPoint::AfterEffectStarted,
        CrashPoint::AfterFakeExecutorCommit,
        CrashPoint::AfterTerminalEffectEvent,
    ] {
        let report = run_crash_scenario(crash_point, false).expect("fake recovery scenario");
        assert!(report.committed_effects <= 1, "{crash_point:?}");
        assert_eq!(report.committed_effects, 1, "{crash_point:?}");
        if crash_point != CrashPoint::AfterFakeExecutorCommit {
            assert_eq!(
                report.ready_step_id.as_deref(),
                Some(TERMINAL_STEP),
                "{crash_point:?}"
            );
        }
    }
}

#[test]
fn crash_after_external_commit_blocks_without_status_and_reconciles_with_it() {
    let blocked = run_crash_scenario(CrashPoint::AfterFakeExecutorCommit, false)
        .expect("unknown-status recovery");
    assert_eq!(blocked.recovery_code, "effect-state-unknown");
    assert_eq!(
        blocked.journal_len_after_recovery,
        blocked.journal_len_before_recovery
    );
    assert_eq!(blocked.committed_effects, 1);

    let reconciled = run_crash_scenario(CrashPoint::AfterFakeExecutorCommit, true)
        .expect("terminal-status recovery");
    assert_eq!(reconciled.recovery_code, "emission-duplicate");
    assert_eq!(reconciled.committed_effects, 1);
    assert_eq!(reconciled.ready_step_id.as_deref(), Some(TERMINAL_STEP));
}

#[test]
fn terminal_event_recovery_records_result_without_reemission() {
    let report = run_crash_scenario(CrashPoint::AfterTerminalEffectEvent, false)
        .expect("terminal-event recovery");
    assert_eq!(report.committed_before_recovery, 1);
    assert_eq!(report.committed_effects, 1);
    assert_eq!(
        report.journal_len_after_recovery,
        report.journal_len_before_recovery + 1
    );
    assert_eq!(report.ready_step_id.as_deref(), Some(TERMINAL_STEP));
}

fn run_crash_scenario(
    crash_point: CrashPoint,
    executor_reports_terminal: bool,
) -> Result<RecoveryReport, &'static str> {
    let registry = ContractRegistry::embedded().map_err(|_| "embedded-registry")?;
    let graph = effect_graph(&registry)?;
    let mut journal = FakeJournal::default();
    let mut executor = FakeExecutor::default();
    append_initial(&registry, &mut journal)?;

    if matches!(
        crash_point,
        CrashPoint::AfterEffectReserved
            | CrashPoint::AfterEffectStarted
            | CrashPoint::AfterFakeExecutorCommit
            | CrashPoint::AfterTerminalEffectEvent
    ) {
        append_reserved(&registry, &mut journal)?;
    }
    if matches!(
        crash_point,
        CrashPoint::AfterEffectStarted
            | CrashPoint::AfterFakeExecutorCommit
            | CrashPoint::AfterTerminalEffectEvent
    ) {
        append_started(&registry, &mut journal)?;
    }
    if matches!(
        crash_point,
        CrashPoint::AfterFakeExecutorCommit | CrashPoint::AfterTerminalEffectEvent
    ) {
        commit_with_invocation(
            &registry,
            &mut executor,
            &support::authorized_execution::schema_fixture("step-invocation.v1.schema.json"),
        )?;
    }
    if crash_point == CrashPoint::AfterTerminalEffectEvent {
        append_terminal(&registry, &mut journal, "committed")?;
    }
    if crash_point == CrashPoint::AfterFakeExecutorCommit && executor_reports_terminal {
        executor.terminal_observation = Some(FakeTerminalObservation {
            status: "committed",
        });
    }

    journal.replay(&registry, &graph)?;
    let committed_before_recovery = executor.committed_effects;
    let journal_len_before_recovery = journal.len();
    let recovery_code = match crash_point {
        CrashPoint::AfterStepAuthorized => {
            append_reserved(&registry, &mut journal)?;
            append_started(&registry, &mut journal)?;
            let code = commit_with_invocation(
                &registry,
                &mut executor,
                &support::authorized_execution::schema_fixture("step-invocation.v1.schema.json"),
            )?;
            append_terminal(&registry, &mut journal, "committed")?;
            append_result(&registry, &mut journal)?;
            code
        }
        CrashPoint::AfterEffectReserved => {
            append_started(&registry, &mut journal)?;
            let code = commit_with_invocation(
                &registry,
                &mut executor,
                &support::authorized_execution::schema_fixture("step-invocation.v1.schema.json"),
            )?;
            append_terminal(&registry, &mut journal, "committed")?;
            append_result(&registry, &mut journal)?;
            code
        }
        CrashPoint::AfterEffectStarted => {
            let code = commit_with_invocation(
                &registry,
                &mut executor,
                &support::authorized_execution::schema_fixture("step-invocation.v1.schema.json"),
            )?;
            append_terminal(&registry, &mut journal, "committed")?;
            append_result(&registry, &mut journal)?;
            code
        }
        CrashPoint::AfterFakeExecutorCommit => match executor.terminal_observation {
            None => evaluate_effect_attestation(
                &registry,
                &valid_effect_attestation("state-unknown"),
                effect_observation(None),
            )
            .code(),
            Some(observation) => {
                let decision = evaluate_effect_attestation(
                    &registry,
                    &valid_effect_attestation(observation.status),
                    effect_observation(Some((EMISSION, EMISSION_DIGEST))),
                );
                let code = decision.code();
                if code != "emission-duplicate" {
                    return Err("terminal-reconciliation-refused");
                }
                append_terminal(&registry, &mut journal, observation.status)?;
                append_result(&registry, &mut journal)?;
                code
            }
        },
        CrashPoint::AfterTerminalEffectEvent => {
            append_result(&registry, &mut journal)?;
            "terminal-reconciled"
        }
    };
    let state = journal.replay(&registry, &graph)?;
    Ok(RecoveryReport {
        committed_effects: executor.committed_effects,
        committed_before_recovery,
        journal_len_before_recovery,
        journal_len_after_recovery: journal.len(),
        recovery_code,
        ready_step_id: state.ready_step_id().map(str::to_owned),
    })
}

fn append_initial(
    registry: &ContractRegistry,
    journal: &mut FakeJournal,
) -> Result<(), &'static str> {
    journal.append(registry, "graph-activated", None, None, None, None)?;
    journal.append(
        registry,
        "step-authorized",
        Some(EFFECT_STEP),
        None,
        None,
        None,
    )
}

fn append_reserved(
    registry: &ContractRegistry,
    journal: &mut FakeJournal,
) -> Result<(), &'static str> {
    journal.append(
        registry,
        "effect-reserved",
        Some(EFFECT_STEP),
        None,
        None,
        Some("reserved"),
    )
}

fn append_started(
    registry: &ContractRegistry,
    journal: &mut FakeJournal,
) -> Result<(), &'static str> {
    journal.append(
        registry,
        "effect-started",
        Some(EFFECT_STEP),
        None,
        None,
        Some("started"),
    )
}

fn append_terminal(
    registry: &ContractRegistry,
    journal: &mut FakeJournal,
    status: &str,
) -> Result<(), &'static str> {
    journal.append(
        registry,
        "effect-terminal",
        Some(EFFECT_STEP),
        Some(EDGE),
        None,
        Some(status),
    )
}

fn append_result(
    registry: &ContractRegistry,
    journal: &mut FakeJournal,
) -> Result<(), &'static str> {
    journal.append(
        registry,
        "step-result-recorded",
        Some(EFFECT_STEP),
        Some(EDGE),
        Some("effect-committed"),
        None,
    )
}

fn effect_observation<'a>(prior_emission: Option<(&'a str, &'a str)>) -> EffectObservation<'a> {
    EffectObservation::Authoritative {
        expected_organization_id: ORGANIZATION,
        expected_run_id: RUN,
        expected_attempt_id: ATTEMPT,
        current_generation: 1,
        active_fencing: Some(7),
        expected_executor_profile_digest: EXECUTOR_PROFILE,
        generation_consumed: false,
        lineage_closed: false,
        predecessor_effects_terminal: true,
        executor_profile_qualified: true,
        prior_emission,
        existing_attempt_emission_id: None,
    }
}

fn commit_with_invocation(
    registry: &ContractRegistry,
    executor: &mut FakeExecutor,
    invocation: &Value,
) -> Result<&'static str, &'static str> {
    if !registry
        .is_valid("step-invocation.v1.schema.json", invocation)
        .map_err(|_| "invocation-boundary")?
    {
        return Ok("orchestrator.authorized-execution.schema-invalid");
    }
    let decision = evaluate_effect_attestation(
        registry,
        &valid_effect_attestation("committed"),
        effect_observation(None),
    );
    executor.commit(&decision)?;
    Ok(decision.code())
}

#[test]
fn invalid_invocation_and_identity_substitutions_never_commit() {
    let registry = ContractRegistry::embedded().expect("embedded registry");
    let mut executor = FakeExecutor::default();
    let mut invocation =
        support::authorized_execution::schema_fixture("step-invocation.v1.schema.json");
    invocation["organizationId"] = Value::String("invalid".to_owned());
    assert_eq!(
        commit_with_invocation(&registry, &mut executor, &invocation)
            .expect("closed invocation decision"),
        "orchestrator.authorized-execution.schema-invalid"
    );
    assert_eq!(executor.committed_effects, 0);

    let mut journal = FakeJournal::default();
    append_initial(&registry, &mut journal).expect("initial journal");
    let unchanged_len = journal.len();
    let substitutions = [
        (
            "organizationId",
            Value::String("ten_abcdef1234567890".to_owned()),
        ),
        ("runId", Value::String("urn:libre-ai:run:other".to_owned())),
        (
            "attemptId",
            Value::String("urn:libre-ai:attempt:other".to_owned()),
        ),
        ("generation", Value::from(2)),
        ("fencingValue", Value::from(6)),
        (
            "executorProfileRef.digest",
            Value::String(
                "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_owned(),
            ),
        ),
    ];
    for (field, replacement) in substitutions {
        let mut attestation = valid_effect_attestation("committed");
        if field == "executorProfileRef.digest" {
            attestation["executorProfileRef"]["digest"] = replacement;
        } else {
            attestation[field] = replacement;
        }
        let decision =
            evaluate_effect_attestation(&registry, &attestation, effect_observation(None));
        assert!(decision.application().is_none(), "{}", decision.code());
        assert_eq!(journal.len(), unchanged_len);
        assert_eq!(executor.committed_effects, 0);
    }
}

#[test]
fn unavailable_collision_lineage_and_executor_status_fail_closed() {
    let registry = ContractRegistry::embedded().expect("embedded registry");
    let event_document = event_document(&EventFixture {
        event_type: "graph-activated",
        sequence: 1,
        previous_event_digest: None,
        graph_digest: support::crash::GRAPH_DIGEST,
        step_id: None,
        attempt_id: None,
        worker_invocation_id: None,
        selected_edge_id: None,
        outcome_code: None,
        effect_status: None,
        tool_calls_delta: 0,
        tool_calls_total: 0,
    });
    let event = parse_authorized_execution_event(&registry, &event_document).expect("valid event");
    assert_eq!(
        evaluate_causal_transition(None, &event, EventCollisionObservation::Unavailable).code(),
        "orchestrator.authorized-execution.store-unavailable"
    );
    assert_eq!(
        evaluate_execution_transfer(
            &registry,
            &valid_execution_transfer(),
            TransferObservation::Unavailable,
            "2026-09-10T10:05:00Z",
        )
        .code(),
        "orchestrator.authorized-execution.store-unavailable"
    );
    assert_eq!(
        evaluate_effect_attestation(
            &registry,
            &valid_effect_attestation("committed"),
            EffectObservation::Unavailable,
        )
        .code(),
        "orchestrator.authorized-execution.store-unavailable"
    );
}
