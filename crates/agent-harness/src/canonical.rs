use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write;

const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Canonical unpadded base64url (RFC 4648 §5) — the signature encoding of
/// `contracts/agent-orchestration/SEMANTICS.md`. Hand-rolled because the
/// dependency surface of this crate is a closed allowlist.
pub(crate) fn base64url_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).copied().map_or(0, u32::from);
        let b2 = chunk.get(2).copied().map_or(0, u32::from);
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(char::from(
            BASE64URL[usize::try_from((triple >> 18) & 63).unwrap_or(0)],
        ));
        out.push(char::from(
            BASE64URL[usize::try_from((triple >> 12) & 63).unwrap_or(0)],
        ));
        if chunk.len() > 1 {
            out.push(char::from(
                BASE64URL[usize::try_from((triple >> 6) & 63).unwrap_or(0)],
            ));
        }
        if chunk.len() > 2 {
            out.push(char::from(
                BASE64URL[usize::try_from(triple & 63).unwrap_or(0)],
            ));
        }
    }
    out
}

/// Strict decoder: rejects padding, foreign characters and non-canonical
/// trailing bits (re-encode must reproduce the input exactly).
pub(crate) fn base64url_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut values = [0u32; 4];
        for (index, character) in chunk.iter().enumerate() {
            values[index] = u32::try_from(BASE64URL.iter().position(|x| x == character)?).ok()?;
        }
        let triple = (values[0] << 18) | (values[1] << 12) | (values[2] << 6) | values[3];
        out.push(u8::try_from((triple >> 16) & 255).ok()?);
        if chunk.len() > 2 {
            out.push(u8::try_from((triple >> 8) & 255).ok()?);
        }
        if chunk.len() > 3 {
            out.push(u8::try_from(triple & 255).ok()?);
        }
    }
    if base64url_encode(&out) != text {
        return None;
    }
    Some(out)
}

/// A 64-char lowercase hex digest as its 32 raw bytes.
pub(crate) fn hex_to_bytes32(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (index, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let text = std::str::from_utf8(chunk).ok()?;
        bytes[index] = u8::from_str_radix(text, 16).ok()?;
    }
    Some(bytes)
}

/// SHA-256 of raw bytes, lowercase hex — the content address of a file that
/// is not a JSON document to canonicalize.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let mut digest = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        let _ = write!(&mut digest, "{byte:02x}");
    }
    digest
}

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
