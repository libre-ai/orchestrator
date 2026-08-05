use crate::fs_policy::{PathAccess, PathAccessKind, evaluate_path_access};
use crate::profile::HarnessProfile;
use crate::refusal::HarnessRefusal;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Canonicalizes real paths against a real workspace root and produces the
/// facts the pure policy judges. Journey 2: a path escaping the workspace
/// root after canonicalization is refused before any process starts.
#[derive(Clone, Debug)]
pub struct WorkspaceObserver {
    root: PathBuf,
    given_root: PathBuf,
}

impl WorkspaceObserver {
    /// A root that cannot be canonicalized is a filesystem control that
    /// cannot be applied — a refusal, never a best-effort fallback.
    pub fn new(root: &Path) -> Result<Self, HarnessRefusal> {
        let canonical =
            fs::canonicalize(root).map_err(|_| HarnessRefusal::ControlNotEnforceable)?;
        Ok(Self {
            root: canonical,
            given_root: root.to_path_buf(),
        })
    }

    /// Canonicalize a raw path and report whether any symlink took part in
    /// its resolution below the workspace root — the host's own ancestry
    /// (a `/var` that is itself a link, say) is not the workspace's doing.
    /// The leaf of a write target may not exist yet: its parent is
    /// canonicalized and the plain file name re-appended.
    fn observe(&self, raw: &Path, kind: PathAccessKind) -> Result<PathAccess, HarnessRefusal> {
        let absolute = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            self.root.join(raw)
        };
        // Rebase a path expressed under the given (possibly uncanonical)
        // root onto the canonical one, so both spellings judge identically.
        let absolute = match absolute.strip_prefix(&self.given_root) {
            Ok(below) => self.root.join(below),
            Err(_) => absolute,
        };

        let traversed_symlink = match absolute.strip_prefix(&self.root) {
            Ok(below) => walks_through_symlink(&self.root, below),
            Err(_) => false,
        };

        let canonical = match fs::canonicalize(&absolute) {
            Ok(canonical) => canonical,
            Err(_) => {
                let parent = absolute
                    .parent()
                    .ok_or(HarnessRefusal::PathEscapesWorkspace)?;
                let leaf = absolute
                    .file_name()
                    .ok_or(HarnessRefusal::PathEscapesWorkspace)?;
                let parent =
                    fs::canonicalize(parent).map_err(|_| HarnessRefusal::PathEscapesWorkspace)?;
                parent.join(leaf)
            }
        };

        Ok(PathAccess::new(&canonical, traversed_symlink, kind))
    }

    /// Observe then judge: the canonical path comes back only when the pure
    /// policy admitted the access.
    pub fn judge(
        &self,
        raw: &Path,
        kind: PathAccessKind,
        profile: &HarnessProfile,
    ) -> Result<PathBuf, HarnessRefusal> {
        let access = self.observe(raw, kind)?;
        evaluate_path_access(&self.root, &access, profile)?;
        Ok(access.canonical_path().to_path_buf())
    }
}

/// Walk the components below the workspace root and report whether any is a
/// symlink — the fact distinguishing an escape through a link from a plain
/// `..` traversal.
fn walks_through_symlink(root: &Path, below: &Path) -> bool {
    let mut walked = root.to_path_buf();
    for component in below.components() {
        match component {
            Component::ParentDir => {
                walked.pop();
            }
            Component::CurDir => {}
            other => walked.push(other.as_os_str()),
        }
        if !walked.starts_with(root) {
            // Popped above the root: nothing left of the workspace's own
            // ancestry to blame on the workspace content.
            continue;
        }
        if let Ok(metadata) = fs::symlink_metadata(&walked)
            && metadata.file_type().is_symlink()
        {
            return true;
        }
    }
    false
}
