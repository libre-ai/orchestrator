use crate::refusal::HarnessRefusal;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Output and duration bounds of one confined spawn.
#[derive(Clone, Debug)]
pub struct SpawnLimits {
    max_output_bytes: u64,
    max_duration: Duration,
}

impl SpawnLimits {
    #[must_use]
    pub const fn new(max_output_bytes: u64, max_duration: Duration) -> Self {
        Self {
            max_output_bytes,
            max_duration,
        }
    }
}

/// The process-identity controls a resolved run applies. The unprivileged
/// plan exists for the host primitive alone: a real run only ever reaches
/// this module after `resolve_controls` admitted the full profile, which
/// requires the privileged fields on the platforms the profile names.
#[derive(Clone, Debug, Default)]
pub struct ConfinementPlan {
    dedicated_uid: Option<u32>,
    setpriv: Option<PathBuf>,
}

impl ConfinementPlan {
    #[must_use]
    pub fn unprivileged() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn privileged(dedicated_uid: u32, setpriv: &Path) -> Self {
        Self {
            dedicated_uid: Some(dedicated_uid),
            setpriv: Some(setpriv.to_path_buf()),
        }
    }

    #[must_use]
    pub const fn is_privileged(&self) -> bool {
        self.dedicated_uid.is_some() && self.setpriv.is_some()
    }
}

/// What one confined spawn actually did — bounded output, truncation and
/// timeout facts included, so the pure layers judge and the attestation
/// binds facts, never hopes.
#[derive(Clone, Debug)]
pub struct ConfinedOutcome {
    output: Vec<u8>,
    truncated: bool,
    timed_out: bool,
    exit_ok: bool,
}

impl ConfinedOutcome {
    #[must_use]
    pub fn output(&self) -> &[u8] {
        &self.output
    }

    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }

    #[must_use]
    pub const fn timed_out(&self) -> bool {
        self.timed_out
    }

    #[must_use]
    pub const fn exit_ok(&self) -> bool {
        self.exit_ok
    }
}

/// Journey 3: spawn the worker inside the applied controls. The private
/// anonymous socketpair is both transport ends created by the harness before
/// the child exists — `private-unix-socket` with no name to hijack — and the
/// environment is cleared: nothing of the host reaches the worker.
pub fn spawn_confined(
    program: &Path,
    args: &[String],
    payload: &[u8],
    limits: &SpawnLimits,
    plan: &ConfinementPlan,
) -> Result<ConfinedOutcome, HarnessRefusal> {
    let (harness_end, worker_end) =
        UnixStream::pair().map_err(|_| HarnessRefusal::ControlNotEnforceable)?;
    let worker_stdout = worker_end
        .try_clone()
        .map_err(|_| HarnessRefusal::ControlNotEnforceable)?;

    let mut command = match (&plan.setpriv, plan.dedicated_uid) {
        (Some(setpriv), Some(uid)) => {
            let mut wrapped = Command::new(setpriv);
            wrapped
                .arg("--no-new-privs")
                .arg("--reuid")
                .arg(uid.to_string())
                .arg("--clear-groups")
                .arg("--")
                .arg(program)
                .args(args);
            wrapped
        }
        _ => {
            let mut direct = Command::new(program);
            direct.args(args);
            direct
        }
    };
    command
        .env_clear()
        .stdin(Stdio::from(OwnedFd::from(worker_end)))
        .stdout(Stdio::from(OwnedFd::from(worker_stdout)))
        .stderr(Stdio::null());

    let mut child = command
        .spawn()
        .map_err(|_| HarnessRefusal::ControlNotEnforceable)?;
    // Command keeps the worker-side descriptors alive until dropped; holding
    // them here would deny the reader its EOF when the worker exits.
    drop(command);

    let mut sender = harness_end
        .try_clone()
        .map_err(|_| HarnessRefusal::ControlNotEnforceable)?;
    sender
        .write_all(payload)
        .map_err(|_| HarnessRefusal::ControlNotEnforceable)?;
    sender
        .shutdown(std::net::Shutdown::Write)
        .map_err(|_| HarnessRefusal::ControlNotEnforceable)?;

    let started = Instant::now();
    let mut receiver = harness_end;
    receiver
        .set_read_timeout(Some(Duration::from_millis(50)))
        .map_err(|_| HarnessRefusal::ControlNotEnforceable)?;

    let cap = usize::try_from(limits.max_output_bytes)
        .map_err(|_| HarnessRefusal::ControlNotEnforceable)?;
    let mut output = Vec::with_capacity(cap.min(65_536));
    let mut truncated = false;
    let mut timed_out = false;
    let mut buffer = [0u8; 4_096];
    loop {
        if started.elapsed() >= limits.max_duration {
            timed_out = true;
            let _ = child.kill();
            break;
        }
        match receiver.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                let room = cap.saturating_sub(output.len());
                if count > room {
                    output.extend_from_slice(&buffer[..room]);
                    truncated = true;
                    // The bound is reached: stop buffering, reap the worker.
                    let _ = child.kill();
                    break;
                }
                output.extend_from_slice(&buffer[..count]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => break,
        }
    }

    let exit_ok = child
        .wait()
        .map(|status| status.success() && !timed_out && !truncated)
        .unwrap_or(false);

    Ok(ConfinedOutcome {
        output,
        truncated,
        timed_out,
        exit_ok,
    })
}
