use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

fn run(args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agent-orchestrator"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

fn simulate_args(network: &str) -> Vec<&str> {
    vec![
        "simulate-v1",
        "--mission-id",
        "urn:libre-ai:mission:mission-1",
        "--started-at",
        "2026-07-16T00:00:00Z",
        "--scenario",
        "complete",
        "--max-duration-seconds",
        "2",
        "--max-tool-calls",
        "0",
        "--network",
        network,
        "--artifact-id",
        "urn:libre-ai:artifact:artifact-1",
        "--artifact-digest",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "--artifact-media-type",
        "application/json",
        "--evidence-id",
        "urn:libre-ai:evidence:report-1",
        "--evidence-digest",
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "--evidence-media-type",
        "application/json",
    ]
}

#[test]
fn simulation_writes_only_the_golden_stream() {
    let output = run(
        &simulate_args("none"),
        &fs::read(fixture("handoff.valid.json")).unwrap(),
    );
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        fs::read(fixture("complete.expected.ndjson")).unwrap()
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn network_refusal_is_stable_and_does_not_echo_input() {
    let output = run(
        &simulate_args("allowlisted"),
        &fs::read(fixture("handoff.valid.json")).unwrap(),
    );
    assert_eq!(output.status.code(), Some(66));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"orchestrator.g2_network_unsupported\n");
}

#[test]
fn validator_accepts_the_shared_stream() {
    let handoff = fixture("handoff.valid.json");
    let args = [
        "validate-events-v1",
        "--handoff",
        handoff.to_str().unwrap(),
        "--mission-id",
        "urn:libre-ai:mission:mission-1",
        "--started-at",
        "2026-07-16T00:00:00Z",
        "--max-duration-seconds",
        "2",
        "--max-tool-calls",
        "0",
        "--network",
        "none",
    ];
    let output = run(
        &args,
        &fs::read(fixture("complete.expected.ndjson")).unwrap(),
    );
    assert!(output.status.success());
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
}
