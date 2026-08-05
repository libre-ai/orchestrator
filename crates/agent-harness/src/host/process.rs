use crate::confinement::{ConfinementPlan, WrapperChain};
use crate::refusal::HarnessRefusal;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
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
    chain: &WrapperChain,
) -> Result<ConfinedOutcome, HarnessRefusal> {
    let (harness_end, worker_end) =
        UnixStream::pair().map_err(|_| HarnessRefusal::ControlNotEnforceable)?;
    let worker_stdout = worker_end
        .try_clone()
        .map_err(|_| HarnessRefusal::ControlNotEnforceable)?;

    // The chain applies the prescription; an empty one runs the program
    // directly, which only a plan that prescribes nothing can produce.
    let argv = chain.argv(program, args);
    let (head, tail) = argv
        .split_first()
        .ok_or(HarnessRefusal::ControlNotEnforceable)?;
    let mut command = Command::new(head);
    command.args(tail);
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
            reap(&mut child, plan);
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
                    reap(&mut child, plan);
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

/// Kill the worker's whole process group, not just its leader.
///
/// `setsid` made the worker a group leader whose pgid equals its pid, so
/// `kill -- -<pid>` reaches every descendant. Killing the direct child alone
/// left grandchildren running past the duration bound, holding the transport
/// open (K4 security verdict on 5bee6a3, blocking finding 2). A plan without
/// setsid never reaches a spawn, so the group is always the right target;
/// the direct kill remains as the last resort if the signal cannot be sent.
fn reap(child: &mut Child, plan: &ConfinementPlan) {
    if plan.kills_process_group() {
        let group = format!("-{}", child.id());
        let sent = Command::new("/bin/kill")
            .arg("-KILL")
            .arg("--")
            .arg(&group)
            .env_clear()
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if sent {
            return;
        }
    }
    let _ = child.kill();
}
