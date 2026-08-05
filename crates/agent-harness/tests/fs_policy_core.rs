use libre_ai_agent_harness::{PathAccess, PathAccessKind, evaluate_path_access, parse_profile};
use libre_ai_contract_types::ContractRegistry;
use serde_json::Value;
use std::path::Path;

const CANONICAL_PROFILE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/profiles/local-process.v1.json"
));

const ROOT: &str = "/ws";

fn profile() -> libre_ai_agent_harness::HarnessProfile {
    let document: Value =
        serde_json::from_str(CANONICAL_PROFILE).expect("the canonical profile fixture must parse");
    let registry = ContractRegistry::embedded().expect("embedded contracts must compile");
    parse_profile(&registry, &document).expect("the canonical profile must parse")
}

/// The canonical profile with its writable set replaced, for matcher tests.
fn profile_with_writable(pattern: &str) -> libre_ai_agent_harness::HarnessProfile {
    let mut document: Value =
        serde_json::from_str(CANONICAL_PROFILE).expect("the canonical profile fixture must parse");
    document["filesystem"]["writable"] = serde_json::json!([pattern]);
    let digest =
        libre_ai_agent_harness::profile_digest(&document).expect("the mutation must digest");
    document["profileDigest"] = Value::String(digest);
    let registry = ContractRegistry::embedded().expect("embedded contracts must compile");
    parse_profile(&registry, &document).expect("the mutated profile must parse")
}

fn access(path: &str, kind: PathAccessKind) -> PathAccess {
    PathAccess::new(Path::new(path), false, kind)
}

fn via_symlink(path: &str, kind: PathAccessKind) -> PathAccess {
    PathAccess::new(Path::new(path), true, kind)
}

#[test]
fn writes_inside_the_writable_set_and_reads_inside_the_workspace_pass() {
    let profile = profile();
    evaluate_path_access(
        Path::new(ROOT),
        &access("/ws/out/result.txt", PathAccessKind::Write),
        &profile,
    )
    .expect("a write inside the writable set passes");
    evaluate_path_access(
        Path::new(ROOT),
        &access("/ws/in/data.txt", PathAccessKind::Read),
        &profile,
    )
    .expect("a read of the read-only set passes");
    evaluate_path_access(
        Path::new(ROOT),
        &access("/ws/scratch.txt", PathAccessKind::Read),
        &profile,
    )
    .expect("a read inside the workspace outside every set still passes");
}

#[test]
fn a_write_outside_the_writable_set_is_refused() {
    let refusal = evaluate_path_access(
        Path::new(ROOT),
        &access("/ws/in/data.txt", PathAccessKind::Write),
        &profile(),
    )
    .expect_err("the read-only set is not writable");
    assert_eq!(refusal.code(), "harness.write_outside_writable_set");

    let unlisted = evaluate_path_access(
        Path::new(ROOT),
        &access("/ws/scratch.txt", PathAccessKind::Write),
        &profile(),
    )
    .expect_err("an unlisted path is not writable");
    assert_eq!(unlisted.code(), "harness.write_outside_writable_set");
}

#[test]
fn a_denied_path_is_refused_for_every_access_kind() {
    let profile = profile();
    for kind in [PathAccessKind::Read, PathAccessKind::Write] {
        let refusal = evaluate_path_access(Path::new(ROOT), &access("/ws/.env", kind), &profile)
            .expect_err("the denied set blocks every access");
        assert_eq!(refusal.code(), "harness.denied_path_touched");
    }
    let nested = evaluate_path_access(
        Path::new(ROOT),
        &access("/ws/vendor/.git/config", PathAccessKind::Read),
        &profile,
    )
    .expect_err("the **-glob of the denied set reaches nested paths");
    assert_eq!(nested.code(), "harness.denied_path_touched");
}

#[test]
fn a_path_escaping_the_workspace_after_canonicalization_is_refused() {
    let refusal = evaluate_path_access(
        Path::new(ROOT),
        &access("/etc/passwd", PathAccessKind::Read),
        &profile(),
    )
    .expect_err("a canonical path outside the workspace root is refused");
    assert_eq!(refusal.code(), "harness.path_escapes_workspace");
}

#[test]
fn a_symlink_resolving_outside_the_workspace_names_the_symlink_policy() {
    let refusal = evaluate_path_access(
        Path::new(ROOT),
        &via_symlink("/somewhere/else", PathAccessKind::Read),
        &profile(),
    )
    .expect_err("a symlink escaping the workspace violates the policy");
    assert_eq!(refusal.code(), "harness.symlink_policy_violation");
}

/// A path segment is not a byte string: slicing it at arbitrary offsets
/// panics mid-decision. Found by the xhigh review of f27b3c9; a panic is not
/// a refusal, and the worker chooses the file name.
#[test]
fn a_non_ascii_file_name_is_judged_not_panicked_on() {
    // An intra-segment star is what drives the matcher into the segment's
    // bytes; `é` sits astride the offset the old matcher sliced at.
    let profile = profile_with_writable("*.txt");
    evaluate_path_access(
        Path::new(ROOT),
        &access("/ws/café.txt", PathAccessKind::Write),
        &profile,
    )
    .expect("a non-ASCII name matching the writable pattern passes");

    let refusal = evaluate_path_access(
        Path::new(ROOT),
        &access("/ws/café.bin", PathAccessKind::Write),
        &profile,
    )
    .expect_err("a non-ASCII name outside the writable set is refused, not a panic");
    assert_eq!(refusal.code(), "harness.write_outside_writable_set");
}

/// The matcher must not explode combinatorially on a pattern the profile is
/// free to carry: the decision has no timeout of its own.
#[test]
fn a_multi_star_pattern_decides_in_bounded_time() {
    let started = std::time::Instant::now();
    let refusal = evaluate_path_access(
        Path::new(ROOT),
        &access(&format!("/ws/{}", "a".repeat(64)), PathAccessKind::Write),
        &profile_with_writable("a*a*a*a*a*a*a*b"),
    )
    .expect_err("the crafted name matches no writable pattern");
    assert_eq!(refusal.code(), "harness.write_outside_writable_set");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(1),
        "the glob decision must stay bounded"
    );
}
