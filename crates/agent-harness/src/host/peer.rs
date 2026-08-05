use crate::refusal::HarnessRefusal;
use std::os::unix::net::UnixStream;

/// Whether this host can answer "who is on the other end of this socket".
///
/// `SO_PEERCRED` is a Linux interface; the canonical profile names Linux
/// platforms only, so a host that cannot answer refuses rather than falling
/// back on an argument (round 3 refuted the by-construction one: the peer is
/// the child AND every descendant holding the inherited descriptor, which is
/// exactly what this call distinguishes).
#[must_use]
pub const fn peer_credentials_readable() -> bool {
    cfg!(target_os = "linux")
}

/// Verify that the process on the other end of the transport is the child the
/// harness spawned, not a descendant it forked.
///
/// This is `workerTransport.verifyOsPeer`, applied: the kernel names the peer,
/// and the harness compares that name to the process it started.
#[cfg(target_os = "linux")]
pub(crate) fn verify_peer_is(stream: &UnixStream, expected_pid: u32) -> Result<(), HarnessRefusal> {
    let credentials = rustix::net::sockopt::socket_peercred(stream)
        .map_err(|_| HarnessRefusal::ControlNotEnforceable)?;
    let peer = credentials.pid.as_raw_nonzero().get();
    let expected =
        i32::try_from(expected_pid).map_err(|_| HarnessRefusal::ControlNotEnforceable)?;
    if peer == expected {
        Ok(())
    } else {
        // A different process answers on this socket: the run's transport is
        // not isolated to the worker the harness started.
        Err(HarnessRefusal::ControlNotEnforceable)
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn verify_peer_is(
    _stream: &UnixStream,
    _expected_pid: u32,
) -> Result<(), HarnessRefusal> {
    // Refuse rather than assume: a host that cannot name the peer cannot
    // apply the control the profile prescribes.
    Err(HarnessRefusal::ControlNotEnforceable)
}
