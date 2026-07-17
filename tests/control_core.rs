use libre_ai_agent_orchestrator::{
    CommandCollisionObservation, CommandReceipt, ControlAction, ControlApplication, ControlCommand,
    ControlDecision, ControlEffect, ControlPhase, RunControlState, SimulatedEffectDecision,
    StartPreflight, command_fingerprint, evaluate_control, evaluate_simulated_effect,
    parse_control_document,
};
use libre_ai_contract_types::ContractRegistry;
use serde_json::{Value, json};

const TENANT: &str = "ten_1234567890abcdef";
const MISSION: &str = "urn:libre-ai:mission:mission-1";
const RUN: &str = "urn:libre-ai:run:run-1";
const PLAN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const AUTHORIZATION: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const NOW: &str = "2030-01-01T00:01:00Z";
const NO_COLLISION: CommandCollisionObservation<'static> = CommandCollisionObservation::NoCollision;

fn command_document(action: ControlAction, expected_revision: u64) -> Value {
    let action_name = match action {
        ControlAction::Start => "start",
        ControlAction::Pause => "pause",
        ControlAction::Resume => "resume",
        ControlAction::Cancel => "cancel",
    };
    let mut document = json!({
        "schemaVersion": "libre-ai.orchestrator-control.v1",
        "id": "urn:libre-ai:control:control-1",
        "tenantId": TENANT,
        "missionId": MISSION,
        "planDigest": PLAN,
        "authorizationDigest": AUTHORIZATION,
        "action": action_name,
        "expectedRevision": expected_revision,
        "idempotencyKey": "idem_1234567890abcdef",
        "reasonCode": "orchestrator.fixture",
        "issuedAt": "2030-01-01T00:00:00Z",
        "expiresAt": "2030-01-01T00:05:00Z"
    });
    if action != ControlAction::Start {
        document
            .as_object_mut()
            .expect("command object")
            .insert("runId".to_owned(), Value::String(RUN.to_owned()));
    }
    document
}

fn parse_command(document: Value) -> ControlCommand {
    let registry = ContractRegistry::embedded().expect("embedded contracts must compile");
    parse_control_document(&registry, &document).expect("fixture command must validate")
}

fn command(action: ControlAction, expected_revision: u64) -> ControlCommand {
    parse_command(command_document(action, expected_revision))
}

fn state(phase: ControlPhase, revision: u64) -> RunControlState {
    RunControlState {
        tenant_id: TENANT.to_owned(),
        mission_id: MISSION.to_owned(),
        run_id: RUN.to_owned(),
        plan_digest: PLAN.to_owned(),
        authorization_digest: AUTHORIZATION.to_owned(),
        authorization_active: true,
        authorization_store_available: true,
        revision,
        phase,
    }
}

fn preflight() -> StartPreflight {
    StartPreflight {
        tenant_id: TENANT.to_owned(),
        mission_id: MISSION.to_owned(),
        plan_digest: PLAN.to_owned(),
        authorization_digest: AUTHORIZATION.to_owned(),
        authoritative_revision: 7,
        plan_valid: true,
        authorization_valid: true,
        authorization_active: true,
        quorum_valid: true,
        biscuit_allowed: true,
        harness_attestation_valid: true,
        required_controls_effective: true,
        authorization_store_available: true,
        causal_store_available: true,
        key_registry_available: true,
    }
}

fn code(decision: &ControlDecision) -> &'static str {
    decision.code()
}

#[test]
fn validates_the_locked_control_schema_without_reflecting_values() {
    let registry = ContractRegistry::embedded().expect("embedded contracts must compile");
    let valid = json!({
        "schemaVersion": "libre-ai.orchestrator-control.v1",
        "id": "urn:libre-ai:control:control-1",
        "tenantId": TENANT,
        "missionId": MISSION,
        "planDigest": PLAN,
        "authorizationDigest": AUTHORIZATION,
        "action": "start",
        "expectedRevision": 7,
        "idempotencyKey": "idem_1234567890abcdef",
        "reasonCode": "orchestrator.fixture",
        "issuedAt": "2030-01-01T00:00:00Z",
        "expiresAt": "2030-01-01T00:05:00Z"
    });
    let parsed = parse_control_document(&registry, &valid).expect("valid command must parse");
    assert_eq!(parsed.action(), ControlAction::Start);

    let mut unknown = valid;
    unknown.as_object_mut().expect("object").insert(
        "privateValue".to_owned(),
        Value::String("must-not-appear".to_owned()),
    );
    let refusal = parse_control_document(&registry, &unknown).expect_err("unknown field must fail");
    assert_eq!(refusal.code(), "orchestrator.control.schema-invalid");
    assert!(!format!("{refusal:?}").contains("must-not-appear"));
}

