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
use spacelens_storage_usage::StorageBreakdown;

use crate::app_cleanup_policy as policy;

mod index;
use index::{NodeFacts, NodeId, NodeIndex, NONE};

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
  #[serde(skip_serializing_if = "Option::is_none")]
  pub storage: Option<StorageBreakdown>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub app: Option<AppScanOrigin>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScanSession {
  pub scan_id: String,
  pub root_ids: Vec<String>,
  pub created_at: String,
  pub label: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub app: Option<AppScanOrigin>,
}

/// Pinned app-scan origin (§3.2, additive): which app, under which name, in
/// which audit generation a virtual-root session was started.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppScanOrigin {
  pub app_id: String,
  pub app_name: String,
  pub audit_id: String,
}

/// Closed request starting an app scan: an audit generation plus an app id.
/// The locations come from the pinned audit snapshot, never from the caller.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppScanStartRequest {
  pub audit_id: String,
  pub app_id: String,
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
  /// Legacy walks only: the discovery-time index and the nodes that passed
  /// the safety probe, merged into the scan's index at finish.
  index: Box<NodeIndex>,
  kept: HashSet<u32>,
  items: HashMap<DiscoveryKind, DiscoveryRows>,
}

/// Cached discovery results. Protected scans keep only references into the
/// scan's index (a full disk lists millions of files); legacy walks keep the
/// discovery-time summaries, which can differ from the browse index.
enum DiscoveryRows {
  Items(Vec<DiscoveryItem>),
  Nodes(Vec<DiscoveryRow>),
}

#[derive(Clone, Copy)]
struct DiscoveryRow {
  /// The node's size, kept in the row so rows sort without a key buffer.
  size: u64,
  node: u32,
  category: u8,
}

const LARGE_FILE: u8 = 0;
const GITIGNORED: u8 = 1;
const CATEGORIES: [&str; 9] = [
  "Large file",
  "Gitignored",
  "Python bytecode",
  "pytest cache",
  "mypy cache",
  "Ruff cache",
  "Node dependencies",
  "Rust build output",
  "JavaScript build cache",
];

