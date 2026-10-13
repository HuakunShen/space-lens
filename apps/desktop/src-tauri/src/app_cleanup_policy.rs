//! Shared cleanup policy for the engine app-scan route (`engine.rs`) and the
//! standalone app-cleanup arms (`app_cleanup.rs`) — amendment A5.
//!
//! Both routes enforce the same vocabulary: virtual `app-storage:` identifiers
//! never reach the filesystem, bundle protection is containment-both-ways
//! (A3), staged targets must stay inside the app's own pinned locations and
//! must not overlap another app's owned paths, configured roots stay refused,
//! and every filesystem probe fails closed on symlinks/escapes. Ownership
//! boundaries are computed BEFORE audit filtering: the caller supplies the
//! other apps' owned paths explicitly, so a path the audit attributed
//! elsewhere is refused even when the current entry once claimed it.
//!
//! The module is problem-type agnostic: failures surface as [`PolicyError`]
//! (a static code plus message) and each caller maps them onto its own
//! envelope (`EngineProblem` vs JSON `problem` values).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use space_lens::local_scan::ScanIssue;

/// Cap for the private recursive manifest and the physical-size walk across
/// one plan. Mirrors the standalone arm's historical bound.
pub const MAX_MANIFEST_ENTRIES: usize = 100_000;

/// A typed policy refusal. `code` is one of the contract's problem codes
/// (`Forbidden`, `InvalidRequest`, `StalePlan`); callers map it verbatim.
#[derive(Debug, Clone)]
pub struct PolicyError {
  pub code: &'static str,
  pub message: String,
}

impl PolicyError {
  pub fn new(code: &'static str, message: impl Into<String>) -> Self {
    Self {
      code,
      message: message.into(),
    }
  }

  pub fn forbidden(message: impl Into<String>) -> Self {
    Self::new("Forbidden", message)
  }

  pub fn invalid(message: impl Into<String>) -> Self {
    Self::new("InvalidRequest", message)
  }

  pub fn stale(message: impl Into<String>) -> Self {
    Self::new("StalePlan", message)
  }
}

impl std::fmt::Display for PolicyError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}: {}", self.code, self.message)
  }
}

impl std::error::Error for PolicyError {}

/// Synthetic virtual-root identifiers never name a filesystem path. Check
/// BEFORE any filesystem access (A10).
pub fn is_virtual_app_str(path: &str) -> bool {
  path.starts_with("app-storage:")
}

/// Path-flavoured form of [`is_virtual_app_str`].
pub fn is_virtual_app_path(path: &Path) -> bool {
  is_virtual_app_str(&path.to_string_lossy())
}

/// Fail closed on synthetic identifiers before any filesystem probe.
pub fn reject_virtual(path: &Path) -> Result<(), PolicyError> {
  if is_virtual_app_path(path) {
    return Err(PolicyError::forbidden(
      "the app virtual root cannot be cleaned up",
    ));
  }
  Ok(())
}

/// Bundle protection is containment-both-ways (A3): reject targets that equal,
/// sit inside, or CONTAIN the protected bundle path. Component-wise prefix
/// checks in both directions; callers pass canonical absolute paths.
pub fn bundle_overlap(path: &Path, bundle: &Path) -> bool {
  path == bundle || path.starts_with(bundle) || bundle.starts_with(path)
}

/// Enforce bundle containment for one staged target.
pub fn reject_bundle_overlap(path: &Path, bundle: Option<&Path>) -> Result<(), PolicyError> {
  if bundle.is_some_and(|bundle| bundle_overlap(path, bundle)) {
    return Err(PolicyError::forbidden(
      "app bundles cannot be cleaned up; uninstall is out of scope",
    ));
  }
  Ok(())
}