#[test]
fn start_requires_every_preflight_control_before_run_allocation() {
    let start = command(ControlAction::Start, 7);
    let decision = evaluate_control(None, &start, NOW, Some(&preflight()), NO_COLLISION);
    assert_eq!(
        decision,
        ControlDecision::Apply(ControlApplication {
            effect: ControlEffect::AllocateRun,
            next_revision: 8,
        })
    );

    let mut substituted_preflight = preflight();
    substituted_preflight.plan_digest = "e".repeat(64);
    assert_eq!(
        code(&evaluate_control(
            None,
            &start,
            NOW,
            Some(&substituted_preflight),
            NO_COLLISION
        )),
        "orchestrator.control.identity-mismatch"
    );

    let invalid_preflights = [
        StartPreflight {
            plan_valid: false,
            ..preflight()
        },
        StartPreflight {
            authorization_valid: false,
            ..preflight()
        },
        StartPreflight {
            authorization_active: false,
            ..preflight()
        },
        StartPreflight {
            quorum_valid: false,
            ..preflight()
        },
        StartPreflight {
            biscuit_allowed: false,
            ..preflight()
        },
        StartPreflight {
            harness_attestation_valid: false,
            ..preflight()
        },
        StartPreflight {
            required_controls_effective: false,
            ..preflight()
        },
        StartPreflight {
            authorization_store_available: false,
            ..preflight()
        },
        StartPreflight {
            causal_store_available: false,
            ..preflight()
        },
        StartPreflight {
            key_registry_available: false,
            ..preflight()
        },
    ];
    for invalid in invalid_preflights {
        assert_eq!(
            code(&evaluate_control(
                None,
                &start,
                NOW,
                Some(&invalid),
                NO_COLLISION
            )),
            "orchestrator.control.preflight-failed"
        );
    }

    assert_eq!(
        code(&evaluate_control(
            Some(&state(ControlPhase::Running, 7)),
            &start,
            NOW,
            Some(&preflight()),
            NO_COLLISION
        )),
        "orchestrator.control.run-id-invalid"
    );

    let stale = command(ControlAction::Start, 6);
    assert_eq!(
        code(&evaluate_control(
            None,
            &stale,
            NOW,
            Some(&preflight()),
            NO_COLLISION
        )),
        "orchestrator.control.stale-revision"
    );
}

#[test]
fn pause_resume_and_cancel_are_closed_and_cancel_is_monotone() {
    let running = state(ControlPhase::Running, 3);
    let pause = command(ControlAction::Pause, 3);
    assert_eq!(
        evaluate_control(Some(&running), &pause, NOW, None, NO_COLLISION),
        ControlDecision::Apply(ControlApplication {
            effect: ControlEffect::Pause,
            next_revision: 4,
        })
    );

    let paused = state(ControlPhase::Paused, 4);
    let resume = command(ControlAction::Resume, 4);
    assert_eq!(
        evaluate_control(Some(&paused), &resume, NOW, None, NO_COLLISION),
        ControlDecision::Apply(ControlApplication {
            effect: ControlEffect::Resume,
            next_revision: 5,
        })
    );

    let stale_pause = command(ControlAction::Pause, 1);
    assert_eq!(
        code(&evaluate_control(
            Some(&running),
            &stale_pause,
            NOW,
            None,
            NO_COLLISION
        )),
        "orchestrator.control.stale-revision"
    );

    let stale_cancel = command(ControlAction::Cancel, 1);
    assert_eq!(
        evaluate_control(Some(&running), &stale_cancel, NOW, None, NO_COLLISION),
        ControlDecision::Apply(ControlApplication {
            effect: ControlEffect::Cancel,
            next_revision: 4,
        })
    );
}

#[test]
fn pause_block_and_terminal_states_refuse_every_new_simulated_effect() {
    assert_eq!(
        evaluate_simulated_effect(&state(ControlPhase::Running, 3)),
        SimulatedEffectDecision::Allow
    );
    let mut revoked = state(ControlPhase::Running, 3);
    revoked.authorization_active = false;
    assert_eq!(
        evaluate_simulated_effect(&revoked),
        SimulatedEffectDecision::Refuse
    );
    let mut unavailable = state(ControlPhase::Running, 3);
    unavailable.authorization_store_available = false;
    assert_eq!(
        evaluate_simulated_effect(&unavailable),
        SimulatedEffectDecision::Refuse
    );
    for phase in [
        ControlPhase::Blocked,
        ControlPhase::Paused,
        ControlPhase::Cancelled,
        ControlPhase::ResultSubmitted,
        ControlPhase::Failed,
    ] {
        assert_eq!(
            evaluate_simulated_effect(&state(phase, 3)),
            SimulatedEffectDecision::Refuse
        );
    }
}

