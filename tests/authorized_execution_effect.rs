mod support;

use libre_ai_agent_orchestrator::{
    AuthorizedExecutionRefusal, EffectDecision, EffectObservation, EffectRefusal,
    evaluate_effect_attestation,
};
use libre_ai_contract_types::ContractRegistry;
use serde_json::Value;
use support::authorized_execution::valid_effect_attestation;

const ORGANIZATION: &str = "ten_1234567890abcdef";
const RUN: &str = "urn:libre-ai:run:synthetic-run-1";
const ATTEMPT: &str = "urn:libre-ai:attempt:synthetic-attempt-1";
const EMISSION: &str = "urn:libre-ai:emission:synthetic-emission-1";
const EMISSION_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const EXECUTOR_PROFILE: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

fn registry() -> ContractRegistry {
    ContractRegistry::embedded().expect("embedded registry")
}

#[allow(clippy::too_many_arguments)]
fn observation<'a>(
    expected_organization_id: &'a str,
    expected_run_id: &'a str,
    expected_attempt_id: &'a str,
    current_generation: u64,
    active_fencing: Option<u64>,
    generation_consumed: bool,
    lineage_closed: bool,
    predecessor_effects_terminal: bool,
    executor_profile_qualified: bool,
    prior_emission: Option<(&'a str, &'a str)>,
    existing_attempt_emission_id: Option<&'a str>,
) -> EffectObservation<'a> {
    EffectObservation::Authoritative {
        expected_organization_id,
        expected_run_id,
        expected_attempt_id,
        current_generation,
        active_fencing,
        expected_executor_profile_digest: EXECUTOR_PROFILE,
        generation_consumed,
        lineage_closed,
        predecessor_effects_terminal,
        executor_profile_qualified,
        prior_emission,
        existing_attempt_emission_id,
    }
}

fn valid_observation() -> EffectObservation<'static> {
    observation(
        ORGANIZATION,
        RUN,
        ATTEMPT,
        1,
        Some(7),
        false,
        false,
        true,
        true,
        None,
        None,
    )
}

fn evaluate(attestation: &Value, observation: EffectObservation<'_>) -> EffectDecision {
    evaluate_effect_attestation(&registry(), attestation, observation)
}

#[test]
fn effect_precedence_covers_every_locked_and_internal_outcome() {
    let attestation = valid_effect_attestation("committed");

    assert_eq!(
        evaluate(
            &attestation,
            observation(
                "ten_abcdef1234567890",
                "urn:libre-ai:run:other",
                "urn:libre-ai:attempt:other",
                2,
                Some(6),
                true,
                true,
                false,
                false,
                Some((
                    EMISSION,
                    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                )),
                Some("urn:libre-ai:emission:other"),
            ),
        )
        .code(),
        "organization-mismatch"
    );
    assert_eq!(
        evaluate(
            &attestation,
            observation(
                ORGANIZATION,
                "urn:libre-ai:run:other",
                "urn:libre-ai:attempt:other",
                2,
                Some(6),
                true,
                true,
                false,
                false,
                None,
                None,
            ),
        )
        .code(),
        "identity-mismatch"
    );
    assert_eq!(
        evaluate(
            &attestation,
            observation(
                ORGANIZATION,
                RUN,
                "urn:libre-ai:attempt:other",
                2,
                Some(6),
                true,
                true,
                false,
                false,
                None,
                None,
            ),
        )
        .code(),
        "attempt-mismatch"
    );
    assert_eq!(
        evaluate(
            &attestation,
            observation(
                ORGANIZATION,
                RUN,
                ATTEMPT,
                2,
                Some(6),
                true,
                true,
                false,
                false,
                None,
                None,
            ),
        )
        .code(),
        "lineage-administratively-closed"
    );
    assert_eq!(
        evaluate(
            &attestation,
            observation(
                ORGANIZATION,
                RUN,
                ATTEMPT,
                2,
                Some(6),
                true,
                false,
                false,
                false,
                None,
                None,
            ),
        )
        .code(),
        "generation-consumed"
    );
    assert_eq!(
        evaluate(
            &attestation,
            observation(
                ORGANIZATION,
                RUN,
                ATTEMPT,
                2,
                Some(6),
                false,
                false,
                false,
                false,
                None,
                None,
            ),
        )
        .code(),
        "generation-stale"
    );
    assert_eq!(
        evaluate(
            &attestation,
            observation(
                ORGANIZATION,
                RUN,
                ATTEMPT,
                1,
                Some(6),
                false,
                false,
                false,
                false,
                Some((EMISSION, EMISSION_DIGEST)),
                None,
            ),
        )
        .code(),
        "emission-duplicate"
    );
    assert_eq!(
        evaluate(
            &attestation,
            observation(
                ORGANIZATION,
                RUN,
                ATTEMPT,
                1,
                Some(6),
                false,
                false,
                false,
                false,
                Some((
                    EMISSION,
                    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                )),
                None,
            ),
        )
        .code(),
        "emission-divergent"
    );
    assert_eq!(
        evaluate(
            &attestation,
            observation(
                ORGANIZATION,
                RUN,
                ATTEMPT,
                1,
                Some(6),
                false,
                false,
                false,
                false,
                None,
                Some("urn:libre-ai:emission:other"),
            ),
        )
        .code(),
        "second-emission-for-attempt"
    );
    assert_eq!(
        evaluate(
            &attestation,
            observation(
                ORGANIZATION,
                RUN,
                ATTEMPT,
                1,
                Some(6),
                false,
                false,
                false,
                false,
                None,
                None,
            ),
        )
        .code(),
        "fencing-stale"
    );
    assert_eq!(
        evaluate(
            &attestation,
            observation(
                ORGANIZATION,
                RUN,
                ATTEMPT,
                1,
                Some(7),
                false,
                false,
                false,
                false,
                None,
                None,
            ),
        )
        .code(),
        "executor-unqualified"
    );
    let mut wrong_profile = attestation.clone();
    wrong_profile["executorProfileRef"]["digest"] = Value::String(
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_owned(),
    );
    assert_eq!(
        evaluate(&wrong_profile, valid_observation()).code(),
        "executor-unqualified"
    );

    let unknown = valid_effect_attestation("state-unknown");
    assert_eq!(
        evaluate(&unknown, valid_observation()).code(),
        "effect-state-unknown"
    );
    assert_eq!(
        evaluate(
            &attestation,
            observation(
                ORGANIZATION,
                RUN,
                ATTEMPT,
                1,
                Some(7),
                false,
                false,
                false,
                true,
                None,
                None,
            ),
        )
        .code(),
        "predecessor-effects-nonterminal"
    );

    let decision = evaluate(&attestation, valid_observation());
    assert_eq!(decision.code(), "effect-valid");
    let application = decision.application().expect("valid effect application");
    assert_eq!(application.status(), "committed");
    assert_eq!(application.effect_emission_id(), EMISSION);
    assert_eq!(application.attempt_id(), ATTEMPT);
}