/// Cross-app ownership overlap (A5): a staged target must neither equal, sit
/// inside, nor contain any path owned by another app. `others` holds the
/// other apps' pinned location paths (computed before audit filtering).
pub fn reject_cross_app_overlap(path: &Path, others: &[PathBuf]) -> Result<(), PolicyError> {
  if others
    .iter()
    .any(|other| path == other || path.starts_with(other) || other.starts_with(path))
  {
    return Err(PolicyError::forbidden(
      "path overlaps storage owned by another app",
    ));
  }
  Ok(())
}

/// Configured roots stay refused even when an audit location coincides with
/// one: an exact canonical match rejects the target.
pub fn reject_configured_root(
  path: &Path,
  configured_roots: &[PathBuf],
) -> Result<(), PolicyError> {
  if configured_roots
    .iter()
    .any(|root| fs::canonicalize(root).is_ok_and(|canonical| path == canonical))
  {
    return Err(PolicyError::forbidden(
      "configured roots cannot be cleaned up",
    ));
  }
  Ok(())
}

/// The staged target must equal or sit under one of the scan's own pinned
/// location roots. Location roots themselves ARE cleanable (the app-scan
/// inversion of the scan-root rule); the virtual identifier never reaches
/// here (rejected above) and the bundle never does (rejected above).
pub fn require_under_own_location(
  path: &Path,
  own_locations: &[PathBuf],
) -> Result<(), PolicyError> {
  if own_locations
    .iter()
    .any(|location| path == location || path.starts_with(location))
  {
    Ok(())
  } else {
    Err(PolicyError::forbidden(
      "only this app scan's locations can be cleaned up",
    ))
  }
}

/// Full per-target authorization used by BOTH routes: virtual rejection,
/// bundle containment, own-location containment, cross-app overlap, and the
/// configured-root refusal. Filesystem state is NOT probed here.
pub fn authorize_target(
  path: &Path,
  own_locations: &[PathBuf],
  other_locations: &[PathBuf],
  bundle: Option<&Path>,
  configured_roots: &[PathBuf],
) -> Result<(), PolicyError> {
  reject_virtual(path)?;
  reject_bundle_overlap(path, bundle)?;
  require_under_own_location(path, own_locations)?;
  reject_cross_app_overlap(path, other_locations)?;
  reject_configured_root(path, configured_roots)?;
  Ok(())
}

/// Fail closed if a path lies outside `roots`, is itself a link, or its
/// canonical form drifted from the indexed string or escaped the roots.
/// Mirrors the engine's `safe_metadata`: links are never candidates.
pub fn protected_metadata(path: &Path, roots: &[PathBuf]) -> Option<fs::Metadata> {
  if !roots.iter().any(|root| path.starts_with(root)) {
    return None;
  }
  let metadata = fs::symlink_metadata(path).ok()?;
  if metadata.file_type().is_symlink() {
    return None;
  }
  let canonical = fs::canonicalize(path).ok()?;
  if canonical != path || !roots.iter().any(|root| canonical.starts_with(root)) {
    return None;
  }
  Some(metadata)
}

/// Strictest probe: every component from the filesystem root must exist and
/// be link-free, the canonical form must equal the registered string, and
/// `Group Containers` destinations are refused. Mirrors the standalone arm's
/// historical gate; both routes now share it.
pub fn strict_target_metadata(path: &Path) -> Option<fs::Metadata> {
  let mut prefix = PathBuf::new();
  for component in path.components() {
    prefix.push(component);
    let metadata = fs::symlink_metadata(&prefix).ok()?;
    if metadata.file_type().is_symlink() {
      return None;
    }
  }
  let canonical = fs::canonicalize(path).ok()?;
  if canonical != path {
    return None;
  }
  if canonical
    .components()
    .any(|component| component.as_os_str() == "Group Containers")
  {
    return None;
  }
  fs::symlink_metadata(path).ok()
}

/// Top-level (len, mtime_ms) stamp shared by plan fingerprints.
pub fn fingerprint_parts(metadata: &fs::Metadata) -> (u64, f64) {
  use std::time::UNIX_EPOCH;
  let mtime_ms = metadata
    .modified()
    .ok()
    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
    .map(|duration| duration.as_millis() as f64)
    .unwrap_or(0.0);
  (metadata.len(), mtime_ms)
}

