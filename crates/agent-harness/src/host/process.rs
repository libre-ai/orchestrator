use crate::confinement::{ConfinementPlan, WrapperChain};
use crate::host::binding::RunBinding;
use crate::host::peer::verify_peer_is;
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
    capture_failed: bool,
    run_binding_proved: bool,
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

    /// The response carried this run's token ahead of its content, so it
    /// belongs to this run and not to another (`runBoundToken`).
    #[must_use]
    pub const fn run_binding_proved(&self) -> bool {
        self.run_binding_proved
    }

    /// The capture ended on a transport error rather than on EOF, so the
    /// output is short by an unknown amount. Reported rather than swallowed:
    /// an incomplete capture that looks complete is signed as complete
    /// (xhigh review of f27b3c9).
    #[must_use]
    pub const fn capture_failed(&self) -> bool {
        self.capture_failed
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
#[expect(
    clippy::too_many_arguments,
    reason = "one parameter per applied control, by design: a bundle would hide which are enforced"
)]
pub fn spawn_confined(
    program: &Path,
    args: &[String],
    payload: &[u8],
    workspace: &Path,
    limits: &SpawnLimits,
    plan: &ConfinementPlan,
    chain: &WrapperChain,
    binding: &RunBinding,
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
        .current_dir(workspace)
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

    // `verifyOsPeer`, applied: the kernel names the process on the other end,
    // and it must be the child this harness started — not a descendant that
    // inherited the descriptor (round 3 verdict on 0ab2a20).
    if plan.verifies_peer() {
        verify_peer_is(&harness_end, child.id())?;
    }

    let started = Instant::now();

    let mut sender = harness_end
        .try_clone()
        .map_err(|_| HarnessRefusal::ControlNotEnforceable)?;
    // The duration bound covers the write too: a worker that never drains
    // stdin used to block here, before the clock started and before any
    // timeout existed (round 2 security verdict, blocking finding 2).
    sender
        .set_write_timeout(Some(limits.max_duration))
        .map_err(|_| HarnessRefusal::ControlNotEnforceable)?;
    // A write that cannot complete within the bound is the bound firing, not
    // a control that could not be applied: the run ends timed out, with the
    // group reaped, instead of the harness blocking forever.
    let mut write_timed_out = false;
    if sender.write_all(&binding.frame(payload)).is_err() {
        write_timed_out = true;
        reap(&mut child, plan);
    }
    let _ = sender.shutdown(std::net::Shutdown::Write);

    let mut receiver = harness_end;
    // Only when there is something to read: after a failed write the socket
    // may already be in an error state, and a run that could not deliver its
    // payload has nothing to wait for.
    if !write_timed_out {
        receiver
            .set_read_timeout(Some(Duration::from_millis(50)))
            .map_err(|_| HarnessRefusal::ControlNotEnforceable)?;
    }

    // The frame is transport, not content: the profile's byte bound must not
    // be spent on the run token (round 3 security verdict, minor).
    let cap = usize::try_from(limits.max_output_bytes)
        .map_err(|_| HarnessRefusal::ControlNotEnforceable)?
        .saturating_add(binding.frame_len());
    let mut output = Vec::with_capacity(cap.min(65_536));
    let mut truncated = false;
    let mut timed_out = write_timed_out;
    let mut capture_failed = false;
    let mut collected: Option<std::process::ExitStatus> = None;
    let mut buffer = [0u8; 4_096];
    loop {
        // A run that could not deliver its payload has nothing to read back.
        if write_timed_out {
            break;
        }
        if started.elapsed() >= limits.max_duration {
            timed_out = true;
            reap(&mut child, plan);
            break;
        }
        match receiver.read(&mut buffer) {
            // EOF only says the descriptors are closed, not that the group
            // is gone: a descendant that closed its inherited fds outlived
            // every path that produced an attestation (round 2 security
            // verdict, blocking finding 1). The worker itself is given the
            // remaining time to exit on its own — killing a worker that just
            // finished would turn its success into a signal — and only then
            // is the group cleared of whatever it left behind.
            Ok(0) => {
                collected = wait_within(&mut child, started, limits.max_duration);
                if collected.is_none() {
                    timed_out = true;
                }
                reap(&mut child, plan);
                break;
            }
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
            Err(_) => {
                // Neither EOF nor a bound: the transport failed and what was
                // read is short by an unknown amount.
                capture_failed = true;
                reap(&mut child, plan);
                break;
            }
        }
    }

    // The frame is transport: it is verified and removed before the output
    // ever reaches the caller. A response that does not carry the token is
    // kept as-is and marked unbound rather than silently accepted.
    let (output, run_binding_proved) = match binding.unframe(&output) {
        Some(content) => (content, true),
        None => (output, false),
    };

    let exit_ok = collected
        .map_or_else(|| child.wait().ok(), Some)
        .is_some_and(|status| {
            status.success() && !timed_out && !truncated && !capture_failed && run_binding_proved
        });

    Ok(ConfinedOutcome {
        output,
        truncated,
        timed_out,
        capture_failed,
        run_binding_proved,
        exit_ok,
    })
}

/// Give the worker what remains of its duration bound to exit on its own.
///
/// Returns its status, or `None` if the bound ran out first — a worker that
/// closed its descriptors and kept running is a timeout, not a success.
fn wait_within(
    child: &mut Child,
    started: Instant,
    max_duration: Duration,
) -> Option<std::process::ExitStatus> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {
                if started.elapsed() >= max_duration {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => return None,
        }
    }
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