#[test]
fn identity_substitution_and_forbidden_transitions_fail_closed() {
    let running = state(ControlPhase::Running, 3);
    let mut substituted_document = command_document(ControlAction::Pause, 3);
    substituted_document
        .as_object_mut()
        .expect("object")
        .insert(
            "tenantId".to_owned(),
            Value::String("ten_abcdef1234567890".to_owned()),
        );
    let substituted = parse_command(substituted_document);
    assert_eq!(
        code(&evaluate_control(
            Some(&running),
            &substituted,
            NOW,
            None,
            NO_COLLISION
        )),
        "orchestrator.control.identity-mismatch"
    );

    let mut stale_cancel_document = command_document(ControlAction::Cancel, 1);
    stale_cancel_document
        .as_object_mut()
        .expect("object")
        .insert(
            "authorizationDigest".to_owned(),
            Value::String("e".repeat(64)),
        );
    let stale_cancel_substitution = parse_command(stale_cancel_document);
    assert_eq!(
        code(&evaluate_control(
            Some(&running),
            &stale_cancel_substitution,
            NOW,
            None,
            NO_COLLISION
        )),
        "orchestrator.control.identity-mismatch"
    );

    let cancelled = state(ControlPhase::Cancelled, 4);
    let resume = command(ControlAction::Resume, 4);
    assert_eq!(
        code(&evaluate_control(
            Some(&cancelled),
            &resume,
            NOW,
            None,
            NO_COLLISION
        )),
        "orchestrator.control.transition-forbidden"
    );
}

#[test]
fn authorization_revocation_and_store_outages_fail_closed() {
    let pause = command(ControlAction::Pause, 3);
    let mut unavailable = state(ControlPhase::Running, 3);
    unavailable.authorization_store_available = false;
    assert_eq!(
        evaluate_control(Some(&unavailable), &pause, NOW, None, NO_COLLISION).code(),
        "orchestrator.control.authorization-store-unavailable"
    );

    let mut revoked = state(ControlPhase::Running, 3);
    revoked.authorization_active = false;
    assert_eq!(
        evaluate_control(Some(&revoked), &pause, NOW, None, NO_COLLISION).code(),
        "orchestrator.control.authorization-revoked"
    );

    assert_eq!(
        evaluate_control(
            Some(&state(ControlPhase::Running, 3)),
            &pause,
            NOW,
            None,
            CommandCollisionObservation::StoreUnavailable
        )
        .code(),
        "orchestrator.control.idempotency-store-unavailable"
    );
}

#[test]
fn identical_replay_returns_the_receipt_and_divergence_is_refused() {
    let running = state(ControlPhase::Running, 3);
    let pause = command(ControlAction::Pause, 3);
    let application = ControlApplication {
        effect: ControlEffect::Pause,
        next_revision: 4,
    };
    let receipt = CommandReceipt {
        idempotency_key: pause.idempotency_key().to_owned(),
        command_fingerprint: command_fingerprint(&pause).expect("fingerprint"),
        application: application.clone(),
    };
    assert_eq!(
        evaluate_control(
            Some(&running),
            &pause,
            NOW,
            None,
            CommandCollisionObservation::Existing(&receipt)
        ),
        ControlDecision::Idempotent {
            recorded_next_revision: application.next_revision,
        }
    );

    let wrong_store_receipt = CommandReceipt {
        idempotency_key: "idem_bbbbbbbbbbbbbbbb".to_owned(),
        ..receipt.clone()
    };
    assert_eq!(
        code(&evaluate_control(
            Some(&running),
            &pause,
            NOW,
            None,
            CommandCollisionObservation::Existing(&wrong_store_receipt)
        )),
        "orchestrator.control.idempotency-store-invalid"
    );

    let mut divergent_document = command_document(ControlAction::Pause, 3);
    divergent_document.as_object_mut().expect("object").insert(
        "reasonCode".to_owned(),
        Value::String("orchestrator.changed".to_owned()),
    );
    let divergent = parse_command(divergent_document);
    assert_eq!(
        code(&evaluate_control(
            Some(&running),
            &divergent,
            NOW,
            None,
            CommandCollisionObservation::Existing(&receipt)
        )),
        "orchestrator.control.idempotency-conflict"
    );
}

#[test]
fn revision_overflow_fails_closed() {
    let running = state(ControlPhase::Running, u64::MAX);
    let cancel = command(ControlAction::Cancel, 1);
    assert_eq!(
        code(&evaluate_control(
            Some(&running),
            &cancel,
            NOW,
            None,
            NO_COLLISION
        )),
        "orchestrator.control.revision-overflow"
    );
}

#[test]
fn invalid_time_and_expired_commands_fail_closed() {
    let running = state(ControlPhase::Running, 3);
    let pause = command(ControlAction::Pause, 3);
    assert_eq!(
        code(&evaluate_control(
            Some(&running),
            &pause,
            "not-a-time",
            None,
            NO_COLLISION
        )),
        "orchestrator.control.time-invalid"
    );
    assert_eq!(
        code(&evaluate_control(
            Some(&running),
            &pause,
            "2030-01-01T00:05:00Z",
            None,
            NO_COLLISION
        )),
        "orchestrator.control.expired"
    );
}
