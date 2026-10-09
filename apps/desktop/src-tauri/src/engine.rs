//! Desktop engine store: the Rust twin of `packages/host` scan sessions.
//! Same contract shapes (camelCase JSON), same node-id scheme (sha1(path)[..24]),
//! same safety rules: trash-only cleanup, fingerprint re-verification, and
//! `truncated`/`omitted` accounting computed — never guessed.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use space_lens::local_scan::{
  scan_local_directory_with_progress, LocalScanCancellation, LocalScanNode, LocalScanProgress,
  LocalScanReport, ScanCoverage, ScanVolume,
};
use space_lens::{scan_directory, IgnoredMode, ScanNode, ScanOptions};

// ---------------------------------------------------------------- wire types

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TreeNodeSummary {
  pub id: String,
  pub name: String,
  pub path: String,
  pub size: u64,
  pub depth: u32,
  pub ignored: bool,
  pub collapsed: bool,
  pub has_children: bool,
  pub child_count: usize,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub logical_size: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub is_directory: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub scan_state: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub skip_reason: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TreeSliceNode {
  #[serde(flatten)]
  pub summary: TreeNodeSummary,
  pub children: Vec<TreeSliceNode>,
  pub omitted_bytes: u64,
  pub omitted_count: usize,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TreeSlice {
  pub scan_id: String,
  pub focus_node: TreeNodeSummary,
  pub ancestors: Vec<TreeNodeSummary>,
  pub tree: TreeSliceNode,
  pub total_size: u64,
  pub truncated: bool,
  pub omitted_bytes: u64,
  pub omitted_count: usize,
  pub generated_at: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChildrenPage {
  pub scan_id: String,
  pub node_id: String,
  pub items: Vec<TreeNodeSummary>,
  pub offset: usize,
  pub limit: usize,
  pub total: usize,
  pub sort: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatus {
  pub scan_id: String,
  pub state: String,
  pub message: String,
  pub progress: Option<f64>,
  pub current_path: Option<String>,
  pub bytes_scanned: u64,
  pub entries_scanned: usize,
  pub root_ids: Vec<String>,
  pub label: Option<String>,
  pub updated_at: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub coverage: Option<ScanCoverage>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub volumes: Option<Vec<ScanVolume>>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScanSession {
  pub scan_id: String,
  pub root_ids: Vec<String>,
  pub created_at: String,
  pub label: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScanStartRequest {
  pub paths: Vec<String>,
  #[serde(default)]
  pub local_only: bool,
  #[serde(default)]
  pub ignore_hidden: bool,
  #[serde(default = "default_true")]
  pub respect_gitignore: bool,
  #[serde(default)]
  pub ignored_mode: WireIgnoredMode,
  #[serde(default)]
  pub label: Option<String>,
}

#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "lowercase")]
pub enum WireIgnoredMode {
  #[default]
  Summarize,
  Exclude,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TreeSliceRequest {
  pub scan_id: String,
  pub node_id: String,
  pub depth: u32,
  pub max_children_per_node: usize,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChildrenPageRequest {
  pub scan_id: String,
  pub node_id: String,
  pub offset: usize,
  pub limit: usize,
  pub sort: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum DiscoveryKind {
  LargeFiles,
  Caches,
  Gitignored,
}

pub fn default_discovery_limit() -> usize {
  200
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiscoveryRequest {
  pub scan_id: String,
  pub kind: DiscoveryKind,
  #[serde(default)]
  pub min_size: u64,
  #[serde(default)]
  pub offset: usize,
  #[serde(default = "default_discovery_limit")]
  pub limit: usize,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryItem {
  pub node: TreeNodeSummary,
  pub parent_id: Option<String>,
  pub category: String,
  pub is_directory: bool,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryPage {
  pub scan_id: String,
  pub kind: DiscoveryKind,
  pub items: Vec<DiscoveryItem>,
  pub total: usize,
  pub total_size: u64,
  pub offset: usize,
  pub limit: usize,
}

/// Plain data copied under the engine lock; filesystem walks happen after
/// releasing it. Discovery always respects the scan's hidden-file setting.
pub struct DiscoveryInput {
  roots: Vec<PathBuf>,
  ignore_hidden: bool,
}

pub struct DiscoveryData {
  index: HashMap<String, IndexEntry>,
  items: HashMap<DiscoveryKind, Vec<DiscoveryItem>>,
}

/// What a discovery request needs before any filesystem work. `Cached` means
/// the scan already holds results for the kind; `Ready` carries results
/// derived in memory from an indexed protected scan report (no filesystem
/// access at all); `Walk` requires the legacy filesystem rescan.
pub enum PreparedDiscovery {
  Cached,
  Ready(DiscoveryData),
  Walk(DiscoveryInput),
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanupPlanRequest {
  pub scan_id: String,
  pub node_ids: Vec<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EntryFingerprint {
  pub size: u64,
  pub mtime_ms: f64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlanEntry {
  pub path: String,
  pub size: u64,
  pub reason: String,
  pub preset: Option<String>,
  pub ignored: bool,
  pub fingerprint: EntryFingerprint,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlan {
  pub plan_id: String,
  pub scan_id: String,
  pub mode: String,
  pub entries: Vec<PlanEntry>,
  pub total_size: u64,
  pub errors: Vec<String>,
  pub created_at: String,
  pub expires_at: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanupExecuteRequest {
  pub plan_id: String,
  pub confirm: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CleanupOutcome {
  pub trashed: Vec<TrashedEntry>,
  pub bytes_freed: u64,
  pub failed: Vec<FailedEntry>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TrashedEntry {
  pub path: String,
  pub size: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FailedEntry {
  pub path: String,
  pub code: String,
  pub message: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScanTarget {
  pub id: String,
  pub label: String,
  pub path: String,
  pub kind: String,
  pub description: String,
  pub size: u64,
  /// Bytes used on the volume holding this root, when the OS reports it.
  /// Absent (not zero) when capacity is unknown, so the UI can show
  /// "unknown" instead of "empty".
  #[serde(skip_serializing_if = "Option::is_none")]
  pub used: Option<u64>,
  pub source: String,
  pub removable: bool,
}

/// Used and total bytes for the filesystem holding `path`, via statvfs.
/// Returns `None` when the OS cannot say (missing path, unsupported FS),
/// so callers surface "unknown" rather than a fabricated zero.
///
/// Block size is `f_frsize` alone. APFS reports `f_bsize` as the *optimal
/// transfer size* (1 MiB), not the block size — taking the max of the two
/// inflated every capacity by 256×.
#[cfg(unix)]
pub fn volume_capacity(path: &Path) -> Option<(u64, u64)> {
  use std::ffi::CString;
  use std::os::unix::ffi::OsStrExt;
  let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
  // SAFETY: statvfs only reads through the pointer for the call's duration;
  // `c_path` outlives it and the buffer is a valid zeroed struct.
  let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
  if unsafe { libc::statvfs(c_path.as_ptr(), &mut stats) } != 0 {
    return None;
  }
  #[allow(clippy::unnecessary_cast)]
  let block = stats.f_frsize as u64;
  let total = (stats.f_blocks as u64).saturating_mul(block);
  let free = (stats.f_bfree as u64).saturating_mul(block);
  if total == 0 {
    return None;
  }
  Some((total.saturating_sub(free), total))
}

/// Windows reports capacity through a UTF-16 directory path, not statvfs.
#[cfg(windows)]
pub fn volume_capacity(path: &Path) -> Option<(u64, u64)> {
  use std::os::windows::ffi::OsStrExt;
  use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
  let mut directory: Vec<u16> = path.as_os_str().encode_wide().collect();
  if directory.contains(&0) {
    return None;
  }
  directory.push(0);
  let mut total = 0;
  let mut free = 0;
  // SAFETY: the directory is terminated and lives through the call; both
  // output pointers refer to writable u64 values. Both values use the caller's
  // quota: disk-wide free space must not be subtracted from quota-limited total.
  let ok = unsafe {
    GetDiskFreeSpaceExW(
      directory.as_ptr(),
      &mut free,
      &mut total,
      std::ptr::null_mut(),
    )
  };
  if ok == 0 || total == 0 {
    return None;
  }
  Some((total.saturating_sub(free), total))
}

fn default_true() -> bool {
  true
}

fn now_iso() -> String {
  let now = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default();
  let secs = now.as_secs();
  let millis = now.subsec_millis();
  // A minimal RFC3339 formatter keeps the desktop shell dependency-free of a
  // full chrono-heavy stack while matching the contract's ISO instants.
  let days = secs / 86_400;
  let rem = secs % 86_400;
  let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
  // civil-from-days (Howard Hinnant's algorithm)
  let z = days as i64 + 719_468;
  let era = z.div_euclid(146_097);
  let doe = z.rem_euclid(146_097);
  let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
  let y = yoe + era * 400;
  let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
  let mp = (5 * doy + 2) / 153;
  let d = doy - (153 * mp + 2) / 5 + 1;
  let month = if mp < 10 { mp + 3 } else { mp - 9 };
  let year = if month <= 2 { y + 1 } else { y };
  format!("{year:04}-{month:02}-{d:02}T{h:02}:{m:02}:{s:02}.{millis:03}Z")
}

fn node_id_of(path: &str) -> String {
  let mut hasher = Sha1::new();
  hasher.update(path.as_bytes());
  let digest = hasher.finalize();
  digest
    .iter()
    .take(12)
    .map(|byte| format!("{byte:02x}"))
    .collect()
}

fn new_id(prefix: &str) -> String {
  let nanos = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .subsec_nanos();
  let mut hasher = Sha1::new();
  hasher.update(nanos.to_le_bytes());
  hasher.update(std::process::id().to_le_bytes());
  let digest = hasher.finalize();
  format!(
    "{prefix}{}",
    digest
      .iter()
      .take(6)
      .map(|byte| format!("{byte:02x}"))
      .collect::<String>()
  )
}

#[derive(Clone)]
struct IndexEntry {
  id: String,
  name: String,
  path: String,
  size: u64,
  depth: u32,
  ignored: bool,
  collapsed: bool,
  parent: Option<String>,
  child_ids: Vec<String>,
  fingerprint: Option<EntryFingerprint>,
  cleanup: Option<CleanupSnapshot>,
  logical_size: Option<u64>,
  is_directory: Option<bool>,
  scan_state: Option<String>,
  skip_reason: Option<String>,
}

#[derive(Clone)]
struct CleanupSnapshot {
  size: u64,
  ignored: bool,
  fingerprint: Option<EntryFingerprint>,
}

impl CleanupSnapshot {
  fn from_entry(entry: &IndexEntry) -> Self {
    Self {
      size: entry.size,
      ignored: entry.ignored,
      fingerprint: entry.fingerprint.clone(),
    }
  }
}

impl IndexEntry {
  fn summary(&self) -> TreeNodeSummary {
    TreeNodeSummary {
      id: self.id.clone(),
      name: self.name.clone(),
      path: self.path.clone(),
      size: self.size,
      depth: self.depth,
      ignored: self.ignored,
      collapsed: self.collapsed,
      has_children: !self.child_ids.is_empty(),
      child_count: self.child_ids.len(),
      logical_size: self.logical_size,
      is_directory: self.is_directory,
      scan_state: self.scan_state.clone(),
      skip_reason: self.skip_reason.clone(),
    }
  }
}

struct ScanRecord {
  session: ScanSession,
  status: ScanStatus,
  index: HashMap<String, IndexEntry>,
  root_ids: Vec<String>,
  ignore_hidden: bool,
  discovery: HashMap<DiscoveryKind, Vec<DiscoveryItem>>,
  local_only: bool,
  // Whether the protected scan read .gitignore content while indexing;
  // mirrors the host's gitignoreClassified flag for the discovery gate.
  gitignore_classified: bool,
  // Lease for a prepared/running worker. Cancellation keeps it occupied;
  // only finish_local_scan releases it after the actual walk has returned.
  active_worker: Option<LocalScanCancellation>,
}

/// Prepared under a short lock, executed with all filesystem work outside it.
pub struct LocalScanTask {
  pub scan_id: String,
  pub options: ScanOptions,
  pub cancellation: LocalScanCancellation,
}

/// Engine error surfaced as a typed problem over IPC.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineProblem {
  pub code: String,
  pub message: String,
}

type EngineResult<T> = Result<T, EngineProblem>;

fn problem(code: &str, message: impl Into<String>) -> EngineProblem {
  EngineProblem {
    code: code.to_string(),
    message: message.into(),
  }
}

/// The whole desktop engine behind one session.
pub struct EngineStore {
  roots: Vec<PathBuf>,
  scans: HashMap<String, ScanRecord>,
  plans: HashMap<String, (CleanupPlan, std::time::Instant)>,
}

impl EngineStore {
  pub fn new(roots: Vec<PathBuf>) -> Self {
    Self {
      roots,
      scans: HashMap::new(),
      plans: HashMap::new(),
    }
  }

  pub fn roots(&self) -> Vec<ScanTarget> {
    self
      .roots
      .iter()
      .enumerate()
      .map(|(index, root)| {
        // Capacity is read live per root so the picker can show used/free
        // before any scan runs. A root the OS cannot stat keeps `size` 0 and
        // no `used`, which the UI renders as "unknown".
        let (size, used) = volume_capacity(root)
          .map(|(used, total)| (total, Some(used)))
          .unwrap_or((0, None));
        ScanTarget {
          id: format!("root_{index}"),
          label: root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| root.to_string_lossy().to_string()),
          path: root.to_string_lossy().to_string(),
          kind: "folder".into(),
          description: String::new(),
          size,
          used,
          source: "preset".into(),
          removable: false,
        }
      })
      .collect()
  }

  /// Mounted volumes with capacity for the pre-scan drive list.
  /// Display-only facts: a mount point grants no scan capability by itself.
  /// Candidates are the startup volume plus entries under the platform's
  /// mount points; anything statvfs cannot report is skipped, never
  /// fabricated.
  pub fn volumes(&self) -> Vec<ScanVolume> {
    #[cfg(unix)]
    let mut mounts = vec![PathBuf::from("/")];
    #[cfg(windows)]
    let mounts: Vec<PathBuf> = {
      // SAFETY: GetLogicalDrives has no arguments or borrowed memory.
      let drives = unsafe { windows_sys::Win32::Storage::FileSystem::GetLogicalDrives() };
      (0..26)
        .filter(|index| drives & (1 << index) != 0)
        .map(|index| PathBuf::from(format!("{}:\\", char::from(b'A' + index))))
        .collect()
    };
    #[cfg(target_os = "macos")]
    if let Ok(entries) = fs::read_dir("/Volumes") {
      mounts.extend(
        entries
          .filter_map(|entry| entry.ok())
          .map(|entry| entry.path())
          // `/Volumes/Macintosh HD` is the firmlink twin of `/` — same disk
          // twice in the list otherwise. Recovery/Preboot/VM are system
          // partitions with no user data to browse.
          .filter(|path| {
            path.file_name().map_or(true, |name| {
              !matches!(
                name.to_string_lossy().as_ref(),
                "Macintosh HD" | "Recovery" | "Preboot" | "VM" | "Boot" | "Home"
              )
            })
          })
          .filter(|path| path.is_dir()),
      );
    }
    #[cfg(target_os = "linux")]
    for base in ["/mnt", "/media"] {
      if let Ok(entries) = fs::read_dir(base) {
        mounts.extend(
          entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_dir()),
        );
      }
    }
    let mut volumes: Vec<ScanVolume> = mounts
      .iter()
      .filter_map(|mount| {
        let (used, total) = volume_capacity(mount)?;
        let free = total.saturating_sub(used);
        Some(ScanVolume {
          path: mount.clone(),
          total_bytes: total,
          available_bytes: free,
          free_bytes: free,
          is_local: true,
        })
      })
      .collect();
    volumes.sort_by(|a, b| {
      if a.path == Path::new("/") {
        std::cmp::Ordering::Less
      } else if b.path == Path::new("/") {
        std::cmp::Ordering::Greater
      } else {
        a.path.cmp(&b.path)
      }
    });
    volumes.dedup_by(|a, b| a.path == b.path);
    volumes
  }

  /// Runs the synchronous engine scan. Call from `spawn_blocking`.
  pub fn start_scan(&mut self, request: ScanStartRequest) -> EngineResult<ScanSession> {
    // Branch before canonicalize/lstat. Protected work delegates all metadata
    // access to the guarded scanner and indexes only its returned snapshot.
    if request.local_only {
      let (session, task) = self.prepare_local_scan(request)?;
      let report = scan_local_directory_with_progress(task.options, task.cancellation, |_| {});
      let status = self.finish_local_scan(&session.scan_id, report)?;
      if status.state != "ready" {
        return Err(problem("Unavailable", status.message));
      }
      return Ok(self.record(&session.scan_id)?.session.clone());
    }
    if self
      .scans
      .values()
      .any(|record| record.active_worker.is_some() || record.status.state == "scanning")
    {
      return Err(problem(
        "LimitExceeded",
        "at most one native scan may run at once",
      ));
    }
    let mut resolved = Vec::with_capacity(request.paths.len());
    for path in &request.paths {
      let canonical = fs::canonicalize(path)
        .map_err(|_| problem("InvalidRequest", format!("path does not exist: {path}")))?;
      let allowed = self.roots.iter().any(|root| {
        fs::canonicalize(root)
          .map(|root| canonical == root || canonical.starts_with(&root))
          .unwrap_or(false)
      });
      if !allowed {
        return Err(problem(
          "Forbidden",
          format!(
            "path is outside the roots this desktop session serves: {}",
            canonical.display()
          ),
        ));
      }
      resolved.push(canonical);
    }
    let scan_id = new_id("scan_");
    let created_at = now_iso();
    let label = request
      .label
      .clone()
      .or_else(|| resolved.first().map(|p| p.to_string_lossy().to_string()));

    let options = ScanOptions {
      directories: resolved,
      ignore_hidden: request.ignore_hidden,
      full_path: true,
      respect_gitignore: request.respect_gitignore,
      ignored_mode: match request.ignored_mode {
        WireIgnoredMode::Summarize => IgnoredMode::Summarize,
        WireIgnoredMode::Exclude => IgnoredMode::Exclude,
      },
      follow_symlinks: false,
    };
    let tree = scan_directory(options);
    if tree.is_empty() {
      return Err(problem(
        "InvalidRequest",
        "the engine produced no tree for the requested paths",
      ));
    }

    let mut index = HashMap::new();
    let mut root_ids = Vec::new();
    let mut bytes = 0u64;
    let mut entries = 0usize;
    for root in &tree {
      root_ids.push(walk_index(root, None, 0, &mut index));
      bytes += root.size;
      entries += count_nodes(root);
    }
    for root_id in &root_ids {
      index
        .entry(root_id.clone())
        .and_modify(|entry| entry.depth = 0);
    }

    let status = ScanStatus {
      scan_id: scan_id.clone(),
      state: "ready".into(),
      message: String::new(),
      progress: None,
      current_path: None,
      bytes_scanned: bytes,
      entries_scanned: entries,
      root_ids: root_ids.clone(),
      label: label.clone(),
      updated_at: now_iso(),
      coverage: None,
      volumes: None,
    };
    let session = ScanSession {
      scan_id: scan_id.clone(),
      root_ids: root_ids.clone(),
      created_at,
      label,
    };
    self.scans.insert(
      scan_id,
      ScanRecord {
        session: session.clone(),
        status,
        index,
        root_ids,
        ignore_hidden: request.ignore_hidden,
        discovery: HashMap::new(),
        local_only: false,
        gitignore_classified: request.respect_gitignore,
        active_worker: None,
      },
    );
    Ok(session)
  }

  /// Pure lexical validation only; no filesystem probes before protection.
  pub fn prepare_local_scan(
    &mut self,
    request: ScanStartRequest,
  ) -> EngineResult<(ScanSession, LocalScanTask)> {
    if !request.local_only {
      return Err(problem(
        "InvalidRequest",
        "protected scan requires localOnly: true",
      ));
    }
    if request.paths.is_empty() {
      return Err(problem(
        "InvalidRequest",
        "choose at least one local scan path",
      ));
    }
    if self
      .scans
      .values()
      .any(|record| record.active_worker.is_some() || record.status.state == "scanning")
    {
      return Err(problem(
        "LimitExceeded",
        "at most one native scan may run at once",
      ));
    }
    let served: Vec<PathBuf> = self
      .roots
      .iter()
      .filter_map(|root| lexical_absolute(root).ok())
      .collect();
    let mut resolved = Vec::<PathBuf>::new();
    for path in &request.paths {
      let root = lexical_absolute(Path::new(path))?;
      if !served.iter().any(|served| root.starts_with(served)) {
        return Err(problem(
          "Forbidden",
          "path is outside the roots this desktop session serves",
        ));
      }
      // Explicit nested roots may be separate mounted/firmlinked volumes;
      // the protected walker excludes them from the parent traversal.
      if !resolved.contains(&root) {
        resolved.push(root);
      }
    }
    let scan_id = new_id("scan_");
    let label = request.label.or_else(|| {
      resolved
        .first()
        .map(|path| path.to_string_lossy().into_owned())
    });
    let session = ScanSession {
      scan_id: scan_id.clone(),
      root_ids: Vec::new(),
      created_at: now_iso(),
      label: label.clone(),
    };
    let cancellation = LocalScanCancellation::default();
    let status = ScanStatus {
      scan_id: scan_id.clone(),
      state: "scanning".into(),
      message: "Scanning local storage with cloud downloads blocked".into(),
      progress: None,
      current_path: None,
      bytes_scanned: 0,
      entries_scanned: 0,
      root_ids: Vec::new(),
      label,
      updated_at: now_iso(),
      coverage: None,
      volumes: None,
    };
    self.scans.insert(
      scan_id.clone(),
      ScanRecord {
        session: session.clone(),
        status,
        index: HashMap::new(),
        root_ids: Vec::new(),
        ignore_hidden: request.ignore_hidden,
        discovery: HashMap::new(),
        local_only: true,
        gitignore_classified: request.respect_gitignore,
        active_worker: Some(cancellation.clone()),
      },
    );
    Ok((
      session,
      LocalScanTask {
        scan_id,
        options: ScanOptions {
          directories: resolved,
          ignore_hidden: request.ignore_hidden,
          full_path: true,
          respect_gitignore: request.respect_gitignore,
          ignored_mode: match request.ignored_mode {
            WireIgnoredMode::Summarize => IgnoredMode::Summarize,
            WireIgnoredMode::Exclude => IgnoredMode::Exclude,
          },
          follow_symlinks: false,
        },
        cancellation,
      },
    ))
  }

  pub fn update_local_scan_progress(
    &mut self,
    scan_id: &str,
    progress: LocalScanProgress,
  ) -> EngineResult<ScanStatus> {
    let record = self
      .scans
      .get_mut(scan_id)
      .ok_or_else(|| problem("NotFound", "scan vanished"))?;
    if record.status.state == "scanning" && record.local_only {
      record.status.bytes_scanned = progress.bytes_scanned;
      record.status.entries_scanned = progress.entries_scanned.try_into().unwrap_or(usize::MAX);
      record.status.current_path = progress
        .current_path
        .map(|path| path.to_string_lossy().into_owned());
      record.status.message = format!(
        "{} files · {} folders · {} skipped · {} denied",
        progress.files, progress.directories, progress.skipped_count, progress.denied_count
      );
      record.status.updated_at = now_iso();
    }
    Ok(record.status.clone())
  }

  pub fn finish_local_scan(
    &mut self,
    scan_id: &str,
    result: std::io::Result<LocalScanReport>,
  ) -> EngineResult<ScanStatus> {
    let record = self
      .scans
      .get_mut(scan_id)
      .ok_or_else(|| problem("NotFound", "scan vanished"))?;
    if !record.local_only {
      return Err(problem("InvalidRequest", "not a protected scan"));
    }
    record.active_worker = None;
    // Cancellation is terminal even if the scanner raced to return a report.
    if record.status.state != "scanning" {
      return Ok(record.status.clone());
    }
    match result {
      Ok(report) => {
        let mut index = HashMap::new();
        let root_ids: Vec<String> = report
          .nodes
          .iter()
          .map(|node| walk_local_index(node, None, 0, &mut index))
          .collect();
        record.status.bytes_scanned = report.nodes.iter().map(|node| node.size).sum();
        record.status.entries_scanned = index.len();
        record.status.state = "ready".into();
        record.status.message = format!(
          "Local scan complete · {} skipped · {} denied",
          report.coverage.skipped_count, report.coverage.denied_count
        );
        record.status.coverage = Some(report.coverage);
        record.status.volumes = Some(report.volumes);
        record.root_ids = root_ids.clone();
        record.session.root_ids = root_ids.clone();
        record.status.root_ids = root_ids;
        record.index = index;
      }
      Err(error) => {
        record.status.state = if error.kind() == std::io::ErrorKind::Interrupted {
          "cancelled"
        } else {
          "failed"
        }
        .into();
        record.status.message = format!("Protected local scan stopped: {error}");
      }
    }
    record.status.current_path = None;
    record.status.updated_at = now_iso();
    Ok(record.status.clone())
  }

  pub fn cancel_scan(&mut self, scan_id: &str) -> EngineResult<ScanStatus> {
    let record = self
      .scans
      .get_mut(scan_id)
      .ok_or_else(|| problem("NotFound", "scan vanished"))?;
    if record.local_only && record.status.state == "scanning" {
      if let Some(cancellation) = &record.active_worker {
        cancellation.cancel();
      }
      record.status.state = "cancelled".into();
      record.status.message = "Local scan cancelled".into();
      record.status.current_path = None;
      record.status.updated_at = now_iso();
    }
    Ok(record.status.clone())
  }

  pub fn cancel_all_scans(&mut self) {
    let scan_ids: Vec<String> = self.scans.keys().cloned().collect();
    for scan_id in scan_ids {
      let _ = self.cancel_scan(&scan_id);
    }
  }

  pub fn status(&mut self, scan_id: &str) -> EngineResult<ScanStatus> {
    Ok(self.record(scan_id)?.status.clone())
  }

  pub fn list(&mut self) -> Vec<ScanStatus> {
    let mut statuses: Vec<ScanStatus> = self
      .scans
      .values()
      .map(|record| record.status.clone())
      .collect();
    statuses.sort_by(|a, b| a.scan_id.cmp(&b.scan_id));
    statuses
  }

  pub fn slice(&self, request: &TreeSliceRequest) -> EngineResult<TreeSlice> {
    let record = self.ready(request.scan_id.as_str())?;
    let focus = record.index.get(&request.node_id).ok_or_else(|| {
      problem(
        "NotFound",
        format!("no node {} in scan {}", request.node_id, request.scan_id),
      )
    })?;
    let mut ancestors = Vec::new();
    let mut cursor = focus.parent.clone();
    while let Some(parent_id) = cursor {
      let parent = record
        .index
        .get(&parent_id)
        .ok_or_else(|| problem("InternalError", "index lost a parent"))?;
      ancestors.insert(0, parent.summary());
      cursor = parent.parent.clone();
    }
    let built = build_slice_node(
      &record.index,
      focus,
      request.depth,
      request.max_children_per_node,
    );
    Ok(TreeSlice {
      scan_id: request.scan_id.clone(),
      focus_node: focus.summary(),
      ancestors,
      tree: built.node,
      total_size: focus.size,
      truncated: built.truncated,
      omitted_bytes: built.omitted_bytes,
      omitted_count: built.omitted_count,
      generated_at: now_iso(),
    })
  }

  pub fn children(&self, request: &ChildrenPageRequest) -> EngineResult<ChildrenPage> {
    let record = self.ready(request.scan_id.as_str())?;
    let entry = record.index.get(&request.node_id).ok_or_else(|| {
      problem(
        "NotFound",
        format!("no node {} in scan {}", request.node_id, request.scan_id),
      )
    })?;
    let mut children: Vec<TreeNodeSummary> = entry
      .child_ids
      .iter()
      .filter_map(|id| record.index.get(id))
      .map(|child| child.summary())
      .collect();
    match request.sort.as_str() {
      "name" => children.sort_by(|a, b| a.name.cmp(&b.name)),
      "path" => children.sort_by(|a, b| a.path.cmp(&b.path)),
      _ => children.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name))),
    }
    let total = children.len();
    let items = children
      .into_iter()
      .skip(request.offset)
      .take(request.limit)
      .collect();
    Ok(ChildrenPage {
      scan_id: request.scan_id.clone(),
      node_id: request.node_id.clone(),
      items,
      offset: request.offset,
      limit: request.limit,
      total,
      sort: request.sort.clone(),
    })
  }

  /// Prepares a discovery request. Protected scans derive all three views
  /// from the already-indexed report in memory (mirroring the host's
  /// protectedDiscovery); legacy scans snapshot walk inputs for the
  /// filesystem rescan that happens outside the engine lock.
  pub fn prepare_discovery(&self, request: &DiscoveryRequest) -> EngineResult<PreparedDiscovery> {
    validate_discovery(request)?;
    let record = self.ready(&request.scan_id)?;
    if record.local_only {
      if request.kind == DiscoveryKind::Gitignored && !record.gitignore_classified {
        return Err(problem(
          "UnsupportedOperation",
          "this protected scan did not classify gitignored paths; scan with respectGitignore enabled",
        ));
      }
      if record.discovery.contains_key(&request.kind) {
        return Ok(PreparedDiscovery::Cached);
      }
      // Pure in-memory classification of the measured report; the derived
      // items add no index entries, so browse topology stays untouched.
      let items = protected_discovery_items(&record.index);
      return Ok(PreparedDiscovery::Ready(DiscoveryData {
        index: HashMap::new(),
        items,
      }));
    }
    if record.discovery.contains_key(&request.kind) {
      return Ok(PreparedDiscovery::Cached);
    }
    Ok(PreparedDiscovery::Walk(discovery_walk_input(record)))
  }

  pub fn finish_discovery(
    &mut self,
    request: &DiscoveryRequest,
    data: Option<DiscoveryData>,
  ) -> EngineResult<DiscoveryPage> {
    validate_discovery(request)?;
    let record = self
      .scans
      .get_mut(&request.scan_id)
      .ok_or_else(|| problem("NotFound", format!("no scan {}", request.scan_id)))?;
    if let Some(data) = data {
      // Existing browse topology stays unchanged, including summarized
      // directories. New nodes/ancestors make discovery IDs usable by
      // slice and cleanup without expanding the ordinary browse view.
      for (id, entry) in data.index {
        let snapshot = CleanupSnapshot::from_entry(&entry);
        record
          .index
          .entry(id)
          // Keep original browse sizes/flags/topology intact; cleanup uses
          // the later discovery snapshot shown in discovery results.
          .and_modify(|old| old.cleanup = Some(snapshot))
          .or_insert(entry);
      }
      for (kind, items) in data.items {
        record.discovery.entry(kind).or_insert(items);
      }
    }
    let rows = record
      .discovery
      .get(&request.kind)
      .ok_or_else(|| problem("InternalError", "discovery results were not computed"))?;
    let filtered: Vec<_> = rows
      .iter()
      .filter(|item| item.node.size >= request.min_size)
      .collect();
    Ok(DiscoveryPage {
      scan_id: request.scan_id.clone(),
      kind: request.kind,
      total: filtered.len(),
      total_size: filtered.iter().map(|item| item.node.size).sum(),
      items: filtered
        .into_iter()
        .skip(request.offset)
        .take(request.limit)
        .cloned()
        .collect(),
      offset: request.offset,
      limit: request.limit,
    })
  }

  pub fn plan(&mut self, request: &CleanupPlanRequest) -> EngineResult<CleanupPlan> {
    let record = self.ready(request.scan_id.as_str())?;
    let scan_roots: Vec<PathBuf> = record
      .root_ids
      .iter()
      .filter_map(|id| record.index.get(id))
      .map(|entry| PathBuf::from(&entry.path))
      .collect();
    let mut selected = Vec::new();
    for node_id in &request.node_ids {
      let entry = record.index.get(node_id).ok_or_else(|| {
        problem(
          "InvalidRequest",
          format!("plan references unknown node {node_id}"),
        )
      })?;
      // A skipped or partial entry was never fully measured, no matter what
      // the path looks like now — mirror the UI selection rules and reject
      // before any metadata probe (which could touch a replaced path).
      if matches!(entry.scan_state.as_deref(), Some("skipped" | "partial")) {
        return Err(problem(
          "InvalidRequest",
          format!(
            "path was skipped or partially scanned; cleanup needs a complete entry: {}",
            entry.path
          ),
        ));
      }
      guard_cleanup_roots(Path::new(&entry.path), &scan_roots, &self.roots)?;
      let snapshot = entry
        .cleanup
        .clone()
        .unwrap_or_else(|| CleanupSnapshot::from_entry(entry));
      let metadata = safe_metadata(Path::new(&entry.path), &scan_roots).ok_or_else(|| {
        problem(
          "StalePlan",
          format!(
            "path is missing, outside the scan roots, or traverses a symbolic link: {}",
            entry.path
          ),
        )
      })?;
      let current = fingerprint_of(&metadata);
      // Protected index entries carry no scan-time fingerprint
      // (walk_local_index never probes the filesystem), so the freshly
      // measured fingerprint becomes the plan baseline and execute
      // re-verifies against it. Entries holding a scan-time or
      // discovery-time fingerprint keep the stricter comparison.
      let baseline = match snapshot.fingerprint.clone() {
        Some(fingerprint) if fingerprint != current => {
          return Err(problem(
            "StaleSnapshot",
            format!("path changed since scanning: {}; scan again", entry.path),
          ));
        }
        Some(fingerprint) => fingerprint,
        None => current,
      };
      selected.push((entry, snapshot, baseline));
    }
    selected.sort_by(|a, b| {
      Path::new(&a.0.path)
        .components()
        .count()
        .cmp(&Path::new(&b.0.path).components().count())
        .then_with(|| a.0.path.cmp(&b.0.path))
    });
    let mut entries: Vec<PlanEntry> = Vec::new();
    for (entry, snapshot, fingerprint) in selected {
      if entries
        .iter()
        .any(|planned| Path::new(&entry.path).starts_with(&planned.path))
      {
        continue;
      }
      entries.push(PlanEntry {
        path: entry.path.clone(),
        size: snapshot.size,
        reason: if snapshot.ignored {
          "ignored"
        } else {
          "staged"
        }
        .into(),
        preset: None,
        ignored: snapshot.ignored,
        fingerprint,
      });
    }
    let total_size = entries.iter().map(|entry| entry.size).sum();
    let plan = CleanupPlan {
      plan_id: new_id("plan_"),
      scan_id: request.scan_id.clone(),
      mode: "trash".into(),
      total_size,
      errors: Vec::new(),
      entries,
      created_at: now_iso(),
      expires_at: now_iso(),
    };
    self.plans.insert(
      plan.plan_id.clone(),
      (plan.clone(), std::time::Instant::now()),
    );
    Ok(plan)
  }

  /// Moves every planned path to the OS trash after re-verifying all
  /// fingerprints. One mismatch fails the whole plan closed.
  pub fn execute(&mut self, request: &CleanupExecuteRequest) -> EngineResult<CleanupOutcome> {
    if !request.confirm {
      return Err(problem("InvalidRequest", "cleanup requires confirm: true"));
    }
    let (plan, planned_at) = self
      .plans
      .remove(&request.plan_id)
      .ok_or_else(|| problem("NotFound", format!("no plan {}", request.plan_id)))?;
    if planned_at.elapsed().as_secs() > 600 {
      return Err(problem(
        "StalePlan",
        "plan expired; plan again from a fresh scan",
      ));
    }
    let mut changed = 0usize;
    let record = self.ready(&plan.scan_id)?;
    let scan_roots: Vec<PathBuf> = record
      .root_ids
      .iter()
      .filter_map(|id| record.index.get(id))
      .map(|entry| PathBuf::from(&entry.path))
      .collect();
    for entry in &plan.entries {
      guard_cleanup_roots(Path::new(&entry.path), &scan_roots, &self.roots)?;
      let matches = safe_metadata(Path::new(&entry.path), &scan_roots)
        .map(|metadata| fingerprint_of(&metadata) == entry.fingerprint)
        .unwrap_or(false);
      if !matches {
        changed += 1;
      }
    }
    if changed > 0 {
      return Err(problem(
        "StalePlan",
        format!(
          "{changed} of {} entries changed since the plan was made",
          plan.entries.len()
        ),
      ));
    }
    let mut outcome = CleanupOutcome {
      trashed: Vec::new(),
      bytes_freed: 0,
      failed: Vec::new(),
    };
    for entry in &plan.entries {
      match trash::delete(&entry.path) {
        Ok(()) => {
          outcome.bytes_freed += entry.size;
          outcome.trashed.push(TrashedEntry {
            path: entry.path.clone(),
            size: entry.size,
          });
        }
        Err(error) => outcome.failed.push(FailedEntry {
          path: entry.path.clone(),
          code: "Unavailable".into(),
          message: format!("trash failed: {error}"),
        }),
      }
    }
    Ok(outcome)
  }

  fn record(&self, scan_id: &str) -> EngineResult<&ScanRecord> {
    self
      .scans
      .get(scan_id)
      .ok_or_else(|| problem("NotFound", format!("no scan {scan_id}")))
  }

  fn ready(&self, scan_id: &str) -> EngineResult<&ScanRecord> {
    let record = self.record(scan_id)?;
    if record.status.state != "ready" {
      return Err(problem(
        "StaleSnapshot",
        format!("scan {scan_id} is not ready"),
      ));
    }
    Ok(record)
  }
}

fn lexical_absolute(path: &Path) -> EngineResult<PathBuf> {
  if !path.is_absolute() {
    return Err(problem(
      "InvalidRequest",
      "local scan paths must be absolute",
    ));
  }
  let mut normalized = PathBuf::new();
  for component in path.components() {
    match component {
      Component::CurDir => {}
      Component::ParentDir => {
        normalized.pop();
      }
      _ => normalized.push(component.as_os_str()),
    }
  }
  Ok(normalized)
}

fn validate_discovery(request: &DiscoveryRequest) -> EngineResult<()> {
  if request.limit == 0 || request.limit > 1000 {
    return Err(problem(
      "InvalidRequest",
      "discovery limit must be between 1 and 1000",
    ));
  }
  Ok(())
}

/// Fail closed if a path or any ancestor was replaced by a link after the
/// scan. Canonical paths must still equal the indexed absolute path and lie
/// under an original scan root. Links themselves are never candidates.
fn safe_metadata(path: &Path, roots: &[PathBuf]) -> Option<fs::Metadata> {
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

fn guard_cleanup_roots(
  path: &Path,
  scan_roots: &[PathBuf],
  configured_roots: &[PathBuf],
) -> EngineResult<()> {
  if scan_roots.iter().any(|root| path == root)
    || !scan_roots.iter().any(|root| path.starts_with(root))
  {
    return Err(problem("Forbidden", "scan roots cannot be cleaned up"));
  }
  if configured_roots
    .iter()
    .any(|root| fs::canonicalize(root).is_ok_and(|root| path == root))
  {
    return Err(problem(
      "Forbidden",
      "configured roots cannot be cleaned up",
    ));
  }
  Ok(())
}

fn fingerprint_of(metadata: &fs::Metadata) -> EntryFingerprint {
  let mtime_ms = metadata
    .modified()
    .ok()
    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
    .map(|duration| duration.as_millis() as f64)
    .unwrap_or(0.0);
  EntryFingerprint {
    size: metadata.len(),
    mtime_ms,
  }
}

/// Snapshots the legacy walk inputs for a ready scan; the caller runs the
/// actual filesystem walk outside the engine lock.
fn discovery_walk_input(record: &ScanRecord) -> DiscoveryInput {
  let mut roots: Vec<PathBuf> = record
    .root_ids
    .iter()
    .filter_map(|id| record.index.get(id))
    .map(|entry| PathBuf::from(&entry.path))
    .collect();
  roots.sort_by_key(|root| root.components().count());
  let mut outer = Vec::<PathBuf>::new();
  for root in roots {
    if !outer.iter().any(|parent| root.starts_with(parent)) {
      outer.push(root);
    }
  }
  DiscoveryInput {
    roots: outer,
    ignore_hidden: record.ignore_hidden,
  }
}

/// Classifies an indexed protected scan report into the three discovery views
/// without touching the filesystem — the desktop twin of the host's
/// `protectedDiscovery` (packages/host/src/protected-discovery.ts). Derived
/// results are sorted by the desktop engine's byte order (size descending,
/// path ascending), unlike the host's localeCompare, to match the legacy
/// discovery sort used elsewhere in this engine.
fn protected_discovery_items(
  index: &HashMap<String, IndexEntry>,
) -> HashMap<DiscoveryKind, Vec<DiscoveryItem>> {
  let by_path: HashMap<&str, &IndexEntry> = index
    .values()
    .map(|entry| (entry.path.as_str(), entry))
    .collect();
  // A marker file counts only when the report measured it as a complete
  // regular file — the report, never the live filesystem, is the source.
  let is_local_file = |path: &str| {
    by_path.get(path).is_some_and(|entry| {
      entry.is_directory == Some(false) && entry.scan_state.as_deref() == Some("complete")
    })
  };
  let in_environment = |path: &str| -> bool {
    let mut cursor = Path::new(path);
    loop {
      let Some(parent) = cursor.parent() else {
        return false;
      };
      if matches!(
        parent.file_name().and_then(|name| name.to_str()),
        Some(".venv" | "venv" | "env")
      ) {
        return true;
      }
      if let Some(config) = parent.join("pyvenv.cfg").to_str() {
        if is_local_file(config) {
          return true;
        }
      }
      cursor = parent;
    }
  };
  let python = [
    ("__pycache__", "Python bytecode"),
    (".pytest_cache", "pytest cache"),
    (".mypy_cache", "mypy cache"),
    (".ruff_cache", "Ruff cache"),
  ];
  let js = [".next", ".nuxt", ".turbo", ".parcel-cache", ".svelte-kit"];
  let mut items: HashMap<DiscoveryKind, Vec<DiscoveryItem>> = [
    DiscoveryKind::LargeFiles,
    DiscoveryKind::Caches,
    DiscoveryKind::Gitignored,
  ]
  .into_iter()
  .map(|kind| (kind, Vec::new()))
  .collect();
  for entry in index.values() {
    // Roots (no parent) and entries the scanner skipped never qualify.
    if entry.scan_state.as_deref() == Some("skipped") || entry.parent.is_none() {
      continue;
    }
    let item = |category: &str| DiscoveryItem {
      node: entry.summary(),
      parent_id: entry.parent.clone(),
      category: category.into(),
      is_directory: entry.is_directory == Some(true),
    };
    if entry.ignored {
      items
        .get_mut(&DiscoveryKind::Gitignored)
        .unwrap()
        .push(item("Gitignored"));
    }
    if entry.is_directory == Some(false) {
      items
        .get_mut(&DiscoveryKind::LargeFiles)
        .unwrap()
        .push(item("Large file"));
      continue;
    }
    let name = Path::new(&entry.path)
      .file_name()
      .and_then(|name| name.to_str());
    let parent = Path::new(&entry.path).parent().map(Path::to_path_buf);
    let marker = |file: &str| {
      parent
        .as_ref()
        .map(|parent| parent.join(file))
        .is_some_and(|joined| joined.to_str().is_some_and(is_local_file))
    };
    let mut category = name.and_then(|name| {
      python
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, label)| *label)
    });
    if name == Some("node_modules") && marker("package.json") {
      category = Some("Node dependencies");
    }
    if name == Some("target") && marker("Cargo.toml") {
      category = Some("Rust build output");
    }
    if name.is_some_and(|name| js.contains(&name)) && marker("package.json") {
      category = Some("JavaScript build cache");
    }
    if let Some(category) = category {
      if !in_environment(&entry.path) {
        items
          .get_mut(&DiscoveryKind::Caches)
          .unwrap()
          .push(item(category));
      }
    }
  }
  // A nested candidate inside an already-listed directory of the same kind
  // is redundant; keep only the outermost rows.
  for kind in [DiscoveryKind::Caches, DiscoveryKind::Gitignored] {
    let directories: HashSet<String> = items[&kind]
      .iter()
      .filter(|item| item.is_directory)
      .map(|item| item.node.path.clone())
      .collect();
    items.get_mut(&kind).unwrap().retain(|item| {
      let mut cursor: &Path = Path::new(&item.node.path);
      loop {
        let Some(parent) = cursor.parent() else {
          return true;
        };
        if parent
          .to_str()
          .is_some_and(|text| directories.contains(text))
        {
          return false;
        }
        cursor = parent;
      }
    });
  }
  for rows in items.values_mut() {
    rows.sort_by(|a, b| {
      b.node
        .size
        .cmp(&a.node.size)
        .then_with(|| a.node.path.cmp(&b.node.path))
    });
  }
  items
}

/// Build once per scan: a fully enumerated tree for files/cache sizes and
/// a summarized gitignore tree for rules, regardless of the browse option.
/// This synchronous function runs on the IPC blocking pool without locks.
pub fn run_discovery(input: DiscoveryInput) -> EngineResult<DiscoveryData> {
  for root in &input.roots {
    if safe_metadata(root, &input.roots).is_none() {
      return Err(problem(
        "StaleSnapshot",
        "a scan root is missing or traverses a symbolic link; scan again",
      ));
    }
  }
  let options = |respect_gitignore| ScanOptions {
    directories: input.roots.clone(),
    ignore_hidden: input.ignore_hidden,
    full_path: true,
    respect_gitignore,
    ignored_mode: IgnoredMode::Summarize,
    follow_symlinks: false,
  };
  let full = scan_directory(options(false));
  let summarized = scan_directory(options(true));
  let mut index = HashMap::new();
  for root in &full {
    walk_index(root, None, 0, &mut index);
  }
  let mut ignored_index = HashMap::new();
  for root in &summarized {
    walk_index(root, None, 0, &mut ignored_index);
  }
  let ignored_paths: HashSet<PathBuf> = ignored_index
    .values()
    .filter(|entry| entry.ignored)
    .map(|entry| PathBuf::from(&entry.path))
    .collect();
  let mut items: HashMap<DiscoveryKind, Vec<DiscoveryItem>> = [
    DiscoveryKind::LargeFiles,
    DiscoveryKind::Caches,
    DiscoveryKind::Gitignored,
  ]
  .into_iter()
  .map(|kind| (kind, Vec::new()))
  .collect();
  let mut safe_index = HashMap::new();
  for (_, mut entry) in index {
    let path = Path::new(&entry.path);
    let Some(metadata) = safe_metadata(path, &input.roots) else {
      continue;
    };
    entry.ignored = path
      .ancestors()
      .any(|ancestor| ignored_paths.contains(ancestor));
    let is_directory = metadata.is_dir();
    let item = |category: &str| DiscoveryItem {
      node: entry.summary(),
      parent_id: entry.parent.clone(),
      category: category.into(),
      is_directory,
    };
    if metadata.is_file() {
      items
        .get_mut(&DiscoveryKind::LargeFiles)
        .unwrap()
        .push(item("Large file"));
    }
    if is_directory {
      if let Some(category) = cache_category(path, &input.roots) {
        items
          .get_mut(&DiscoveryKind::Caches)
          .unwrap()
          .push(item(category));
      }
    }
    if ignored_index
      .get(&entry.id)
      .is_some_and(|ignored| ignored.ignored)
    {
      items
        .get_mut(&DiscoveryKind::Gitignored)
        .unwrap()
        .push(item("Gitignored"));
    }
    safe_index.insert(entry.id.clone(), entry);
  }
  for (kind, rows) in &mut items {
    if *kind != DiscoveryKind::LargeFiles {
      prune_discovery_overlap(rows);
    }
    rows.sort_by(|a, b| {
      b.node
        .size
        .cmp(&a.node.size)
        .then_with(|| a.node.path.cmp(&b.node.path))
    });
  }
  Ok(DiscoveryData {
    index: safe_index,
    items,
  })
}

// The catalog lives in the shared headless crate (the CLI's MCP adapter
// consumes it too); the desktop only keeps its own discovery wiring here.
fn cache_category(path: &Path, roots: &[PathBuf]) -> Option<&'static str> {
  spacelens_discovery::cache_category(path, roots)
}

fn prune_discovery_overlap(rows: &mut Vec<DiscoveryItem>) {
  rows.sort_by(|a, b| {
    Path::new(&a.node.path)
      .components()
      .count()
      .cmp(&Path::new(&b.node.path).components().count())
      .then_with(|| a.node.path.cmp(&b.node.path))
  });
  let mut kept: Vec<DiscoveryItem> = Vec::new();
  for row in rows.drain(..) {
    if !kept
      .iter()
      .any(|parent| Path::new(&row.node.path).starts_with(&parent.node.path))
    {
      kept.push(row);
    }
  }
  *rows = kept;
}

fn walk_index(
  node: &ScanNode,
  parent: Option<String>,
  depth: u32,
  index: &mut HashMap<String, IndexEntry>,
) -> String {
  let path_string = node.path.to_string_lossy().to_string();
  let id = node_id_of(&path_string);
  let child_ids: Vec<String> = node
    .children
    .iter()
    .map(|child| walk_index(child, Some(id.clone()), depth + 1, index))
    .collect();
  index.insert(
    id.clone(),
    IndexEntry {
      id: id.clone(),
      name: Path::new(&path_string)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path_string.clone()),
      path: path_string,
      size: node.size,
      depth,
      ignored: node.ignored,
      collapsed: node.collapsed,
      parent,
      child_ids,
      fingerprint: fs::symlink_metadata(&node.path)
        .ok()
        .map(|metadata| fingerprint_of(&metadata)),
      cleanup: None,
      logical_size: None,
      is_directory: None,
      scan_state: None,
      skip_reason: None,
    },
  );
  id
}

fn walk_local_index(
  node: &LocalScanNode,
  parent: Option<String>,
  depth: u32,
  index: &mut HashMap<String, IndexEntry>,
) -> String {
  let path = node.path.to_string_lossy().into_owned();
  let id = node_id_of(&path);
  let child_ids = node
    .children
    .iter()
    .map(|child| walk_local_index(child, Some(id.clone()), depth + 1, index))
    .collect();
  index.insert(
    id.clone(),
    IndexEntry {
      id: id.clone(),
      name: node
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone()),
      path,
      size: node.size,
      depth,
      ignored: node.ignored,
      collapsed: node.collapsed,
      parent,
      child_ids,
      // Read-only snapshot: never probe filesystem metadata while indexing.
      fingerprint: None,
      cleanup: None,
      logical_size: Some(node.logical_size),
      is_directory: Some(node.is_directory),
      scan_state: Some(node.scan_state.clone()),
      skip_reason: node.skip_reason.clone(),
    },
  );
  id
}

fn count_nodes(node: &ScanNode) -> usize {
  1 + node.children.iter().map(count_nodes).sum::<usize>()
}

struct BuiltSliceNode {
  node: TreeSliceNode,
  truncated: bool,
  omitted_bytes: u64,
  omitted_count: usize,
}

fn build_slice_node(
  index: &HashMap<String, IndexEntry>,
  entry: &IndexEntry,
  remaining_depth: u32,
  max_children: usize,
) -> BuiltSliceNode {
  let mut omitted_bytes = 0u64;
  let mut omitted_count = 0usize;
  let mut truncated = false;

  let mut children_sorted: Vec<&IndexEntry> = entry
    .child_ids
    .iter()
    .filter_map(|id| index.get(id))
    .collect();
  children_sorted.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));

  let mut slice_children = Vec::new();
  if remaining_depth > 0 {
    for (position, child) in children_sorted.into_iter().enumerate() {
      if position >= max_children {
        omitted_bytes += child.size;
        omitted_count += 1;
        truncated = true;
        continue;
      }
      let built = build_slice_node(index, child, remaining_depth - 1, max_children);
      omitted_bytes += built.omitted_bytes;
      omitted_count += built.omitted_count;
      truncated |= built.truncated;
      slice_children.push(built.node);
    }
  } else {
    for child in children_sorted {
      omitted_bytes += child.size;
      omitted_count += 1;
      truncated = true;
    }
  }

  BuiltSliceNode {
    node: TreeSliceNode {
      summary: entry.summary(),
      children: slice_children,
      omitted_bytes,
      omitted_count,
    },
    truncated,
    omitted_bytes,
    omitted_count,
  }
}
