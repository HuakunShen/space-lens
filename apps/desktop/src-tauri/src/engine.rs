//! Desktop engine store: the Rust twin of `packages/host` scan sessions.
//! Same contract shapes (camelCase JSON), same node-id scheme (sha1(path)[..24]),
//! same safety rules: trash-only cleanup, fingerprint re-verification, and
//! `truncated`/`omitted` accounting computed — never guessed.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

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

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanupPlanRequest {
    pub scan_id: String,
    pub node_ids: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
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
    pub source: String,
    pub removable: bool,
}

fn default_true() -> bool {
    true
}

fn now_iso() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
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
    digest.iter().take(12).map(|byte| format!("{byte:02x}")).collect()
}

fn new_id(prefix: &str) -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().subsec_nanos();
    let mut hasher = Sha1::new();
    hasher.update(nanos.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    let digest = hasher.finalize();
    format!("{prefix}{}", digest.iter().take(6).map(|byte| format!("{byte:02x}")).collect::<String>())
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
        }
    }
}

struct ScanRecord {
    session: ScanSession,
    status: ScanStatus,
    index: HashMap<String, IndexEntry>,
    root_ids: Vec<String>,
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
    EngineProblem { code: code.to_string(), message: message.into() }
}

/// The whole desktop engine behind one session.
pub struct EngineStore {
    roots: Vec<PathBuf>,
    scans: HashMap<String, ScanRecord>,
    plans: HashMap<String, (CleanupPlan, std::time::Instant)>,
    allow_cleanup: bool,
}

impl EngineStore {
    pub fn new(roots: Vec<PathBuf>, allow_cleanup: bool) -> Self {
        Self { roots, scans: HashMap::new(), plans: HashMap::new(), allow_cleanup }
    }

