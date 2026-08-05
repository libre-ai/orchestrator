use libre_ai_agent_harness::{PathAccessKind, WorkspaceObserver, parse_profile};
use libre_ai_contract_types::ContractRegistry;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;

const CANONICAL_PROFILE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/profiles/local-process.v1.json"
));

/// A throwaway real workspace with an `out/` writable dir, an outside file
/// and a symlink escaping the root — the adversarial fixtures of the spec.
struct Fixture {
    base: PathBuf,
    workspace: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let base =
            std::env::temp_dir().join(format!("harness-host-fs-{label}-{}", std::process::id()));
        let workspace = base.join("ws");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(workspace.join("out")).expect("the workspace fixture must build");
        fs::write(base.join("secret.txt"), b"outside").expect("the outside file must build");
        symlink(base.join("secret.txt"), workspace.join("link-out"))
            .expect("the escaping symlink must build");
        Self { base, workspace }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn profile() -> libre_ai_agent_harness::HarnessProfile {
    let registry = ContractRegistry::embedded().expect("embedded contracts must compile");
    let document = serde_json::from_str(CANONICAL_PROFILE).expect("the fixture must parse");
    parse_profile(&registry, &document).expect("the canonical profile must parse")
}

#[test]
fn a_symlink_escaping_the_workspace_is_named_as_the_symlink_policy() {
    let fixture = Fixture::new("symlink");
    let observer =
        WorkspaceObserver::new(&fixture.workspace).expect("the workspace root must canonicalize");
    let refusal = observer
        .judge(
            &fixture.workspace.join("link-out"),
            PathAccessKind::Read,
            &profile(),
        )
        .expect_err("a symlink resolving outside the workspace violates the policy");
    assert_eq!(refusal.code(), "harness.symlink_policy_violation");
}

#[test]
fn a_dotdot_traversal_surviving_naive_normalization_is_an_escape() {
    let fixture = Fixture::new("dotdot");
    let observer =
        WorkspaceObserver::new(&fixture.workspace).expect("the workspace root must canonicalize");
    let raw = fixture.workspace.join("out/../../secret.txt");
    let refusal = observer
        .judge(&raw, PathAccessKind::Read, &profile())
        .expect_err("a canonical path outside the root is refused");
    assert_eq!(refusal.code(), "harness.path_escapes_workspace");
}

#[test]
fn a_write_to_a_denied_path_produces_its_code_never_a_warning() {
    let fixture = Fixture::new("denied");
    let observer =
        WorkspaceObserver::new(&fixture.workspace).expect("the workspace root must canonicalize");
    let refusal = observer
        .judge(
            &fixture.workspace.join(".env"),
            PathAccessKind::Write,
            &profile(),
        )
        .expect_err("the denied set blocks the write");
    assert_eq!(refusal.code(), "harness.denied_path_touched");
}

#[test]
fn a_legitimate_write_target_resolves_to_its_canonical_path() {
    let fixture = Fixture::new("write");
    let observer =
        WorkspaceObserver::new(&fixture.workspace).expect("the workspace root must canonicalize");
    let canonical = observer
        .judge(
            &fixture.workspace.join("out/result.txt"),
            PathAccessKind::Write,
            &profile(),
        )
        .expect("a write inside the writable set passes");
    assert!(canonical.ends_with("out/result.txt"));
}
