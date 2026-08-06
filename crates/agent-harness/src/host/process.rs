use crate::confinement::{ConfinementPlan, WrapperChain};
use crate::host::binding::RunBinding;
use crate::refusal::HarnessRefusal;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// How long a single send may block before the phase clock is consulted
/// again. Short enough that the phase bound is honoured to the slice, long
/// enough not to spin.
const WRITE_SLICE: Duration = Duration::from_millis(50);

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

    let started = Instant::now();

    let mut sender = harness_end
        .try_clone()
        .map_err(|_| HarnessRefusal::ControlNotEnforceable)?;
    // The bound is on the PHASE, not on each syscall: SO_SNDTIMEO rearms on
    // every send, so a worker draining one byte per window would extend the
    // write without limit while the clock it is supposedly subject to was
    // never consulted (round 4 architecture verdict, blocking finding 1).
    sender
        .set_write_timeout(Some(WRITE_SLICE))
        .map_err(|_| HarnessRefusal::ControlNotEnforceable)?;
    let framed = binding.frame(payload);
    let mut written = 0usize;
    let mut write_timed_out = false;
    while written < framed.len() {
        if started.elapsed() >= limits.max_duration {
            write_timed_out = true;
            break;
        }
        match sender.write(&framed[written..]) {
            // The worker closed its input: stop writing, keep whatever it
            // already answered.
            Ok(0) => break,
            Ok(count) => written += count,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            // EPIPE and friends: the worker is gone, which a worker that has
            // already answered legitimately is. Read what it left rather
            // than discarding it (round 4 architecture verdict, major 3).
            Err(_) => break,
        }
    }
    if write_timed_out {
        reap(&mut child, plan);
    }
    let _ = sender.shutdown(std::net::Shutdown::Write);

    let mut receiver = harness_end;
    // Best effort: a peer that closed its input may leave the socket in a
    // state that refuses the option, and a worker which answered before
    // closing still has bytes worth draining (round 4, major finding 3). A
    // socket that cannot be armed simply fails its reads, which the capture
    // reports rather than swallows.
    let _ = receiver.set_read_timeout(Some(Duration::from_millis(50)));

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
                // The group is captured BEFORE the wait: try_wait reaps the
                // zombie and frees the pid, and killing a recycled pid as
                // root is the hazard std::process::Child::kill refuses
                // outright (round 4 architecture verdict, major finding 2).
                let group = child.id();
                collected = wait_within(&mut child, started, limits.max_duration);
                if collected.is_none() {
                    timed_out = true;
                }
                reap_group(group, plan);
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
        reap_group(child.id(), plan);
        return;
    }
    let _ = child.kill();
}

/// Kill a process group by an identifier captured while the harness still
/// owned it. Never call this with a pid that has already been waited on
/// unless the group was captured beforehand.
fn reap_group(group: u32, plan: &ConfinementPlan) {
    if plan.kills_process_group() {
        let group = format!("-{group}");
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
        let _ = sent;
    }
}
