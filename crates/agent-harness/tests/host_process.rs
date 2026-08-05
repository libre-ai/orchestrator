use libre_ai_agent_harness::{ConfinementPlan, SpawnLimits, spawn_confined};
use std::path::Path;
use std::time::Duration;

fn limits(max_output: u64, timeout_ms: u64) -> SpawnLimits {
    SpawnLimits::new(max_output, Duration::from_millis(timeout_ms))
}

#[test]
fn the_payload_travels_the_private_pair_and_comes_back_bound_to_the_run() {
    let outcome = spawn_confined(
        Path::new("/bin/cat"),
        &[],
        b"run-token-42:payload",
        &limits(4_096, 5_000),
        &ConfinementPlan::unprivileged(),
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
        &limits(1_000, 5_000),
        &ConfinementPlan::unprivileged(),
    )
    .expect("the flooding worker still runs");
    assert!(outcome.truncated(), "the capture must stop at its bound");
    assert!(outcome.output().len() <= 1_000);
}

#[test]
fn the_worker_inherits_no_environment() {
    let outcome = spawn_confined(
        Path::new("/bin/sh"),
        &["-c".to_owned(), "env".to_owned()],
        b"",
        &limits(4_096, 5_000),
        &ConfinementPlan::unprivileged(),
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
        &limits(4_096, 300),
        &ConfinementPlan::unprivileged(),
    )
    .expect("the sleeping worker is reaped");
    assert!(outcome.timed_out(), "the duration bound must kill the run");
    assert!(!outcome.exit_ok());
}
