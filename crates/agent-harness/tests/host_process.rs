use libre_ai_agent_harness::{
    ConfinementPlan, ProcessPrescription, RunBinding, SpawnLimits, WrapperChain,
    plan_wrapper_chain, spawn_confined,
};
use std::path::Path;
use std::time::Duration;

fn limits(max_output: u64, timeout_ms: u64) -> SpawnLimits {
    SpawnLimits::new(max_output, Duration::from_millis(timeout_ms))
}

/// These tests exercise the host primitive itself, so they prescribe nothing:
/// the chain is empty and the program runs directly. A real run always goes
/// through a profile, whose const-locked prescription the chain must apply.
fn bare_chain() -> WrapperChain {
    plan_wrapper_chain(
        &ProcessPrescription::new(false, false, false, false, 0),
        &ConfinementPlan::unprivileged(),
    )
    .expect("an empty prescription needs no mechanism")
}

#[test]
fn the_payload_travels_the_private_pair_and_comes_back_bound_to_the_run() {
    let outcome = spawn_confined(
        Path::new("/bin/cat"),
        &[],
        b"run-token-42:payload",
        Path::new("/tmp"),
        &limits(4_096, 5_000),
        &ConfinementPlan::unprivileged(),
        &bare_chain(),
        &RunBinding::fresh().expect("the host must provide entropy"),
    )
    .expect("a cat worker echoes the payload");
    assert!(outcome.exit_ok());
    assert!(!outcome.truncated());
    assert_eq!(outcome.output(), b"run-token-42:payload");
}

#[test]
fn a_worker_flooding_its_output_is_truncated_and_marked() {
    let outcome = spawn_confined(
        Path::new("/bin/sh"),
        &["-c".to_owned(), "yes flood | head -c 5000".to_owned()],
        b"",
        Path::new("/tmp"),
        &limits(1_000, 5_000),
        &ConfinementPlan::unprivileged(),
        &bare_chain(),
        &RunBinding::fresh().expect("the host must provide entropy"),
    )
    .expect("the flooding worker still runs");
    assert!(outcome.truncated(), "the capture must stop at its bound");
    // The worker never returns the token, so the frame cannot be stripped and
    // the caller sees the raw capture: bounded by the content limit plus the
    // transport frame the profile's bound does not have to pay for.
    assert!(outcome.output().len() <= 1_044);
    assert!(!outcome.run_binding_proved());
}

#[test]
fn the_worker_inherits_no_environment() {
    let outcome = spawn_confined(
        Path::new("/bin/sh"),
        &["-c".to_owned(), "env".to_owned()],
        b"",
        Path::new("/tmp"),
        &limits(4_096, 5_000),
        &ConfinementPlan::unprivileged(),
        &bare_chain(),
        &RunBinding::fresh().expect("the host must provide entropy"),
    )
    .expect("the env worker runs");
    let printed = String::from_utf8_lossy(outcome.output()).to_string();
    assert!(
        !printed.contains("PATH=") && !printed.contains("HOME="),
        "no host environment may reach the worker"
    );
}

#[test]
fn a_worker_outliving_its_duration_bound_is_killed() {
    let outcome = spawn_confined(
        Path::new("/bin/sleep"),
        &["5".to_owned()],
        b"",
        Path::new("/tmp"),
        &limits(4_096, 300),
        &ConfinementPlan::unprivileged(),
        &bare_chain(),
        &RunBinding::fresh().expect("the host must provide entropy"),
    )
    .expect("the sleeping worker is reaped");
    assert!(outcome.timed_out(), "the duration bound must kill the run");
    assert!(!outcome.exit_ok());
}

/// `maxDurationSeconds` must bound the RUN, not merely the read phase: a
/// worker that never drains stdin used to block write_all before the clock
/// started (round 2 security verdict, blocking finding 2).
#[test]
fn a_worker_that_never_reads_its_input_cannot_stall_the_harness() {
    let started = std::time::Instant::now();
    let big = vec![b'x'; 1_048_576];
    let outcome = spawn_confined(
        // Exits at once without reading a byte of stdin.
        Path::new("/bin/echo"),
        &["done".to_owned()],
        &big,
        Path::new("/tmp"),
        &limits(4_096, 1_000),
        &ConfinementPlan::unprivileged(),
        &bare_chain(),
        &RunBinding::fresh().expect("entropy"),
    )
    .expect("the harness returns rather than blocking on the write");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "a payload the worker never reads must not hold the harness open"
    );
    assert!(!outcome.exit_ok());
}