/// Recursive-manifest (len, mtime_ms) stamp.
pub fn manifest_stamp(metadata: &fs::Metadata) -> (u64, i64) {
  use std::time::UNIX_EPOCH;
  let mtime_ms = metadata
    .modified()
    .ok()
    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
    .map(|duration| duration.as_millis() as i64)
    .unwrap_or(0);
  (metadata.len(), mtime_ms)
}

#[derive(Debug, PartialEq)]
pub enum ManifestError {
  TooLarge,
  Unverifiable,
}

/// Recursive walk collecting (len, mtime_ms) for the target and every
/// descendant. Symlinks are recorded but never followed or descended into.
/// Any unreadable directory or vanished entry fails the whole walk: cleanup
/// must never infer completeness from a partial listing.
pub fn collect_manifest(
  target: &Path,
  manifest: &mut HashMap<String, (u64, i64)>,
) -> Result<(), ManifestError> {
  fn push(
    path: &Path,
    metadata: &fs::Metadata,
    manifest: &mut HashMap<String, (u64, i64)>,
  ) -> Result<(), ManifestError> {
    if manifest.len() >= MAX_MANIFEST_ENTRIES {
      return Err(ManifestError::TooLarge);
    }
    manifest.insert(
      path.to_string_lossy().into_owned(),
      manifest_stamp(metadata),
    );
    Ok(())
  }

  let root_meta = fs::symlink_metadata(target).map_err(|_| ManifestError::Unverifiable)?;
  if root_meta.file_type().is_symlink() {
    return Err(ManifestError::Unverifiable);
  }
  push(target, &root_meta, manifest)?;
  if !root_meta.is_dir() {
    return Ok(());
  }
  let mut stack = vec![target.to_path_buf()];
  while let Some(dir) = stack.pop() {
    let entries = fs::read_dir(&dir).map_err(|_| ManifestError::Unverifiable)?;
    for entry in entries {
      let entry = entry.map_err(|_| ManifestError::Unverifiable)?;
      let path = entry.path();
      let metadata = fs::symlink_metadata(&path).map_err(|_| ManifestError::Unverifiable)?;
      push(&path, &metadata, manifest)?;
      if metadata.is_dir() && !metadata.file_type().is_symlink() {
        stack.push(path);
      }
    }
  }
  Ok(())
}

/// Measure the complete physical union of the retained (pruned, top-most)
/// targets with a protected traversal shaped like [`collect_manifest`]:
/// symlinks never followed, any unreadable directory or vanished entry fails
/// that target. Sizes are freshly measured file bytes (physical, logical
/// lengths), deduplicated by shared inode so nested/overlapping targets never
/// double-count — never index partitions (A2). Directories themselves
/// contribute no bytes; symlinks and special files contribute none.
pub fn measure_physical_union(targets: &[PathBuf]) -> Result<HashMap<PathBuf, u64>, ManifestError> {
  // Deepest-first so a shared inode attributes to the deepest target, matching
  // the audit's partition order.
  let mut ordered = targets.to_vec();
  ordered.sort_by(|a, b| {
    b.components()
      .count()
      .cmp(&a.components().count())
      .then_with(|| a.cmp(b))
  });
  let mut sizes = HashMap::with_capacity(ordered.len());
  let mut seen_files: HashSet<FileKey> = HashSet::new();
  let mut visited: usize = 0;
  for target in &ordered {
    let size = measure_one(target, &mut seen_files, &mut visited)?;
    sizes.insert(target.clone(), size);
  }
  Ok(sizes)
}

#[derive(Hash, PartialEq, Eq)]
enum FileKey {
  #[cfg(unix)]
  Inode(u64, u64),
  #[cfg(not(unix))]
  Path(PathBuf),
}

