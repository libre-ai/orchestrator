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
