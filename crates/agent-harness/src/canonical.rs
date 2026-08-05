use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write;

/// SHA-256 over the RFC 8785 (JCS) canonical form, lowercase hex — the digest
/// preimage rule every locked contract shares
/// (`contracts/agent-orchestration/SEMANTICS.md`).
pub(crate) fn canonical_sha256(value: &Value) -> Option<String> {
    let canonical = serde_jcs::to_vec(value).ok()?;
    let mut digest = String::with_capacity(64);
    for byte in Sha256::digest(canonical) {
        write!(&mut digest, "{byte:02x}").ok()?;
    }
    Some(digest)
}
