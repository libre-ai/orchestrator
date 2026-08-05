use libre_ai_agent_harness::{
    ConfinementPlan, ProcessPrescription, WrapperChain, plan_wrapper_chain,
};
use std::path::Path;

fn prescription() -> ProcessPrescription {
    ProcessPrescription::new(true, true, true, true, 16)
}

fn full_plan() -> ConfinementPlan {
    ConfinementPlan::privileged(4242, 4242, Path::new("/usr/bin/setpriv"))
        .with_setsid(Path::new("/usr/bin/setsid"))
        .with_prlimit(Path::new("/usr/bin/prlimit"))
}

/// The chain is what actually applies the const-locked process block of
/// harness-profile.v1; a prescription with no mechanism behind it must refuse.
#[test]
fn the_full_prescription_composes_the_whole_wrapper_chain() {
    let chain: WrapperChain =
        plan_wrapper_chain(&prescription(), &full_plan()).expect("a fully equipped plan applies");
    let argv = chain.argv(Path::new("/bin/cat"), &["-u".to_owned()]);

    // setsid first: the worker leads its own process group, which is what
    // makes killProcessGroup meaningful at all.
    assert_eq!(argv[0], "/usr/bin/setsid");
    // prlimit carries maxProcesses.
    assert!(argv.contains(&"/usr/bin/prlimit".to_owned()));
    assert!(argv.contains(&"--nproc=16".to_owned()));
    // setpriv drops uid AND gid, clears groups, refuses new privileges and
    // empties every capability set.
    assert!(argv.contains(&"/usr/bin/setpriv".to_owned()));
    assert!(argv.contains(&"--reuid=4242".to_owned()));
    assert!(argv.contains(&"--regid=4242".to_owned()));
    assert!(argv.contains(&"--clear-groups".to_owned()));
    assert!(argv.contains(&"--no-new-privs".to_owned()));
    assert!(argv.contains(&"--inh-caps=-all".to_owned()));
    assert!(argv.contains(&"--ambient-caps=-all".to_owned()));
    assert!(argv.contains(&"--bounding-set=-all".to_owned()));
    // The worker and its own arguments close the chain.
    assert_eq!(argv[argv.len() - 2], "/bin/cat");
    assert_eq!(argv[argv.len() - 1], "-u");
}

#[test]
fn a_missing_mechanism_refuses_the_prescription_it_cannot_apply() {
    let no_setsid = ConfinementPlan::privileged(4242, 4242, Path::new("/usr/bin/setpriv"))
        .with_prlimit(Path::new("/usr/bin/prlimit"));
    assert_eq!(
        plan_wrapper_chain(&prescription(), &no_setsid)
            .expect_err("killProcessGroup without setsid is not enforceable")
            .code(),
        "harness.control_not_enforceable"
    );

    let no_prlimit = ConfinementPlan::privileged(4242, 4242, Path::new("/usr/bin/setpriv"))
        .with_setsid(Path::new("/usr/bin/setsid"));
    assert_eq!(
        plan_wrapper_chain(&prescription(), &no_prlimit)
            .expect_err("maxProcesses without prlimit is not enforceable")
            .code(),
        "harness.control_not_enforceable"
    );

    assert_eq!(
        plan_wrapper_chain(&prescription(), &ConfinementPlan::unprivileged())
            .expect_err("a dedicated identity needs setpriv and a target uid")
            .code(),
        "harness.control_not_enforceable"
    );
}

/// uid 0 is not a dedicated identity — it is the absence of one.
#[test]
fn root_is_refused_as_the_dedicated_identity() {
    let root_plan = ConfinementPlan::privileged(0, 0, Path::new("/usr/bin/setpriv"))
        .with_setsid(Path::new("/usr/bin/setsid"))
        .with_prlimit(Path::new("/usr/bin/prlimit"));
    assert_eq!(
        plan_wrapper_chain(&prescription(), &root_plan)
            .expect_err("uid 0 is not a dedicated identity")
            .code(),
        "harness.control_not_enforceable"
    );
}