#[test]
fn unknown_external_state_never_authorizes_reemission() {
    let decision = evaluate(
        &valid_effect_attestation("state-unknown"),
        valid_observation(),
    );
    assert_eq!(decision.code(), "effect-state-unknown");
    assert!(decision.application().is_none());
}

#[test]
fn unavailable_observation_refuses_closed() {
    assert_eq!(
        evaluate_effect_attestation(
            &registry(),
            &valid_effect_attestation("committed"),
            EffectObservation::Unavailable,
        )
        .code(),
        "orchestrator.authorized-execution.store-unavailable"
    );
}

#[test]
fn nonterminal_attestation_cannot_be_applied() {
    let decision = evaluate(&valid_effect_attestation("started"), valid_observation());
    assert_eq!(
        decision.code(),
        "orchestrator.authorized-execution.transition-forbidden"
    );
    assert!(decision.application().is_none());
}

#[test]
fn every_effect_decision_has_minimized_diagnostics() {
    let valid = evaluate(&valid_effect_attestation("committed"), valid_observation());
    let duplicate = evaluate(
        &valid_effect_attestation("committed"),
        observation(
            ORGANIZATION,
            RUN,
            ATTEMPT,
            1,
            Some(7),
            false,
            false,
            true,
            true,
            Some((EMISSION, EMISSION_DIGEST)),
            None,
        ),
    );
    let decisions = vec![
        valid,
        duplicate,
        EffectDecision::ContinuityBarrier,
        EffectDecision::Refused(EffectRefusal::OrganizationMismatch),
        EffectDecision::Refused(EffectRefusal::IdentityMismatch),
        EffectDecision::Refused(EffectRefusal::AttemptMismatch),
        EffectDecision::Refused(EffectRefusal::LineageAdministrativelyClosed),
        EffectDecision::Refused(EffectRefusal::GenerationConsumed),
        EffectDecision::Refused(EffectRefusal::GenerationStale),
        EffectDecision::Refused(EffectRefusal::EmissionDivergent),
        EffectDecision::Refused(EffectRefusal::SecondEmissionForAttempt),
        EffectDecision::Refused(EffectRefusal::FencingStale),
        EffectDecision::Refused(EffectRefusal::ExecutorUnqualified),
        EffectDecision::Refused(EffectRefusal::PredecessorEffectsNonterminal),
        EffectDecision::BoundaryRefused(AuthorizedExecutionRefusal::StoreUnavailable),
    ];
    let private_values = [
        ORGANIZATION,
        RUN,
        ATTEMPT,
        EMISSION,
        EMISSION_DIGEST,
        EXECUTOR_PROFILE,
        "executor_key_1",
        "synthetic-effect-1",
    ];

    for decision in decisions {
        let diagnostics = format!("{} {decision:?}", decision);
        assert!(!diagnostics.is_empty());
        for private_value in private_values {
            assert!(!diagnostics.contains(private_value), "{}", decision.code());
        }
    }
}