fn file_key(path: &Path, metadata: &fs::Metadata) -> FileKey {
  #[cfg(unix)]
  {
    use std::os::unix::fs::MetadataExt;
    let _ = path;
    FileKey::Inode(metadata.dev(), metadata.ino())
  }
  #[cfg(not(unix))]
  {
    let _ = metadata;
    FileKey::Path(path.to_path_buf())
  }
}

fn measure_one(
  target: &Path,
  seen_files: &mut HashSet<FileKey>,
  visited: &mut usize,
) -> Result<u64, ManifestError> {
  fn touch(visited: &mut usize) -> Result<(), ManifestError> {
    *visited += 1;
    if *visited > MAX_MANIFEST_ENTRIES {
      return Err(ManifestError::TooLarge);
    }
    Ok(())
  }

  let root_meta = fs::symlink_metadata(target).map_err(|_| ManifestError::Unverifiable)?;
  if root_meta.file_type().is_symlink() {
    return Err(ManifestError::Unverifiable);
  }
  touch(visited)?;
  if !root_meta.is_dir() {
    if root_meta.is_file() && seen_files.insert(file_key(target, &root_meta)) {
      return Ok(root_meta.len());
    }
    return Ok(0);
  }
  let mut total = 0u64;
  let mut stack = vec![target.to_path_buf()];
  while let Some(dir) = stack.pop() {
    let entries = fs::read_dir(&dir).map_err(|_| ManifestError::Unverifiable)?;
    for entry in entries {
      let entry = entry.map_err(|_| ManifestError::Unverifiable)?;
      let path = entry.path();
      let metadata = fs::symlink_metadata(&path).map_err(|_| ManifestError::Unverifiable)?;
      touch(visited)?;
      if metadata.file_type().is_symlink() {
        continue;
      }
      if metadata.is_dir() {
        stack.push(path);
      } else if metadata.is_file() && seen_files.insert(file_key(&path, &metadata)) {
        total = total.saturating_add(metadata.len());
      }
    }
  }
  Ok(total)
}

/// Per-root completeness evidence (A1/R2), derived from the walker's own
/// [`LocalScanReport`] coverage issues — no vendored-engine changes. A
/// location root is whole-folder-cleanable ONLY IF every issue at or under it
/// is a `separate-root` omission whose replacement root is one of this scan's
/// authorized roots, each itself complete (fixpoint over nesting). A truncated
/// issue list (`issuesTruncated`) means the evidence is incomplete, so the
/// answer is conservatively false.
pub fn location_folder_cleanable(
  location: &Path,
  issues: &[ScanIssue],
  authorized: &HashSet<PathBuf>,
  issues_truncated: bool,
) -> bool {
  if issues_truncated {
    return false;
  }
  let mut visiting = HashSet::new();
  root_complete(location, issues, authorized, &mut visiting)
}

fn root_complete(
  root: &Path,
  issues: &[ScanIssue],
  authorized: &HashSet<PathBuf>,
  visiting: &mut HashSet<PathBuf>,
) -> bool {
  if !visiting.insert(root.to_path_buf()) {
    return true;
  }
  let result = issues
    .iter()
    .filter(|issue| issue.path == root || issue.path.starts_with(root))
    .all(|issue| {
      if issue.reason != "separate-root" {
        return false;
      }
      // The replacement root must be an authorized root of this scan, and it
      // must itself be complete. (A self-edge cannot occur — separate-root
      // children are strictly deeper — but the `visiting` guard closes it.)
      if issue.path == root {
        return true;
      }
      authorized.contains(&issue.path) && root_complete(&issue.path, issues, authorized, visiting)
    });
  visiting.remove(root);
  result
}

