use crate::refusal::HarnessRefusal;
use std::path::{Path, PathBuf};

/// The process block of `harness-profile.v1`, as prescribed. Every boolean is
/// `const: true` in the locked contract, so a profile cannot ask for less:
/// the engine either applies each one or refuses the run. There is no third
/// state, and none of these fields may be parsed away
/// (K4 verdicts on 5bee6a3, both roles).
#[derive(Clone, Copy, Debug)]
pub struct ProcessPrescription {
    dedicated_identity: bool,
    deny_privilege_escalation: bool,
    drop_ambient_capabilities: bool,
    kill_process_group: bool,
    max_processes: u32,
}

impl ProcessPrescription {
    #[must_use]
    pub const fn new(
        dedicated_identity: bool,
        deny_privilege_escalation: bool,
        drop_ambient_capabilities: bool,
        kill_process_group: bool,
        max_processes: u32,
    ) -> Self {
        Self {
            dedicated_identity,
            deny_privilege_escalation,
            drop_ambient_capabilities,
            kill_process_group,
            max_processes,
        }
    }
}

/// The mechanisms this host actually offers. Absent tool, absent mechanism,
/// refused prescription — the plan never silently downgrades.
#[derive(Clone, Debug, Default)]
pub struct ConfinementPlan {
    dedicated_uid: Option<u32>,
    dedicated_gid: Option<u32>,
    setpriv: Option<PathBuf>,
    setsid: Option<PathBuf>,
    prlimit: Option<PathBuf>,
}

impl ConfinementPlan {
    #[must_use]
    pub fn unprivileged() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn privileged(dedicated_uid: u32, dedicated_gid: u32, setpriv: &Path) -> Self {
        Self {
            dedicated_uid: Some(dedicated_uid),
            dedicated_gid: Some(dedicated_gid),
            setpriv: Some(setpriv.to_path_buf()),
            setsid: None,
            prlimit: None,
        }
    }

    #[must_use]
    pub fn with_setsid(mut self, setsid: &Path) -> Self {
        self.setsid = Some(setsid.to_path_buf());
        self
    }

    #[must_use]
    pub fn with_prlimit(mut self, prlimit: &Path) -> Self {
        self.prlimit = Some(prlimit.to_path_buf());
        self
    }

    /// A plan is privileged only when it can honour the whole identity
    /// prescription: a target uid and gid that are not root, and setpriv.
    #[must_use]
    pub const fn is_privileged(&self) -> bool {
        matches!((self.dedicated_uid, self.dedicated_gid), (Some(uid), Some(gid)) if uid != 0 && gid != 0)
            && self.setpriv.is_some()
    }

    #[must_use]
    pub const fn kills_process_group(&self) -> bool {
        self.setsid.is_some()
    }
}

/// The argv chain that applies a prescription, outermost wrapper first.
#[derive(Clone, Debug)]
pub struct WrapperChain {
    prefix: Vec<String>,
}

impl WrapperChain {
    /// The full argv of the confined worker: wrappers, then the program and
    /// its own arguments.
    #[must_use]
    pub fn argv(&self, program: &Path, args: &[String]) -> Vec<String> {
        let mut argv = self.prefix.clone();
        argv.push(program.to_string_lossy().into_owned());
        argv.extend(args.iter().cloned());
        argv
    }
}

/// Compose the chain, or refuse the prescription this host cannot apply.
///
/// Order matters: `setsid` first so the worker leads its own process group
/// (without which killing that group is meaningless), then `prlimit` for the
/// process ceiling, then `setpriv` which drops identity and capabilities last
/// — every wrapper it follows would otherwise need the privileges it removes.
pub fn plan_wrapper_chain(
    prescription: &ProcessPrescription,
    plan: &ConfinementPlan,
) -> Result<WrapperChain, HarnessRefusal> {
    let mut prefix: Vec<String> = Vec::new();

    if prescription.kill_process_group {
        let setsid = plan
            .setsid
            .as_ref()
            .ok_or(HarnessRefusal::ControlNotEnforceable)?;
        prefix.push(setsid.to_string_lossy().into_owned());
        // --wait would make setsid block on the child and defeat the direct
        // process-group signal; the harness reaps the group itself.
    }

    if prescription.max_processes > 0 {
        let prlimit = plan
            .prlimit
            .as_ref()
            .ok_or(HarnessRefusal::ControlNotEnforceable)?;
        prefix.push(prlimit.to_string_lossy().into_owned());
        prefix.push(format!("--nproc={}", prescription.max_processes));
        prefix.push("--".to_owned());
    }

    if prescription.dedicated_identity
        || prescription.deny_privilege_escalation
        || prescription.drop_ambient_capabilities
    {
        let setpriv = plan
            .setpriv
            .as_ref()
            .ok_or(HarnessRefusal::ControlNotEnforceable)?;
        // uid or gid 0 is not a dedicated identity, it is the absence of one.
        let (Some(uid), Some(gid)) = (plan.dedicated_uid, plan.dedicated_gid) else {
            return Err(HarnessRefusal::ControlNotEnforceable);
        };
        if uid == 0 || gid == 0 {
            return Err(HarnessRefusal::ControlNotEnforceable);
        }
        prefix.push(setpriv.to_string_lossy().into_owned());
        if prescription.deny_privilege_escalation {
            prefix.push("--no-new-privs".to_owned());
        }
        if prescription.dedicated_identity {
            prefix.push(format!("--reuid={uid}"));
            prefix.push(format!("--regid={gid}"));
            prefix.push("--clear-groups".to_owned());
        }
        if prescription.drop_ambient_capabilities {
            prefix.push("--inh-caps=-all".to_owned());
            prefix.push("--ambient-caps=-all".to_owned());
            prefix.push("--bounding-set=-all".to_owned());
        }
        prefix.push("--".to_owned());
    }

    Ok(WrapperChain { prefix })
}