    pub fn roots(&self) -> Vec<ScanTarget> {
        self.roots
            .iter()
            .enumerate()
            .map(|(index, root)| ScanTarget {
                id: format!("root_{index}"),
                label: root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| root.to_string_lossy().to_string()),
                path: root.to_string_lossy().to_string(),
                kind: "folder".into(),
                description: String::new(),
                size: 0,
                source: "preset".into(),
                removable: false,
            })
            .collect()
    }

    /// Runs the synchronous engine scan. Call from `spawn_blocking`.
    pub fn start_scan(&mut self, request: ScanStartRequest) -> EngineResult<ScanSession> {
        let mut resolved = Vec::with_capacity(request.paths.len());
        for path in &request.paths {
            let canonical = fs::canonicalize(path).map_err(|_| problem("InvalidRequest", format!("path does not exist: {path}")))?;
            let allowed = self.roots.iter().any(|root| {
                fs::canonicalize(root).map(|root| canonical == root || canonical.starts_with(&root)).unwrap_or(false)
            });
            if !allowed {
                return Err(problem(
                    "Forbidden",
                    format!("path is outside the roots this desktop session serves: {}", canonical.display()),
                ));
            }
            resolved.push(canonical);
        }
        let scan_id = new_id("scan_");
        let created_at = now_iso();
        let label = request.label.clone().or_else(|| resolved.first().map(|p| p.to_string_lossy().to_string()));

        let options = ScanOptions {
            directories: resolved,
            ignore_hidden: request.ignore_hidden,
            full_path: true,
            respect_gitignore: request.respect_gitignore,
            ignored_mode: match request.ignored_mode {
                WireIgnoredMode::Summarize => IgnoredMode::Summarize,
                WireIgnoredMode::Exclude => IgnoredMode::Exclude,
            },
        };
        let tree = scan_directory(options);
        if tree.is_empty() {
            return Err(problem("InvalidRequest", "the engine produced no tree for the requested paths"));
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
            index.entry(root_id.clone()).and_modify(|entry| entry.depth = 0);
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
        };
        let session = ScanSession { scan_id: scan_id.clone(), root_ids: root_ids.clone(), created_at, label };
        self.scans.insert(scan_id, ScanRecord { session: session.clone(), status, index, root_ids });
        Ok(session)
    }

    pub fn status(&mut self, scan_id: &str) -> EngineResult<ScanStatus> {
        Ok(self.record(scan_id)?.status.clone())
    }

    pub fn list(&mut self) -> Vec<ScanStatus> {
        let mut statuses: Vec<ScanStatus> = self.scans.values().map(|record| record.status.clone()).collect();
        statuses.sort_by(|a, b| a.scan_id.cmp(&b.scan_id));
        statuses
    }

    pub fn slice(&self, request: &TreeSliceRequest) -> EngineResult<TreeSlice> {
        let record = self.ready(request.scan_id.as_str())?;
        let focus = record.index.get(&request.node_id).ok_or_else(|| problem("NotFound", format!("no node {} in scan {}", request.node_id, request.scan_id)))?;
        let mut ancestors = Vec::new();
        let mut cursor = focus.parent.clone();
        while let Some(parent_id) = cursor {
            let parent = record.index.get(&parent_id).ok_or_else(|| problem("InternalError", "index lost a parent"))?;
            ancestors.insert(0, parent.summary());
            cursor = parent.parent.clone();
        }
        let built = build_slice_node(&record.index, focus, request.depth, request.max_children_per_node);
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
        let entry = record.index.get(&request.node_id).ok_or_else(|| problem("NotFound", format!("no node {} in scan {}", request.node_id, request.scan_id)))?;
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
        let items = children.into_iter().skip(request.offset).take(request.limit).collect();
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

    pub fn plan(&mut self, request: &CleanupPlanRequest) -> EngineResult<CleanupPlan> {
        if !self.allow_cleanup {
            return Err(problem("UnsupportedOperation", "cleanup is disabled in this desktop build"));
        }
        let record = self.ready(request.scan_id.as_str())?;
        let mut entries = Vec::new();
        for node_id in &request.node_ids {
            let entry = record.index.get(node_id).ok_or_else(|| problem("InvalidRequest", format!("plan references unknown node {node_id}")))?;
            let metadata = fs::metadata(&entry.path).map_err(|_| problem("StalePlan", format!("cannot stat {}", entry.path)))?;
            let mtime_ms = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as f64)
                .unwrap_or(0.0);
            entries.push(PlanEntry {
                path: entry.path.clone(),
                size: entry.size,
                reason: if entry.ignored { "ignored" } else { "staged" }.into(),
                preset: None,
                ignored: entry.ignored,
                fingerprint: EntryFingerprint { size: metadata.len(), mtime_ms },
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
        self.plans.insert(plan.plan_id.clone(), (plan.clone(), std::time::Instant::now()));
        Ok(plan)
    }

    /// Moves every planned path to the OS trash after re-verifying all
    /// fingerprints. One mismatch fails the whole plan closed.
    pub fn execute(&mut self, request: &CleanupExecuteRequest) -> EngineResult<CleanupOutcome> {
        if !request.confirm {
            return Err(problem("InvalidRequest", "cleanup requires confirm: true"));
        }
        let (plan, planned_at) = self.plans.remove(&request.plan_id).ok_or_else(|| problem("NotFound", format!("no plan {}", request.plan_id)))?;
        if planned_at.elapsed().as_secs() > 600 {
            return Err(problem("StalePlan", "plan expired; plan again from a fresh scan"));
        }
        let mut changed = 0usize;
        for entry in &plan.entries {
            let matches = fs::metadata(&entry.path)
                .ok()
                .and_then(|metadata| {
                    let mtime_ms = metadata
                        .modified()
                        .ok()
                        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                        .map(|duration| duration.as_millis() as f64)
                        .unwrap_or(0.0);
                    Some(metadata.len() == entry.fingerprint.size && mtime_ms == entry.fingerprint.mtime_ms)
                })
                .unwrap_or(false);
            if !matches {
                changed += 1;
            }
        }
        if changed > 0 {
            return Err(problem("StalePlan", format!("{changed} of {} entries changed since the plan was made", plan.entries.len())));
        }
        let mut outcome = CleanupOutcome { trashed: Vec::new(), bytes_freed: 0, failed: Vec::new() };
        for entry in &plan.entries {
            match trash::delete(&entry.path) {
                Ok(()) => {
                    outcome.bytes_freed += entry.size;
                    outcome.trashed.push(TrashedEntry { path: entry.path.clone(), size: entry.size });
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
        self.scans.get(scan_id).ok_or_else(|| problem("NotFound", format!("no scan {scan_id}")))
    }

    fn ready(&self, scan_id: &str) -> EngineResult<&ScanRecord> {
        let record = self.record(scan_id)?;
        if record.status.state != "ready" {
            return Err(problem("StaleSnapshot", format!("scan {scan_id} is not ready")));
        }
        Ok(record)
    }
}

fn walk_index(node: &ScanNode, parent: Option<String>, depth: u32, index: &mut HashMap<String, IndexEntry>) -> String {
    let path_string = node.path.to_string_lossy().to_string();
    let id = node_id_of(&path_string);
    let child_ids: Vec<String> = node.children.iter().map(|child| walk_index(child, Some(id.clone()), depth + 1, index)).collect();
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

    let mut children_sorted: Vec<&IndexEntry> = entry.child_ids.iter().filter_map(|id| index.get(id)).collect();
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
        node: TreeSliceNode { summary: entry.summary(), children: slice_children, omitted_bytes, omitted_count },
        truncated,
        omitted_bytes,
        omitted_count,
    }
}