/// `audit_` + 8..64 ASCII alphanumerics, matching the contract's audit-id
/// shape and the ids this shell mints.
pub fn valid_audit_id(id: &str) -> bool {
  let Some(rest) = id.strip_prefix("audit_") else {
    return false;
  };
  (8..=64).contains(&rest.len()) && rest.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn virtual_paths_rejected_before_filesystem_access() {
    assert!(is_virtual_app_str("app-storage:abc123"));
    assert!(!is_virtual_app_str("/tmp/app-storage:abc123"));
    assert!(reject_virtual(Path::new("app-storage:deadbeef")).is_err());
    // A missing virtual path is Forbidden, never a filesystem NotFound.
    assert_eq!(
      reject_virtual(Path::new("app-storage:deadbeef"))
        .unwrap_err()
        .code,
      "Forbidden"
    );
  }

  #[test]
  fn bundle_containment_runs_both_ways() {
    let bundle = PathBuf::from("/Applications/Foo.app");
    assert!(bundle_overlap(&bundle, &bundle));
    assert!(bundle_overlap(&bundle.join("Contents/MacOS/foo"), &bundle));
    assert!(bundle_overlap(&PathBuf::from("/Applications"), &bundle));
    assert!(!bundle_overlap(&PathBuf::from("/tmp/other"), &bundle));
    assert!(reject_bundle_overlap(&bundle, Some(&bundle)).is_err());
  }

  #[test]
  fn cross_app_overlap_runs_both_ways() {
    let other = PathBuf::from("/tmp/home/Library/Caches/other");
    assert!(reject_cross_app_overlap(&other, std::slice::from_ref(&other)).is_err());
    assert!(reject_cross_app_overlap(&other.join("sub"), std::slice::from_ref(&other)).is_err());
    assert!(reject_cross_app_overlap(
      &PathBuf::from("/tmp/home/Library"),
      std::slice::from_ref(&other)
    )
    .is_err());
    assert!(
      reject_cross_app_overlap(&PathBuf::from("/tmp/home/Library/Caches/own"), &[other]).is_ok()
    );
  }

  #[test]
  fn separate_root_evidence_requires_authorized_and_complete_replacements() {
    let parent = PathBuf::from("/tmp/home/a");
    let child = PathBuf::from("/tmp/home/a/child");
    let authorized: HashSet<PathBuf> = [parent.clone(), child.clone()].into_iter().collect();
    let separate = ScanIssue {
      path: child.clone(),
      reason: "separate-root".into(),
      message: String::new(),
    };
    assert!(location_folder_cleanable(
      &parent,
      std::slice::from_ref(&separate),
      &authorized,
      false
    ));
    // Truncated evidence is conservatively incomplete.
    assert!(!location_folder_cleanable(
      &parent,
      &[separate],
      &authorized,
      true
    ));
    // A foreign replacement root is not evidence.
    let foreign = ScanIssue {
      path: parent.join("foreign-child"),
      reason: "separate-root".into(),
      message: String::new(),
    };
    assert!(!location_folder_cleanable(
      &parent,
      &[foreign],
      &authorized,
      false
    ));
    // A denial under the root disables folder-level staging.
    let denied = ScanIssue {
      path: child.clone(),
      reason: "denied".into(),
      message: String::new(),
    };
    assert!(!location_folder_cleanable(
      &parent,
      &[denied],
      &authorized,
      false
    ));
    // No issues at all: cleanable.
    assert!(location_folder_cleanable(&parent, &[], &authorized, false));
  }

  #[cfg(unix)]
  #[test]
  fn physical_union_counts_shared_inodes_once_and_fails_closed() {
    let root = std::env::temp_dir().join(format!("sl-policy-union-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir_a = root.join("a");
    let dir_b = root.join("b");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    std::fs::write(dir_a.join("blob.bin"), vec![b'x'; 4096]).unwrap();
    std::fs::hard_link(dir_a.join("blob.bin"), dir_b.join("blob.bin")).unwrap();
    let sizes = measure_physical_union(&[dir_a.clone(), dir_b.clone()]).unwrap();
    assert_eq!(sizes.values().sum::<u64>(), 4096);
    // Unreadable targets fail instead of measuring zero.
    assert_eq!(
      measure_physical_union(&[root.join("missing")]).unwrap_err(),
      ManifestError::Unverifiable
    );
    let _ = std::fs::remove_dir_all(&root);
  }
}
