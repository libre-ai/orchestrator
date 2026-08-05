use crate::profile::HarnessProfile;
use crate::refusal::HarnessRefusal;
use std::path::{Path, PathBuf};

/// How a path is about to be touched. Writes are held to the writable set;
/// reads are held to the workspace and the denied set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathAccessKind {
    Read,
    Write,
}

/// A canonicalized path observation, produced by the host layer (which is the
/// only place canonicalization can physically happen) and judged here.
#[derive(Clone, Debug)]
pub struct PathAccess {
    canonical_path: PathBuf,
    traversed_symlink: bool,
    kind: PathAccessKind,
}

impl PathAccess {
    #[must_use]
    pub fn new(canonical_path: &Path, traversed_symlink: bool, kind: PathAccessKind) -> Self {
        Self {
            canonical_path: canonical_path.to_path_buf(),
            traversed_symlink,
            kind,
        }
    }
}

/// Minimal glob over the locked `relativePath` alphabet: `**` spans any number
/// of segments, `*` spans within one segment, everything else is literal. The
/// schema forbids every other metacharacter, so this is the whole language.
fn glob_matches(pattern: &str, path: &str) -> bool {
    fn segments_match(pattern: &[&str], path: &[&str]) -> bool {
        match (pattern.first(), path.first()) {
            (None, None) => true,
            (Some(&"**"), _) => {
                segments_match(&pattern[1..], path)
                    || (!path.is_empty() && segments_match(pattern, &path[1..]))
            }
            (Some(head), Some(first)) => {
                segment_matches(head, first) && segments_match(&pattern[1..], &path[1..])
            }
            _ => false,
        }
    }

    fn segment_matches(pattern: &str, segment: &str) -> bool {
        match pattern.split_once('*') {
            None => pattern == segment,
            Some((prefix, rest)) => {
                if !segment.starts_with(prefix) {
                    return false;
                }
                let remainder = &segment[prefix.len()..];
                (0..=remainder.len()).any(|skip| segment_matches(rest, &remainder[skip..]))
            }
        }
    }

    let pattern: Vec<&str> = pattern.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    segments_match(&pattern, &path)
}

fn any_matches(patterns: &[String], relative: &str) -> bool {
    patterns
        .iter()
        .any(|pattern| glob_matches(pattern, relative))
}

/// Journey 2 of the specification, decided on canonical facts: escape before
/// policy, denial before write rights, and a symlink that leaves the
/// workspace is named as the symlink policy it violated.
pub fn evaluate_path_access(
    workspace_root: &Path,
    access: &PathAccess,
    profile: &HarnessProfile,
) -> Result<(), HarnessRefusal> {
    let Ok(relative) = access.canonical_path.strip_prefix(workspace_root) else {
        if access.traversed_symlink {
            return Err(HarnessRefusal::SymlinkPolicyViolation);
        }
        return Err(HarnessRefusal::PathEscapesWorkspace);
    };
    let relative = relative.to_string_lossy();

    if any_matches(profile.denied_paths(), &relative) {
        return Err(HarnessRefusal::DeniedPathTouched);
    }

    match access.kind {
        PathAccessKind::Read => Ok(()),
        PathAccessKind::Write => {
            if any_matches(profile.writable_paths(), &relative) {
                Ok(())
            } else {
                Err(HarnessRefusal::WriteOutsideWritableSet)
            }
        }
    }
}