fn category_code(label: &str) -> u8 {
  CATEGORIES
    .iter()
    .position(|candidate| *candidate == label)
    .expect("discovery category is catalogued") as u8
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
struct CleanupSnapshot {
  size: u64,
  ignored: bool,
  fingerprint: Option<EntryFingerprint>,
}

impl CleanupSnapshot {
  fn from_node(index: &NodeIndex, node: u32) -> Self {
    Self {
      size: index.size(node),
      ignored: index.ignored(node),
      fingerprint: index.fingerprint(node),
    }
  }
}

struct ScanRecord {
  session: ScanSession,
  status: ScanStatus,
  index: NodeIndex,
  root_ids: Vec<String>,
  ignore_hidden: bool,
  discovery: HashMap<DiscoveryKind, DiscoveryRows>,
  local_only: bool,
  // Whether the protected scan read .gitignore content while indexing;
  // mirrors the host's gitignoreClassified flag for the discovery gate.
  gitignore_classified: bool,
  // Lease for a prepared/running worker. Cancellation keeps it occupied;
  // only finish_local_scan releases it after the actual walk has returned.
  active_worker: Option<LocalScanCancellation>,
  // App-scan session origin (None for path scans). The pinned audit context
  // (A7): cloned once at prepare time; finish/plan/execute consume only this
  // and never reread current audit state.
  app_origin: Option<AppScanOrigin>,
  app_context: Option<AppScanContext>,
  virtual_root_id: Option<String>,
  bundle_node_id: Option<String>,
  /// Per location-root completeness evidence (A1/R2): canonical location
  /// path string -> whole-folder cleanable. Absent entries count as false.
  app_folder_cleanable: HashMap<String, bool>,
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

/// Immutable pinned context for one app-scan session (A7): the exact audit
/// generation, app identity, canonical location set, bundle leaf, and the
/// cross-app ownership boundary. Cloned ONCE under the audit lock at prepare;
/// finish/plan/execute consume only this.
#[derive(Clone, Debug)]
pub struct AppScanContext {
  pub audit_id: String,
  pub app_id: String,
  pub app_name: String,
  pub generated_at: String,
  /// Pinned exact location paths: directories walk, files become stat leaves.
  pub dir_locations: Vec<PathBuf>,
  pub file_locations: Vec<PathBuf>,
  /// Canonical bundle path plus its audit-time installed size (display only;
  /// never trashed). `None` for CLI entries.
  pub bundle: Option<(PathBuf, u64)>,
  /// Locations owned by OTHER apps in the pinned generation (A5 boundary).
  pub other_app_paths: Vec<PathBuf>,
}

impl AppScanContext {
  /// Every real root this scan is authorized to serve (dirs + files).
  pub fn own_locations(&self) -> Vec<PathBuf> {
    self
      .dir_locations
      .iter()
      .chain(self.file_locations.iter())
      .cloned()
      .collect()
  }
}

/// The whole desktop engine behind one session.
pub struct EngineStore {
  roots: Vec<PathBuf>,
  scans: HashMap<String, ScanRecord>,
  plans: HashMap<String, (CleanupPlan, std::time::Instant)>,
  /// Private recursive manifests for app-scan plans (A4), keyed by plan id
  /// and consumed atomically with the plan.
  app_manifests: HashMap<String, HashMap<String, (u64, i64)>>,
}

impl EngineStore {
  pub fn new(roots: Vec<PathBuf>) -> Self {
    Self {
      roots,
      scans: HashMap::new(),
      plans: HashMap::new(),
      app_manifests: HashMap::new(),
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
            path.file_name().is_none_or(|name| {
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
      // Synthetic app identifiers name no filesystem path (A10): reject
      // before any filesystem access.
      if policy::is_virtual_app_str(path) {
        return Err(problem(
          "InvalidRequest",
          "synthetic app paths cannot be scanned as folders; start an app scan instead",
        ));
      }
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

    let mut index = NodeIndex::default();
    let mut roots = Vec::new();
    let mut bytes = 0u64;
    let mut entries = 0usize;
    for root in &tree {
      roots.push(walk_index(root, NONE, None, &mut index));
      bytes += root.size;
      entries += count_nodes(root);
    }
    index.seal();
    let root_ids: Vec<String> = roots.iter().map(|root| index.wire_id(*root)).collect();

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
      storage: None,
      app: None,
    };
    let session = ScanSession {
      scan_id: scan_id.clone(),
      root_ids: root_ids.clone(),
      created_at,
      label,
      app: None,
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
        app_origin: None,
        app_context: None,
        virtual_root_id: None,
        bundle_node_id: None,
        app_folder_cleanable: HashMap::new(),
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
      // Synthetic app identifiers name no filesystem path (A10): reject
      // before any filesystem access; app scans route through app identity.
      if policy::is_virtual_app_str(path) {
        return Err(problem(
          "InvalidRequest",
          "synthetic app paths cannot be scanned as folders; start an app scan instead",
        ));
      }
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
      app: None,
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
      storage: None,
      app: None,
    };
    self.scans.insert(
      scan_id.clone(),
      ScanRecord {
        session: session.clone(),
        status,
        index: NodeIndex::default(),
        root_ids: Vec::new(),
        ignore_hidden: request.ignore_hidden,
        discovery: HashMap::new(),
        local_only: true,
        gitignore_classified: request.respect_gitignore,
        active_worker: Some(cancellation.clone()),
        app_origin: None,
        app_context: None,
        virtual_root_id: None,
        bundle_node_id: None,
        app_folder_cleanable: HashMap::new(),
      },
    );
    Ok((
      session,
      LocalScanTask {
        scan_id,
        options: ScanOptions {
          directories: resolved,
          ignore_hidden: request.ignore_hidden,
          // Indexing reads `path`, never `name`; a basename name keeps the
          // walker's tree from holding every full path twice.
          full_path: false,
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

  /// Attach APFS accounting to a finished scan. Read off the engine lock
  /// (it spawns `diskutil`), so it lands only if the scan is still ready.
  pub fn attach_storage(
    &mut self,
    scan_id: &str,
    storage: Option<StorageBreakdown>,
  ) -> EngineResult<ScanStatus> {
    let record = self
      .scans
      .get_mut(scan_id)
      .ok_or_else(|| problem("NotFound", "scan vanished"))?;
    if record.status.state == "ready" && storage.is_some() {
      record.status.storage = storage;
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
      Ok(mut report) => {
        // The walker's tree is consumed while indexing, so its memory is
        // released subtree by subtree instead of after the whole index exists.
        let nodes = std::mem::take(&mut report.nodes);
        record.status.bytes_scanned = nodes.iter().map(|node| node.size).sum();
        let mut index = NodeIndex::default();
        let roots = index_local_nodes(&mut index, nodes, NONE);
        index.seal();
        let root_ids: Vec<String> = roots.iter().map(|root| index.wire_id(*root)).collect();
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

  /// Starts an app scan: validates the closed request against the pinned
  /// audit snapshot, clones the snapshot ONCE into an immutable
  /// [`AppScanContext`] stored on the record (A7), and prepares a protected
  /// walk over the app's DIRECTORY locations only. File locations (R1) and
  /// the bundle leaf never enter the walker; `finish_app_scan` injects them
  /// as synthetic leaves. Only a short lock is held; the walk runs outside it.
  pub fn prepare_app_scan(
    &mut self,
    req: AppScanStartRequest,
    snap: &crate::app_cleanup::AuditSnapshot,
    report: &spacelens_app_audit::AppAuditReport,
  ) -> EngineResult<(ScanSession, LocalScanTask)> {
    if req.app_id.is_empty() || req.app_id.len() > 128 {
      return Err(problem(
        "InvalidRequest",
        "appId must be between 1 and 128 characters",
      ));
    }
    if !policy::valid_audit_id(&req.audit_id) {
      return Err(problem("InvalidRequest", "auditId is misshaped"));
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
    // Pinned generation: the request must name the snapshot's own audit id.
    // A re-audit mints a fresh id, so stale requests fail here and live
    // sessions keep browsing their pinned generation (§1Q3).
    if req.audit_id != snap.audit_id {
      return Err(problem(
        "InvalidRequest",
        "audit is stale; re-run the audit",
      ));
    }
    let entry = report
      .apps
      .iter()
      .find(|app| app.id == req.app_id)
      .ok_or_else(|| {
        problem(
          "Forbidden",
          "only audited apps of the current generation can be scanned",
        )
      })?;
    if entry.name.is_empty() || entry.name.len() > 256 {
      return Err(problem("InvalidRequest", "audited app name is misshaped"));
    }
    let reported: Vec<PathBuf> = snap
      .locations_by_app
      .get(&req.app_id)
      .cloned()
      .unwrap_or_default();
    if reported.len() > 200 {
      return Err(problem(
        "InvalidRequest",
        "app reports more locations than can be scanned",
      ));
    }
    // The bundle is an audit-size synthetic leaf, never a walk input; its
    // canonical path comes from the snapshot allow-list (reveal-grade).
    let bundle = snap
      .app_paths
      .get(&req.app_id)
      .map(|path| (path.clone(), entry.installed_size));
    if reported.is_empty() && bundle.is_none() {
      return Err(problem(
        "InvalidRequest",
        "app has no measurable storage in this audit generation",
      ));
    }
    // Every location must sit under the roots this session serves (lexical,
    // like path scans). The bundle is excluded: it may live outside the
    // served roots (`/Applications`) and is never scanned.
    let served: Vec<PathBuf> = self
      .roots
      .iter()
      .filter_map(|root| lexical_absolute(root).ok())
      .collect();
    for location in &reported {
      let absolute = lexical_absolute(location)?;
      if !served.iter().any(|root| absolute.starts_with(root)) {
        return Err(problem(
          "Forbidden",
          "app location is outside the roots this desktop session serves",
        ));
      }
    }
    // Split directory/file roots (R1/A9): files are excluded from the
    // walker's directories and become stat-based leaves at finish. Anything
    // that is not a plain regular file today (missing, link, dir, special)
    // goes to the walker, which classifies it into complete/partial/skipped
    // nodes with coverage evidence.
    let mut dir_locations = Vec::new();
    let mut file_locations = Vec::new();
    for location in reported {
      let is_file = fs::symlink_metadata(&location)
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink());
      if is_file {
        if !file_locations.contains(&location) {
          file_locations.push(location);
        }
      } else if !dir_locations.contains(&location) {
        dir_locations.push(location);
      }
    }
    let other_app_paths: Vec<PathBuf> = snap
      .locations_by_app
      .iter()
      .filter(|(id, _)| id.as_str() != req.app_id)
      .flat_map(|(_, paths)| paths.iter().cloned())
      .chain(
        snap
          .app_paths
          .iter()
          .filter(|(id, _)| id.as_str() != req.app_id)
          .map(|(_, path)| path.clone()),
      )
      .collect();
    let origin = AppScanOrigin {
      app_id: req.app_id.clone(),
      app_name: entry.name.clone(),
      audit_id: req.audit_id.clone(),
    };
    let context = AppScanContext {
      audit_id: req.audit_id.clone(),
      app_id: req.app_id.clone(),
      app_name: entry.name.clone(),
      generated_at: report.generated_at.clone(),
      dir_locations: dir_locations.clone(),
      file_locations: file_locations.clone(),
      bundle: bundle.clone(),
      other_app_paths,
    };
    let scan_id = new_id("scan_");
    let created_at = now_iso();
    let session = ScanSession {
      scan_id: scan_id.clone(),
      root_ids: Vec::new(),
      created_at,
      label: Some(entry.name.clone()),
      app: Some(origin.clone()),
    };
    let cancellation = LocalScanCancellation::default();
    let status = ScanStatus {
      scan_id: scan_id.clone(),
      state: "scanning".into(),
      message: format!(
        "Scanning app storage for {} with cloud downloads blocked",
        entry.name
      ),
      progress: None,
      current_path: None,
      bytes_scanned: 0,
      entries_scanned: 0,
      root_ids: Vec::new(),
      label: Some(entry.name.clone()),
      updated_at: now_iso(),
      coverage: None,
      volumes: None,
      storage: None,
      app: Some(origin),
    };
    self.scans.insert(
      scan_id.clone(),
      ScanRecord {
        session: session.clone(),
        status,
        index: NodeIndex::default(),
        root_ids: Vec::new(),
        ignore_hidden: false,
        discovery: HashMap::new(),
        local_only: true,
        gitignore_classified: true,
        active_worker: Some(cancellation.clone()),
        app_origin: session.app.clone(),
        app_context: Some(context),
        virtual_root_id: None,
        bundle_node_id: None,
        app_folder_cleanable: HashMap::new(),
      },
    );
    Ok((
      session,
      LocalScanTask {
        scan_id,
        options: ScanOptions {
          directories: dir_locations,
          ignore_hidden: false,
          // Indexing reads `path`, never `name`; a basename name keeps the
          // walker's tree from holding every full path twice.
          full_path: false,
          respect_gitignore: true,
          ignored_mode: IgnoredMode::Summarize,
          follow_symlinks: false,
        },
        cancellation,
      },
    ))
  }

  /// Completes an app scan: depth-shifts the walker index under a synthetic
  /// virtual root, injects stat-based file leaves (R1) and the audit-size
  /// bundle leaf, and records per-root completeness evidence (A1/R2) derived
  /// from the walker's own coverage issues. Consumes only the pinned context
  /// stored at prepare (A7): the passed origin/locations/bundle (captured at
  /// prepare time, never reread from current audit state) are cross-checked
  /// against it and never trusted on their own.
  #[allow(clippy::too_many_arguments)]
  pub fn finish_app_scan(
    &mut self,
    scan_id: &str,
    result: std::io::Result<LocalScanReport>,
    origin: AppScanOrigin,
    locations: Vec<PathBuf>,
    bundle: Option<(PathBuf, u64, String)>,
  ) -> EngineResult<ScanStatus> {
    let record = self
      .scans
      .get_mut(scan_id)
      .ok_or_else(|| problem("NotFound", "scan vanished"))?;
    let context = record
      .app_context
      .clone()
      .ok_or_else(|| problem("InvalidRequest", "not an app scan"))?;
    // Cross-check the prepare-time captures against the pinned context; any
    // drift fails closed instead of indexing untrusted inputs.
    let stored_origin = record
      .app_origin
      .clone()
      .ok_or_else(|| problem("InternalError", "app scan lost its origin"))?;
    if stored_origin != origin
      || context.audit_id != origin.audit_id
      || context.app_id != origin.app_id
    {
      return Err(problem("InternalError", "app scan context mismatch"));
    }
    let mut expected: Vec<PathBuf> = context.own_locations();
    expected.sort();
    let mut given = locations;
    given.sort();
    if expected != given {
      return Err(problem("InternalError", "app scan locations mismatch"));
    }
    let expected_bundle = context.bundle.clone();
    let given_bundle = bundle.map(|(path, size, _)| (path, size));
    if expected_bundle != given_bundle {
      return Err(problem("InternalError", "app scan bundle mismatch"));
    }
    if record.status.state != "scanning" {
      return Ok(record.status.clone());
    }
    record.active_worker = None;
    match result {
      Ok(report) => {
        Self::index_app_report(record, &context, report);
        Ok(record.status.clone())
      }
      Err(error) => {
        record.status.state = if error.kind() == std::io::ErrorKind::Interrupted {
          "cancelled"
        } else {
          "failed"
        }
        .into();
        record.status.message = format!("App scan stopped: {error}");
        record.status.current_path = None;
        record.status.updated_at = now_iso();
        Ok(record.status.clone())
      }
    }
  }

  /// Index one finished walker report under the pinned context. Pure
  /// index surgery: virtual root + file/bundle leaves, depth shifts,
  /// evidence, totals.
  fn index_app_report(
    record: &mut ScanRecord,
    context: &AppScanContext,
    mut report: LocalScanReport,
  ) {
    let origin = record.app_origin.clone().unwrap_or(AppScanOrigin {
      app_id: context.app_id.clone(),
      app_name: context.app_name.clone(),
      audit_id: context.audit_id.clone(),
    });
    let (virtual_id, virtual_path) = app_virtual_ids(&context.audit_id, &context.app_id);
    let authorized: HashSet<PathBuf> = context.own_locations().into_iter().collect();

    let mut index = NodeIndex::default();
    // The virtual root goes first so every location can name it as parent;
    // its size is known only once the locations are measured.
    let virtual_root = index.push(NodeFacts {
      id: virtual_id,
      name: &context.app_name,
      path: &virtual_path,
      parent: NONE,
      parent_path: None,
      size: 0,
      logical_size: Some(0),
      is_directory: Some(true),
      ignored: false,
      collapsed: false,
      scan_state: Some("complete"),
      skip_reason: None,
      fingerprint: None,
    });
    let mut children: Vec<u32> = Vec::new();
    let mut evidence: HashMap<String, bool> = HashMap::new();
    let mut locations_size = 0u64;

    // Directory locations: the walker index, depth-shifted to 1.
    for node in std::mem::take(&mut report.nodes) {
      let path_string = node.path.to_string_lossy().into_owned();
      let id = walk_local_index(node, virtual_root, None, &mut index);
      children.push(id);
      let path = PathBuf::from(&path_string);
      let cleanable = if context.dir_locations.contains(&path) {
        policy::location_folder_cleanable(
          &path,
          &report.coverage.issues,
          &authorized,
          report.coverage.issues_truncated,
        )
      } else {
        false
      };
      evidence.insert(path_string, cleanable);
      locations_size = locations_size.saturating_add(index.size(id));
    }
    index.seal();
    let leaf =
      |index: &mut NodeIndex, id: NodeId, path: &Path, size: u64, directory: bool, found: bool| {
        let path_string = path.to_string_lossy().into_owned();
        let name = path
          .file_name()
          .map(|name| name.to_string_lossy().into_owned())
          .unwrap_or_else(|| path_string.clone());
        index.push(NodeFacts {
          id,
          name: &name,
          path: &path_string,
          parent: virtual_root,
          parent_path: None,
          size,
          logical_size: Some(size),
          is_directory: Some(directory),
          ignored: false,
          collapsed: false,
          scan_state: Some(if found { "complete" } else { "skipped" }),
          skip_reason: (!found).then_some("not-found"),
          fingerprint: None,
        })
      };
    // Directory locations the walker never returned (vanished between
    // prepare and walk): skipped placeholders so evidence-driven guards see
    // them as contents-only at best.
    for location in &context.dir_locations {
      let path_string = location.to_string_lossy().into_owned();
      if index.get(NodeId::of_path(&path_string)).is_some() {
        continue;
      }
      children.push(leaf(
        &mut index,
        NodeId::of_path(&path_string),
        location,
        0,
        true,
        false,
      ));
      evidence.insert(path_string, false);
    }

    // File locations (R1/A9): stat-based single-file leaves, never walker
    // output. A stat failure becomes a skipped leaf (refused at plan).
    for location in &context.file_locations {
      let path_string = location.to_string_lossy().into_owned();
      let id = NodeId::of_path(&path_string);
      let node = match fs::symlink_metadata(location) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
          let size = metadata.len();
          locations_size = locations_size.saturating_add(size);
          evidence.insert(path_string, true);
          leaf(&mut index, id, location, size, false, true)
        }
        _ => {
          evidence.insert(path_string, false);
          leaf(&mut index, id, location, 0, false, false)
        }
      };
      children.push(node);
    }

    // Bundle leaf (A3): audit-size, never scanned, never cleanable. When the
    // bundle overlaps a location subtree its bytes are already inside the
    // walker measurement, so no separate leaf is injected (no double count);
    // the guard still refuses every bundle-overlapping target.
    let mut bundle_node_id = None;
    let mut bundle_size = 0u64;
    if let Some((bundle_path, installed_size)) = &context.bundle {
      let overlaps = context
        .own_locations()
        .iter()
        .any(|location| policy::bundle_overlap(location, bundle_path));
      if !overlaps {
        let id = NodeId::app_bundle(&bundle_path.to_string_lossy());
        children.push(leaf(
          &mut index,
          id,
          bundle_path,
          *installed_size,
          true,
          true,
        ));
        bundle_node_id = Some(id.to_wire());
        bundle_size = *installed_size;
      }
    }

    children.sort_by_cached_key(|child| index.wire_id(*child));
    children.dedup_by_key(|child| index.id(*child));
    let virtual_size = locations_size.saturating_add(bundle_size);
    index.set_children(virtual_root, &children);
    index.set_size(virtual_root, virtual_size, Some(virtual_size));
    index.seal();
    let virtual_id = virtual_id.to_wire();

    record.status.bytes_scanned = virtual_size;
    record.status.entries_scanned = index.len();
    record.status.state = "ready".into();
    record.status.message = format!(
      "App scan complete · {} skipped · {} denied",
      report.coverage.skipped_count, report.coverage.denied_count
    );
    record.status.coverage = Some(report.coverage);
    record.status.volumes = Some(report.volumes);
    record.status.app = Some(origin.clone());
    record.session.app = Some(origin);
    record.root_ids = vec![virtual_id.clone()];
    record.session.root_ids = vec![virtual_id.clone()];
    record.status.root_ids = vec![virtual_id.clone()];
    record.virtual_root_id = Some(virtual_id);
    record.bundle_node_id = bundle_node_id;
    record.app_folder_cleanable = evidence;
    record.index = index;
    record.status.current_path = None;
    record.status.updated_at = now_iso();
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
    let index = &record.index;
    let focus = index.find(&request.node_id).ok_or_else(|| {
      problem(
        "NotFound",
        format!("no node {} in scan {}", request.node_id, request.scan_id),
      )
    })?;
    let mut ancestors = Vec::new();
    let mut cursor = index.parent(focus);
    while let Some(parent) = cursor {
      ancestors.insert(0, index.summary(parent));
      cursor = index.parent(parent);
    }
    let built = build_slice_node(index, focus, request.depth, request.max_children_per_node);
    Ok(TreeSlice {
      scan_id: request.scan_id.clone(),
      focus_node: index.summary(focus),
      ancestors,
      tree: built.node,
      total_size: index.size(focus),
      truncated: built.truncated,
      omitted_bytes: built.omitted_bytes,
      omitted_count: built.omitted_count,
      generated_at: now_iso(),
    })
  }

  pub fn children(&self, request: &ChildrenPageRequest) -> EngineResult<ChildrenPage> {
    let record = self.ready(request.scan_id.as_str())?;
    let index = &record.index;
    let node = index.find(&request.node_id).ok_or_else(|| {
      problem(
        "NotFound",
        format!("no node {} in scan {}", request.node_id, request.scan_id),
      )
    })?;
    let mut children: Vec<TreeNodeSummary> = index
      .children(node)
      .map(|child| index.summary(child))
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
      // App scans additionally drop the audit-size bundle leaf: it is display
      // metadata, never a cleanup candidate.
      let mut rows = protected_discovery_rows(&record.index);
      // Rows naming the bundle by its leaf id or by its path.
      let mut bundle_nodes = HashSet::new();
      if let Some(bundle_id) = &record.bundle_node_id {
        bundle_nodes.extend(record.index.find(bundle_id));
      }
      if let Some(bundle) = record
        .app_context
        .as_ref()
        .and_then(|context| context.bundle.clone().map(|(path, _)| path))
      {
        let bundle_string = bundle.to_string_lossy().into_owned();
        bundle_nodes.extend(record.index.get(NodeId::of_path(&bundle_string)));
      }
      if !bundle_nodes.is_empty() {
        for kind_rows in rows.values_mut() {
          kind_rows.retain(|row| !bundle_nodes.contains(&row.node));
        }
      }
      return Ok(PreparedDiscovery::Ready(DiscoveryData {
        index: Box::default(),
        kept: HashSet::new(),
        items: rows
          .into_iter()
          .map(|(kind, rows)| (kind, DiscoveryRows::Nodes(rows)))
          .collect(),
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
      if data.index.len() > 0 {
        let existing = record
          .index
          .merge(&data.index, |node| data.kept.contains(&node));
        // Keep original browse sizes/flags/topology intact; cleanup uses
        // the later discovery snapshot shown in discovery results.
        for (node, source) in existing {
          record
            .index
            .cleanup
            .insert(node, CleanupSnapshot::from_node(&data.index, source));
        }
      }
      for (kind, items) in data.items {
        record.discovery.entry(kind).or_insert(items);
      }
    }
    let rows = record
      .discovery
      .get(&request.kind)
      .ok_or_else(|| problem("InternalError", "discovery results were not computed"))?;
    let mut total = 0usize;
    let mut total_size = 0u64;
    let mut items = Vec::new();
    let mut take = |size: u64, item: &dyn Fn() -> DiscoveryItem| {
      if size < request.min_size {
        return;
      }
      if total >= request.offset && items.len() < request.limit {
        items.push(item());
      }
      total += 1;
      total_size += size;
    };
    match rows {
      DiscoveryRows::Items(rows) => {
        for row in rows {
          take(row.node.size, &|| row.clone());
        }
      }
      DiscoveryRows::Nodes(rows) => {
        let index = &record.index;
        for row in rows {
          take(row.size, &|| DiscoveryItem {
            node: index.summary(row.node),
            parent_id: index.parent(row.node).map(|parent| index.wire_id(parent)),
            category: CATEGORIES[row.category as usize].into(),
            is_directory: index.is_directory(row.node) == Some(true),
          });
        }
      }
    }
    Ok(DiscoveryPage {
      scan_id: request.scan_id.clone(),
      kind: request.kind,
      total,
      total_size,
      items,
      offset: request.offset,
      limit: request.limit,
    })
  }

  pub fn plan(&mut self, request: &CleanupPlanRequest) -> EngineResult<CleanupPlan> {
    // App scans branch to the pinned-context route; path scans keep the
    // verbatim flow below (no loosening).
    let is_app_scan = self
      .scans
      .get(request.scan_id.as_str())
      .is_some_and(|record| record.app_context.is_some());
    if is_app_scan {
      return self.plan_app_scan(request);
    }
    let record = self.ready(request.scan_id.as_str())?;
    let scan_roots = scan_root_paths(record);
    let mut selected = Vec::new();
    for node_id in &request.node_ids {
      let node = record.index.find(node_id).ok_or_else(|| {
        problem(
          "InvalidRequest",
          format!("plan references unknown node {node_id}"),
        )
      })?;
      let path = record.index.path(node);
      // A skipped or partial entry was never fully measured, no matter what
      // the path looks like now — mirror the UI selection rules and reject
      // before any metadata probe (which could touch a replaced path).
      if matches!(record.index.scan_state(node), Some("skipped" | "partial")) {
        return Err(problem(
          "InvalidRequest",
          format!("path was skipped or partially scanned; cleanup needs a complete entry: {path}"),
        ));
      }
      guard_cleanup_roots(Path::new(&path), &scan_roots, &self.roots)?;
      let snapshot = record
        .index
        .cleanup
        .get(&node)
        .cloned()
        .unwrap_or_else(|| CleanupSnapshot::from_node(&record.index, node));
      let metadata = safe_metadata(Path::new(&path), &scan_roots).ok_or_else(|| {
        problem(
          "StalePlan",
          format!("path is missing, outside the scan roots, or traverses a symbolic link: {path}"),
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
            format!("path changed since scanning: {path}; scan again"),
          ));
        }
        Some(fingerprint) => fingerprint,
        None => current,
      };
      selected.push((path, snapshot, baseline));
    }
    selected.sort_by(|a, b| {
      Path::new(&a.0)
        .components()
        .count()
        .cmp(&Path::new(&b.0).components().count())
        .then_with(|| a.0.cmp(&b.0))
    });
    let mut entries: Vec<PlanEntry> = Vec::new();
    for (path, snapshot, fingerprint) in selected {
      if entries
        .iter()
        .any(|planned| Path::new(&path).starts_with(&planned.path))
      {
        continue;
      }
      entries.push(PlanEntry {
        path,
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

  /// App-scan planning: authorizes every staged node against the pinned
  /// context (A5/A7), enforces per-root completeness evidence for folder-level
  /// staging (A1), measures the pruned union's physical sizes (A2), and
  /// captures a bounded recursive manifest per retained target for the
  /// execute-time verification (A4).
  fn plan_app_scan(&mut self, request: &CleanupPlanRequest) -> EngineResult<CleanupPlan> {
    let record = self.ready(request.scan_id.as_str())?;
    let context = record
      .app_context
      .clone()
      .ok_or_else(|| problem("InternalError", "app scan lost its context"))?;
    let own = context.own_locations();
    let mut selected: Vec<(PlannedNode, EntryFingerprint)> = Vec::new();
    for node_id in &request.node_ids {
      let node = record.index.find(node_id).ok_or_else(|| {
        problem(
          "InvalidRequest",
          format!("plan references unknown node {node_id}"),
        )
      })?;
      let entry = PlannedNode {
        path: record.index.path(node),
        size: record.index.size(node),
        ignored: record.index.ignored(node),
      };
      let scan_state = record.index.scan_state(node);
      let path = PathBuf::from(&entry.path);
      // Shared-policy authorization (A3/A5), synthetic identifiers first.
      guard_app_cleanup(&path, record, &self.roots)?;
      // Scan-state rule with the A1 evidence exception: a location root is
      // whole-folder stageable only when its coverage evidence is complete
      // (nested `separate-root` partitions accepted); `skipped` is refused
      // everywhere and `partial` below depth 1 stays refused.
      let is_location_root = own.iter().any(|location| location == &path);
      if matches!(scan_state, Some("skipped")) {
        return Err(problem(
          "InvalidRequest",
          format!(
            "path was skipped during the scan; cleanup needs a complete entry: {}",
            entry.path
          ),
        ));
      }
      if is_location_root {
        let cleanable = record
          .app_folder_cleanable
          .get(&entry.path)
          .copied()
          .unwrap_or(false);
        if !cleanable {
          return Err(problem(
            "InvalidRequest",
            format!(
              "location was partially scanned; only its fully measured contents can be staged: {}",
              entry.path
            ),
          ));
        }
      } else if matches!(scan_state, Some("partial")) {
        return Err(problem(
          "InvalidRequest",
          format!(
            "path was skipped or partially scanned; cleanup needs a complete entry: {}",
            entry.path
          ),
        ));
      }
      // Fail-closed probe (shared strict gate): symlink/escape drift, Group
      // Containers, and missing paths all refuse before measuring.
      let metadata = policy::strict_target_metadata(&path).ok_or_else(|| {
        problem(
          "StalePlan",
          format!(
            "path is missing, outside the scan roots, or traverses a symbolic link: {}",
            entry.path
          ),
        )
      })?;
      let current = fingerprint_of(&metadata);
      // Protected index entries carry no scan-time fingerprint, so the fresh
      // measurement becomes the plan baseline and execute re-verifies it.
      let baseline = match record.index.fingerprint(node) {
        Some(fingerprint) if fingerprint != current => {
          return Err(problem(
            "StaleSnapshot",
            format!("path changed since scanning: {}; scan again", entry.path),
          ));
        }
        Some(fingerprint) => fingerprint,
        None => current,
      };
      selected.push((entry, baseline));
    }
    // Prune staged targets to top-most ancestors (mirror the path-scan drop).
    selected.sort_by(|a, b| {
      Path::new(&a.0.path)
        .components()
        .count()
        .cmp(&Path::new(&b.0.path).components().count())
        .then_with(|| a.0.path.cmp(&b.0.path))
    });
    let mut retained: Vec<(PlannedNode, EntryFingerprint)> = Vec::new();
    for candidate in selected {
      if retained
        .iter()
        .any(|planned| Path::new(&candidate.0.path).starts_with(&planned.0.path))
      {
        continue;
      }
      retained.push(candidate);
    }
    // Physical union (A2): freshly measured file bytes with shared inodes
    // counted once — never index partitions. Unreadable targets fail closed.
    let retained_paths: Vec<PathBuf> = retained
      .iter()
      .map(|(entry, _)| PathBuf::from(&entry.path))
      .collect();
    let sizes = policy::measure_physical_union(&retained_paths).map_err(|error| match error {
      policy::ManifestError::TooLarge => {
        problem("InvalidRequest", "location too large to verify safely")
      }
      policy::ManifestError::Unverifiable => problem(
        "StalePlan",
        "location cannot be measured safely; scan again",
      ),
    })?;
    // Bounded recursive manifest per retained target (A4), verified again
    // before the first delete at execute time.
    let mut manifest: HashMap<String, (u64, i64)> = HashMap::new();
    for path in &retained_paths {
      match policy::collect_manifest(path, &mut manifest) {
        Ok(()) => {}
        Err(policy::ManifestError::TooLarge) => {
          return Err(problem(
            "InvalidRequest",
            "location too large to verify safely",
          ));
        }
        Err(policy::ManifestError::Unverifiable) => {
          return Err(problem(
            "StalePlan",
            "locations changed since the scan; scan again",
          ));
        }
      }
    }
    let entries: Vec<PlanEntry> = retained
      .iter()
      .map(|(entry, fingerprint)| {
        let size = sizes
          .get(&PathBuf::from(&entry.path))
          .copied()
          .unwrap_or(entry.size);
        PlanEntry {
          path: entry.path.clone(),
          size,
          reason: if entry.ignored { "ignored" } else { "staged" }.into(),
          preset: None,
          ignored: entry.ignored,
          fingerprint: fingerprint.clone(),
        }
      })
      .collect();
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
    self.app_manifests.insert(plan.plan_id.clone(), manifest);
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
    // App-scan plans carry a private manifest and branch to the pinned-context
    // route; path plans keep the verbatim flow below.
    let is_app_scan = self
      .scans
      .get(plan.scan_id.as_str())
      .is_some_and(|record| record.app_context.is_some());
    if is_app_scan {
      let manifest = self.app_manifests.remove(&request.plan_id);
      return self.execute_app_scan(plan, manifest);
    }
    let mut changed = 0usize;
    let record = self.ready(&plan.scan_id)?;
    let scan_roots = scan_root_paths(record);
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

  /// App-scan execute (A4): consumes the plan atomically, re-verifies
  /// authorization + fingerprints + the private recursive manifest BEFORE the
  /// first delete, then trashes with an immediate per-target recheck
  /// (authorization + path walk + fingerprint) just before each delete. Drift
  /// at the per-target recheck marks that entry failed and continues.
  /// Outcome bytes are the plan's physical sizes (A2). A successful cleanup
  /// marks the session so the UI can offer a rescan (A11).
  fn execute_app_scan(
    &mut self,
    plan: CleanupPlan,
    manifest: Option<HashMap<String, (u64, i64)>>,
  ) -> EngineResult<CleanupOutcome> {
    let record = self.ready(&plan.scan_id)?;
    let context = record
      .app_context
      .clone()
      .ok_or_else(|| problem("InternalError", "app scan lost its context"))?;
    let stored = manifest.ok_or_else(|| problem("InternalError", "app plan lost its manifest"))?;
    let own = context.own_locations();
    let bundle = context.bundle.clone().map(|(path, _)| path);
    let bundle_ref = bundle.as_deref();
    // Verify-all-before-first-delete: authorization, then fingerprints.
    let mut changed = 0usize;
    for entry in &plan.entries {
      let path = Path::new(&entry.path);
      policy::authorize_target(
        path,
        &own,
        &context.other_app_paths,
        bundle_ref,
        &self.roots,
      )
      .map_err(|error| problem(error.code, error.message))?;
      let matches = policy::strict_target_metadata(path)
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
    // Manifest re-verification before the FIRST trash call: a changed
    // grandchild can leave top-level fingerprints untouched.
    let mut current: HashMap<String, (u64, i64)> = HashMap::new();
    for entry in &plan.entries {
      if policy::collect_manifest(Path::new(&entry.path), &mut current).is_err() {
        return Err(problem(
          "StalePlan",
          "locations changed since the plan was made; plan again",
        ));
      }
    }
    if current != stored {
      return Err(problem(
        "StalePlan",
        "locations changed since the plan was made; plan again",
      ));
    }
    let mut outcome = CleanupOutcome {
      trashed: Vec::new(),
      bytes_freed: 0,
      failed: Vec::new(),
    };
    for entry in &plan.entries {
      let path = Path::new(&entry.path);
      // Immediate per-target recheck just before this target's delete.
      let authorized = policy::authorize_target(
        path,
        &own,
        &context.other_app_paths,
        bundle_ref,
        &self.roots,
      )
      .is_ok();
      let fresh = authorized
        && policy::strict_target_metadata(path)
          .map(|metadata| fingerprint_of(&metadata) == entry.fingerprint)
          .unwrap_or(false);
      if !fresh {
        outcome.failed.push(FailedEntry {
          path: entry.path.clone(),
          code: "StalePlan".into(),
          message: format!("path changed before deletion: {}", entry.path),
        });
        continue;
      }
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
    if !outcome.trashed.is_empty() {
      if let Some(record) = self.scans.get_mut(&plan.scan_id) {
        record.status.message = format!(
          "{} · cleanup done; rescan recommended",
          record.status.message
        );
        record.status.updated_at = now_iso();
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

/// App-scan cleanup guard (amendments A3/A5): the virtual root and the bundle
/// are refused, location roots ARE cleanable (inversion of the scan-root
/// rule), and descendants must sit under one pinned location. Bundle
/// protection is containment-both-ways; cross-app ownership overlap and the
/// configured-root refusal come from the shared policy module. Everything is
/// derived from the record's pinned context — never current audit state.
fn guard_app_cleanup(
  path: &Path,
  record: &ScanRecord,
  configured_roots: &[PathBuf],
) -> EngineResult<()> {
  let context = record
    .app_context
    .as_ref()
    .ok_or_else(|| problem("InvalidRequest", "not an app scan"))?;
  // Synthetic identifiers never reach the shared policy's path logic without
  // an explicit early refusal here (A10).
  if is_virtual_app_path(path) {
    return Err(problem(
      "Forbidden",
      "the app virtual root cannot be cleaned up",
    ));
  }
  let own = context.own_locations();
  let bundle = context.bundle.as_ref().map(|(path, _)| path.as_path());
  policy::authorize_target(
    path,
    &own,
    &context.other_app_paths,
    bundle,
    configured_roots,
  )
  .map_err(|error| problem(error.code, error.message))
}

/// True for synthetic `app-storage:` identifiers, which never name a
/// filesystem path and must be rejected before any filesystem access.
fn is_virtual_app_path(path: &Path) -> bool {
  policy::is_virtual_app_path(path)
}

/// Pinned virtual conventions (§3.2): `approot_` + sha1 hex[..24] id and the
/// matching `app-storage:` path for one audit generation + app id. The
/// bundle leaf is `appbundle_` + sha1(bundlePath)[..24] (`NodeId::app_bundle`).
fn app_virtual_ids(audit_id: &str, app_id: &str) -> (NodeId, String) {
  let id = NodeId::app_root(&format!("app-scan:{audit_id}:{app_id}"));
  let path = format!("app-storage:{}", id.hex());
  (id, path)
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

/// What an app-scan plan keeps of each staged node.
struct PlannedNode {
  path: String,
  size: u64,
  ignored: bool,
}

/// The scan's root paths, in root order.
fn scan_root_paths(record: &ScanRecord) -> Vec<PathBuf> {
  record
    .root_ids
    .iter()
    .filter_map(|id| record.index.find(id))
    .map(|root| PathBuf::from(record.index.path(root)))
    .collect()
}

/// Snapshots the legacy walk inputs for a ready scan; the caller runs the
/// actual filesystem walk outside the engine lock.
fn discovery_walk_input(record: &ScanRecord) -> DiscoveryInput {
  let mut roots = scan_root_paths(record);
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
/// discovery sort used elsewhere in this engine. Rows reference index nodes;
/// paths are built only for cache candidates and for size ties.
fn protected_discovery_rows(index: &NodeIndex) -> HashMap<DiscoveryKind, Vec<DiscoveryRow>> {
  // A marker file counts only when the report measured it as a complete
  // regular file — the report, never the live filesystem, is the source.
  let is_local_file = |path: &str| {
    index.get(NodeId::of_path(path)).is_some_and(|node| {
      index.is_directory(node) == Some(false) && index.scan_state(node) == Some("complete")
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
  // Roots (no parent) and entries the scanner skipped never qualify.
  let listed =
    |node: u32| index.scan_state(node) != Some("skipped") && index.parent(node).is_some();
  // Sized up front: a full disk lists millions of files, and every regrown
  // buffer would stay resident in the allocator's large-block cache.
  let files = index
    .iter()
    .filter(|node| listed(*node) && index.is_directory(*node) == Some(false))
    .count();
  let mut large_files = Vec::with_capacity(files);
  let mut caches = Vec::new();
  let mut gitignored = Vec::new();
  let row = |node: u32, category: u8| DiscoveryRow {
    size: index.size(node),
    node,
    category,
  };
  for node in index.iter() {
    if !listed(node) {
      continue;
    }
    if index.ignored(node) {
      gitignored.push(row(node, GITIGNORED));
    }
    if index.is_directory(node) == Some(false) {
      large_files.push(row(node, LARGE_FILE));
      continue;
    }
    // A derived node's name is its path's file name; only nodes holding
    // their path verbatim need the path to say.
    let explicit_path = index.has_explicit_path(node).then(|| index.path(node));
    let name = match &explicit_path {
      Some(path) => Path::new(path).file_name().and_then(|name| name.to_str()),
      None => Some(index.name(node)),
    };
    let is_candidate = name.is_some_and(|name| {
      python.iter().any(|(candidate, _)| *candidate == name)
        || name == "node_modules"
        || name == "target"
        || js.contains(&name)
    });
    if !is_candidate {
      continue;
    }
    let path = explicit_path.clone().unwrap_or_else(|| index.path(node));
    let parent = Path::new(&path).parent().map(Path::to_path_buf);
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
      if !in_environment(&path) {
        caches.push(row(node, category_code(category)));
      }
    }
  }
  // A nested candidate inside an already-listed directory of the same kind
  // is redundant; keep only the outermost rows.
  for rows in [&mut caches, &mut gitignored] {
    let directories: HashSet<u32> = rows
      .iter()
      .filter(|row| index.is_directory(row.node) == Some(true))
      .map(|row| row.node)
      .collect();
    let mut beyond_chain: HashMap<u32, bool> = HashMap::new();
    rows.retain(|row| !inside_listed_directory(index, row.node, &directories, &mut beyond_chain));
  }
  for rows in [&mut large_files, &mut caches, &mut gitignored] {
    sort_rows_by_size_then_path(index, rows);
  }
  [
    (DiscoveryKind::LargeFiles, large_files),
    (DiscoveryKind::Caches, caches),
    (DiscoveryKind::Gitignored, gitignored),
  ]
  .into_iter()
  .collect()
}

/// Whether any proper path ancestor of `node` is one of `directories`.
/// Inside a scan the ancestors are the parent chain; above the first node
/// holding its path verbatim (a root or app location) they are looked up by
/// path, memoized per such node in `beyond_chain`.
fn inside_listed_directory(
  index: &NodeIndex,
  node: u32,
  directories: &HashSet<u32>,
  beyond_chain: &mut HashMap<u32, bool>,
) -> bool {
  let mut cursor = node;
  while !index.has_explicit_path(cursor) {
    let Some(parent) = index.parent(cursor) else {
      break;
    };
    if directories.contains(&parent) {
      return true;
    }
    cursor = parent;
  }
  *beyond_chain.entry(cursor).or_insert_with(|| {
    let path = index.path(cursor);
    Path::new(&path).ancestors().skip(1).any(|ancestor| {
      ancestor
        .to_str()
        .and_then(|ancestor| index.get(NodeId::of_path(ancestor)))
        .is_some_and(|ancestor| directories.contains(&ancestor))
    })
  })
}

/// Size descending, then path ascending (byte order). Paths are built only
/// for runs of equal size, into scratch memory mapped for that run.
fn sort_rows_by_size_then_path(index: &NodeIndex, rows: &mut [DiscoveryRow]) {
  rows.sort_unstable_by_key(|row| std::cmp::Reverse(row.size));
  let mut path = String::new();
  let mut start = 0;
  while start < rows.len() {
    let size = rows[start].size;
    let end = start
      + rows[start..]
        .iter()
        .take_while(|row| row.size == size)
        .count();
    if end - start > 1 {
      let run = &mut rows[start..end];
      let total = run.iter().map(|row| index.path_len(row.node)).sum();
      let mut text = ScratchBytes::new(total);
      let bytes = text.bytes();
      let mut offset = 0;
      for row in run.iter_mut() {
        path.clear();
        index.push_path(row.node, &mut path);
        bytes[offset..offset + path.len()].copy_from_slice(path.as_bytes());
        // Every row in the run has the same size, so the field holds the
        // path's place while sorting: offset in the high 40 bits, length in
        // the low 24 (paths are far shorter than 16 MiB).
        row.size = ((offset as u64) << 24) | path.len() as u64;
        offset += path.len();
      }
      let bytes = &*bytes;
      let text_of = |packed: u64| {
        let from = (packed >> 24) as usize;
        &bytes[from..from + (packed & 0xff_ffff) as usize]
      };
      run.sort_unstable_by(|a, b| text_of(a.size).cmp(text_of(b.size)));
      for row in run.iter_mut() {
        row.size = size;
      }
    }
    start = end;
  }
}

/// Zeroed scratch bytes for one sort. On unix they are mapped straight from
/// the OS and unmapped on drop: a freed allocation this large stays resident
/// in the macOS allocator's large-block cache, which Activity Monitor keeps
/// counting long after discovery finished.
struct ScratchBytes {
  #[cfg(unix)]
  mapped: Option<(std::ptr::NonNull<u8>, usize)>,
  fallback: Vec<u8>,
}

impl ScratchBytes {
  fn new(len: usize) -> Self {
    #[cfg(unix)]
    {
      if len > 0 {
        // SAFETY: a fresh private anonymous mapping; no existing memory is
        // touched, and the result is checked before use.
        let pointer = unsafe {
          libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
          )
        };
        if pointer != libc::MAP_FAILED {
          if let Some(pointer) = std::ptr::NonNull::new(pointer.cast::<u8>()) {
            return Self {
              mapped: Some((pointer, len)),
              fallback: Vec::new(),
            };
          }
        }
      }
      Self {
        mapped: None,
        fallback: vec![0; len],
      }
    }
    #[cfg(not(unix))]
    Self {
      fallback: vec![0; len],
    }
  }

  fn bytes(&mut self) -> &mut [u8] {
    #[cfg(unix)]
    if let Some((pointer, len)) = self.mapped {
      // SAFETY: the mapping is `len` readable, writable, zero-filled bytes
      // owned by `self` until drop, and the borrow is tied to `&mut self`.
      return unsafe { std::slice::from_raw_parts_mut(pointer.as_ptr(), len) };
    }
    &mut self.fallback
  }
}

#[cfg(unix)]
impl Drop for ScratchBytes {
  fn drop(&mut self) {
    if let Some((pointer, len)) = self.mapped.take() {
      // SAFETY: unmaps exactly the mapping `new` created; no slice of it
      // outlives `self`.
      unsafe {
        libc::munmap(pointer.as_ptr().cast(), len);
      }
    }
  }
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
  let mut index = NodeIndex::default();
  for root in &full {
    walk_index(root, NONE, None, &mut index);
  }
  index.seal();
  let mut ignored_index = NodeIndex::default();
  for root in &summarized {
    walk_index(root, NONE, None, &mut ignored_index);
  }
  ignored_index.seal();
  let ignored_paths: HashSet<PathBuf> = ignored_index
    .iter()
    .filter(|node| ignored_index.ignored(*node))
    .map(|node| PathBuf::from(ignored_index.path(node)))
    .collect();
  let mut items: HashMap<DiscoveryKind, Vec<DiscoveryItem>> = [
    DiscoveryKind::LargeFiles,
    DiscoveryKind::Caches,
    DiscoveryKind::Gitignored,
  ]
  .into_iter()
  .map(|kind| (kind, Vec::new()))
  .collect();
  let mut kept = HashSet::new();
  let nodes: Vec<u32> = index.iter().collect();
  for node in nodes {
    let path_string = index.path(node);
    let path = Path::new(&path_string);
    let Some(metadata) = safe_metadata(path, &input.roots) else {
      continue;
    };
    index.set_ignored(
      node,
      path
        .ancestors()
        .any(|ancestor| ignored_paths.contains(ancestor)),
    );
    let is_directory = metadata.is_dir();
    let item = |category: &str| DiscoveryItem {
      node: index.summary(node),
      parent_id: index.parent(node).map(|parent| index.wire_id(parent)),
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
      .get(index.id(node))
      .is_some_and(|ignored| ignored_index.ignored(ignored))
    {
      items
        .get_mut(&DiscoveryKind::Gitignored)
        .unwrap()
        .push(item("Gitignored"));
    }
    kept.insert(node);
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
    index: Box::new(index),
    kept,
    items: items
      .into_iter()
      .map(|(kind, rows)| (kind, DiscoveryRows::Items(rows)))
      .collect(),
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
  parent: u32,
  parent_path: Option<&str>,
  index: &mut NodeIndex,
) -> u32 {
  let path = node.path.to_string_lossy();
  let name = Path::new(&*path)
    .file_name()
    .map(|name| name.to_string_lossy())
    .unwrap_or_else(|| path.clone());
  let id = index.push(NodeFacts {
    id: NodeId::of_path(&path),
    name: &name,
    path: &path,
    parent,
    parent_path,
    size: node.size,
    logical_size: None,
    is_directory: None,
    ignored: node.ignored,
    collapsed: node.collapsed,
    scan_state: None,
    skip_reason: None,
    fingerprint: fs::symlink_metadata(&node.path)
      .ok()
      .map(|metadata| fingerprint_of(&metadata)),
  });
  let children: Vec<u32> = node
    .children
    .iter()
    .map(|child| walk_index(child, id, Some(&path), index))
    .collect();
  index.set_children(id, &children);
  id
}

/// Indexes a protected report's root nodes under `parent`, consuming the
/// walker's tree as it goes. The caller seals the index.
fn index_local_nodes(index: &mut NodeIndex, nodes: Vec<LocalScanNode>, parent: u32) -> Vec<u32> {
  fn measure(node: &LocalScanNode, totals: &mut (usize, usize)) {
    totals.0 += 1;
    totals.1 += node.path.file_name().map_or(0, |name| name.len()) + 2;
    for child in &node.children {
      measure(child, totals);
    }
  }
  let mut totals = (0, 0);
  for node in &nodes {
    measure(node, &mut totals);
  }
  index.reserve(totals.0, totals.1);
  nodes
    .into_iter()
    .map(|node| walk_local_index(node, parent, None, index))
    .collect()
}

fn walk_local_index(
  mut node: LocalScanNode,
  parent: u32,
  parent_path: Option<&str>,
  index: &mut NodeIndex,
) -> u32 {
  let children = std::mem::take(&mut node.children);
  let path = node.path.to_string_lossy();
  let name = node
    .path
    .file_name()
    .map(|name| name.to_string_lossy())
    .unwrap_or_else(|| path.clone());
  let id = index.push(NodeFacts {
    id: NodeId::of_path(&path),
    name: &name,
    path: &path,
    parent,
    parent_path,
    size: node.size,
    logical_size: Some(node.logical_size),
    is_directory: Some(node.is_directory),
    ignored: node.ignored,
    collapsed: node.collapsed,
    // Read-only snapshot: never probe filesystem metadata while indexing.
    scan_state: Some(&node.scan_state),
    skip_reason: node.skip_reason.as_deref(),
    fingerprint: None,
  });
  // Each child subtree is dropped as soon as it is indexed.
  let children: Vec<u32> = children
    .into_iter()
    .map(|child| walk_local_index(child, id, Some(&path), index))
    .collect();
  index.set_children(id, &children);
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
  index: &NodeIndex,
  node: u32,
  remaining_depth: u32,
  max_children: usize,
) -> BuiltSliceNode {
  let mut omitted_bytes = 0u64;
  let mut omitted_count = 0usize;
  let mut truncated = false;

  let mut children_sorted: Vec<u32> = index.children(node).collect();
  children_sorted.sort_by(|a, b| {
    index
      .size(*b)
      .cmp(&index.size(*a))
      .then_with(|| index.name(*a).cmp(index.name(*b)))
  });

  let mut slice_children = Vec::new();
  if remaining_depth > 0 {
    for (position, child) in children_sorted.into_iter().enumerate() {
      if position >= max_children {
        omitted_bytes += index.size(child);
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
      omitted_bytes += index.size(child);
      omitted_count += 1;
      truncated = true;
    }
  }

  BuiltSliceNode {
    node: TreeSliceNode {
      summary: index.summary(node),
      children: slice_children,
      omitted_bytes,
      omitted_count,
    },
    truncated,
    omitted_bytes,
    omitted_count,
  }
}
