use crate::canonical::base64url_encode;
use crate::refusal::HarnessRefusal;
use std::fs;
use std::io::Read;

/// The frame separator between the run token and the payload. A newline is
/// enough because the token alphabet is base64url, which contains none.
const FRAME: u8 = b'\n';

/// A per-run secret the worker must return, realising the locked profile's
/// `runBoundToken`.
///
/// Without it the transport prescription was inert: the socketpair proved the
/// worker could not be impersonated by a third party, but nothing proved that
/// what came back belonged to THIS run rather than to a previous one whose
/// descriptor lingered (K4 rounds 1 and 2). The token is generated per run,
/// sent ahead of the payload, and required back ahead of the response.
#[derive(Clone, Debug)]
pub struct RunBinding {
    token: String,
}

impl RunBinding {
    /// Draw 32 bytes from the host's entropy source. A run whose token cannot
    /// be drawn is refused rather than run unbound.
    pub fn fresh() -> Result<Self, HarnessRefusal> {
        let mut file =
            fs::File::open("/dev/urandom").map_err(|_| HarnessRefusal::ControlNotEnforceable)?;
        let mut bytes = [0u8; 32];
        file.read_exact(&mut bytes)
            .map_err(|_| HarnessRefusal::ControlNotEnforceable)?;
        Ok(Self {
            token: base64url_encode(&bytes),
        })
    }

    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The bytes actually written to the worker: the token, the frame, then
    /// the caller's payload untouched.
    pub(crate) fn frame(&self, payload: &[u8]) -> Vec<u8> {
        let mut framed = Vec::with_capacity(self.token.len() + 1 + payload.len());
        framed.extend_from_slice(self.token.as_bytes());
        framed.push(FRAME);
        framed.extend_from_slice(payload);
        framed
    }

    /// Strip the token from a response, or report that it was not there.
    /// The caller never sees the frame: it is transport, not content.
    pub(crate) fn unframe(&self, response: &[u8]) -> Option<Vec<u8>> {
        let expected = self.token.as_bytes();
        let (head, rest) = response.split_at_checked(expected.len())?;
        // Constant-time equality is not the property at stake — the token is
        // returned by the confined worker, not guessed by a remote party —
        // but the comparison is byte-exact.
        if head != expected {
            return None;
        }
        match rest.split_first() {
            Some((&FRAME, tail)) => Some(tail.to_vec()),
            // A response that is exactly the token, with nothing after it.
            None => Some(Vec::new()),
            Some(_) => None,
        }
    }
}
