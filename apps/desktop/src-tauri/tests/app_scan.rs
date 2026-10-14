//! Engine-lane tests for app virtual-root sessions (plan §8 E1, as amended).
//!
//! Boundary: the desktop engine + shared policy only. No IPC runtime, no
//! filesystem walks outside temp fixtures, no vendored-engine changes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use space_lens::local_scan::{
  scan_local_directory_with_progress, LocalScanNode, LocalScanReport, ScanCoverage, ScanIssue,
};
use space_lens_desktop_lib::app_cleanup::{
  plan_app_cleanup, reveal_app_location, AppCleanupPlanRequest, AppCleanupState, AuditSnapshot,
};
use space_lens_desktop_lib::engine::{
  AppScanOrigin, AppScanStartRequest, ChildrenPageRequest, CleanupExecuteRequest,
  CleanupPlanRequest, DiscoveryKind, DiscoveryRequest, EngineStore, PreparedDiscovery,
  TreeSliceRequest,
};
use spacelens_app_audit::{
  AppAuditEntry, AppAuditLocation, AppAuditReport, AppKind, AuditSource, Classification,
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
  root: PathBuf,
}

impl Fixture {
  fn tagged(tag: &str) -> Self {
    let root = std::env::temp_dir().join(format!(
      "sl-app-scan-{tag}-{}-{}",
      std::process::id(),
      NEXT.fetch_add(1, Ordering::SeqCst),
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    Self {
      root: root.canonicalize().unwrap(),
    }
  }

  fn dir(&self, relative: &str) -> PathBuf {
    let path = self.root.join(relative);
    std::fs::create_dir_all(&path).unwrap();
    path
  }

  fn file(&self, relative: &str, bytes: usize) -> PathBuf {
    let path = self.root.join(relative);
    if let Some(parent) = path.parent() {
      std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&path, vec![b'x'; bytes]).unwrap();
    path
  }
}

impl Drop for Fixture {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.root);
  }
}

const AUDIT_ID: &str = "audit_abcdef1234567890abcdef";

fn snapshot(
  audit_id: &str,
  locations: &[(&str, Vec<PathBuf>)],
  bundles: &[(&str, PathBuf)],
) -> AuditSnapshot {
  AuditSnapshot {
    audit_id: audit_id.into(),
    report: serde_json::json!({}),
    locations_by_app: locations
      .iter()
      .map(|(app, paths)| (app.to_string(), paths.clone()))
      .collect(),
    app_paths: bundles
      .iter()
      .map(|(app, path)| (app.to_string(), path.clone()))
      .collect(),
  }
}

fn report_entry(
  id: &str,
  name: &str,
  app_path: Option<PathBuf>,
  installed_size: u64,
  locations: &[PathBuf],
) -> AppAuditEntry {
  AppAuditEntry {
    id: id.into(),
    name: name.into(),
    kind: AppKind::App,
    source: AuditSource::Heuristic,
    bundle_id: None,
    app_path: app_path.map(|path| path.to_string_lossy().into_owned()),
    installed_size,
    total_size: installed_size,
    locations: locations
      .iter()
      .map(|path| AppAuditLocation {
        path: path.to_string_lossy().into_owned(),
        label: None,
        classification: Classification::Cache,
        purgeable: true,
        comment: None,
        size: 0,
        source: AuditSource::Heuristic,
      })
      .collect(),
  }
}

fn audit_report(entries: Vec<AppAuditEntry>) -> AppAuditReport {
  AppAuditReport {
    platform: "test".into(),
    generated_at: "2026-10-13T00:00:00Z".into(),
    apps: entries,
    total_apps: 0,
    truncated: false,
  }
}

/// Prepare + synchronously walk + finish one app scan. The origin/locations/
/// bundle captures come from the same snapshot the prepare call validated —
/// exactly what `submit_app_scan_reply` passes (never a reread).
fn run_app_scan(
  store: &mut EngineStore,
  snap: &AuditSnapshot,
  report: &AppAuditReport,
  app_id: &str,
) -> String {
  let request: AppScanStartRequest = serde_json::from_value(serde_json::json!({
    "auditId": snap.audit_id,
    "appId": app_id,
  }))
  .unwrap();
  let entry = report.apps.iter().find(|app| app.id == app_id).unwrap();
  let origin = AppScanOrigin {
    app_id: app_id.into(),
    app_name: entry.name.clone(),
    audit_id: snap.audit_id.clone(),
  };
  let locations: Vec<PathBuf> = snap
    .locations_by_app
    .get(app_id)
    .cloned()
    .unwrap_or_default();
  let bundle = snap
    .app_paths
    .get(app_id)
    .map(|path| (path.clone(), entry.installed_size, entry.name.clone()));
  let (session, task) = store.prepare_app_scan(request, snap, report).unwrap();
  assert_eq!(session.app.clone().unwrap(), origin);
  let result = scan_local_directory_with_progress(task.options, task.cancellation, |_| {});
  let status = store
    .finish_app_scan(&session.scan_id, result, origin, locations, bundle)
    .unwrap();
  assert_eq!(status.state, "ready");
  assert!(status.app.is_some());
  session.scan_id
}

fn vroot(store: &mut EngineStore, scan_id: &str) -> String {
  store.status(scan_id).unwrap().root_ids[0].clone()
}

fn children_of(store: &EngineStore, scan_id: &str, node_id: &str) -> Vec<String> {
  store
    .children(&ChildrenPageRequest {
      scan_id: scan_id.into(),
      node_id: node_id.into(),
      offset: 0,
      limit: 512,
      sort: "size".into(),
    })
    .unwrap()
    .items
    .into_iter()
    .map(|item| item.id)
    .collect()
}

/// One-level child id lookup by exact path.
fn child_id_by_path(store: &EngineStore, scan_id: &str, parent_id: &str, want: &Path) -> String {
  let want = want.to_string_lossy().into_owned();
  store
    .children(&ChildrenPageRequest {
      scan_id: scan_id.into(),
      node_id: parent_id.into(),
      offset: 0,
      limit: 512,
      sort: "name".into(),
    })
    .unwrap()
    .items
    .into_iter()
    .find(|item| item.path == want)
    .unwrap_or_else(|| panic!("no child {want} under {parent_id}"))
    .id
}

// Walker-driven app scans need the pinned local traversal backend, which is
// unix-only (kuntu NativeBackend answers Unsupported elsewhere) — same gate
// as the protected-cleanup tests in local_scan.rs. Refusal-semantics tests
// below stay ungated: on Windows they assert the typed-failure path.
#[cfg(unix)]
#[test]
fn virtual_topology_ids_depths_and_sizes() {
  let fixture = Fixture::tagged("topology");
  let loc_a = fixture.dir("loc-a");
  let loc_b = fixture.dir("loc-b");
  fixture.file("loc-a/a.bin", 4096);
  fixture.file("loc-b/b.bin", 2048);
  let bundle = fixture.dir("Fixture.app");
  let snap = snapshot(
    AUDIT_ID,
    &[("app1", vec![loc_a.clone(), loc_b.clone()])],
    &[("app1", bundle.clone())],
  );
  let report = audit_report(vec![report_entry(
    "app1",
    "Fixture",
    Some(bundle.clone()),
    12345,
    &[loc_a.clone(), loc_b.clone()],
  )]);
  let mut store = EngineStore::new(vec![fixture.root.clone()]);
  let scan_id = run_app_scan(&mut store, &snap, &report, "app1");

  // Pinned virtual conventions: approot_ + 24 hex, app-storage: path.
  let status = store.status(&scan_id).unwrap();
  let status_json = serde_json::to_value(&status).unwrap();
  assert_eq!(status_json["app"]["appId"], "app1");
  assert_eq!(status_json["app"]["appName"], "Fixture");
  assert_eq!(status_json["app"]["auditId"], AUDIT_ID);
  let virtual_id = status.root_ids[0].clone();
  assert!(virtual_id.starts_with("approot_"));
  assert_eq!(virtual_id.len(), "approot_".len() + 24);
  let slice = store
    .slice(&TreeSliceRequest {
      scan_id: scan_id.clone(),
      node_id: virtual_id.clone(),
      depth: 1,
      max_children_per_node: 512,
    })
    .unwrap();
  assert_eq!(slice.focus_node.depth, 0);
  assert_eq!(slice.focus_node.name, "Fixture");
  assert!(slice.focus_node.path.starts_with("app-storage:"));
  assert_eq!(slice.focus_node.path.len(), "app-storage:".len() + 24);
  assert!(slice.ancestors.is_empty());

  // Children: two locations + the bundle leaf, all at depth 1.
  assert_eq!(slice.tree.children.len(), 3);
  for child in &slice.tree.children {
    assert_eq!(child.summary.depth, 1);
  }
  let bundle_child = slice
    .tree
    .children
    .iter()
    .find(|child| child.summary.path == bundle.to_string_lossy())
    .expect("bundle leaf injected");
  assert!(bundle_child.summary.id.starts_with("appbundle_"));
  assert_eq!(bundle_child.summary.id.len(), "appbundle_".len() + 24);
  assert_eq!(bundle_child.summary.size, 12345);
  // Virtual size reconciles: location children + audit-size bundle leaf.
  let children_total: u64 = slice
    .tree
    .children
    .iter()
    .map(|child| child.summary.size)
    .sum();
  assert_eq!(slice.focus_node.size, children_total);
  assert_eq!(slice.total_size, children_total);

  // Below the locations, the real protected tree (depths shifted +1).
  let loc_child = slice
    .tree
    .children
    .iter()
    .find(|child| child.summary.path == loc_a.to_string_lossy())
    .unwrap();
  let deep = store
    .slice(&TreeSliceRequest {
      scan_id: scan_id.clone(),
      node_id: loc_child.summary.id.clone(),
      depth: 1,
      max_children_per_node: 512,
    })
    .unwrap();
  assert!(deep
    .tree
    .children
    .iter()
    .any(|child| child.summary.depth == 2 && child.summary.path.ends_with("a.bin")));
}

// Walker-driven app scans need the pinned local traversal backend, which is
// unix-only (kuntu NativeBackend answers Unsupported elsewhere) — same gate
// as the protected-cleanup tests in local_scan.rs. Refusal-semantics tests
// below stay ungated: on Windows they assert the typed-failure path.
#[cfg(unix)]
#[test]
fn location_roots_cleanable_virtual_and_bundle_refused() {
  let fixture = Fixture::tagged("guards");
  let loc = fixture.dir("loc");
  fixture.file("loc/data.bin", 1024);
  let bundle = fixture.dir("App.app");
  let snap = snapshot(
    AUDIT_ID,
    &[("app1", vec![loc.clone()])],
    &[("app1", bundle.clone())],
  );
  let report = audit_report(vec![report_entry(
    "app1",
    "App",
    Some(bundle.clone()),
    999,
    std::slice::from_ref(&loc),
  )]);
  let mut store = EngineStore::new(vec![fixture.root.clone()]);
  let scan_id = run_app_scan(&mut store, &snap, &report, "app1");
  let virtual_id = vroot(&mut store, &scan_id);
  let kids = children_of(&store, &scan_id, &virtual_id);
  assert_eq!(kids.len(), 2);

  // The location root stages (inversion of the scan-root rule).
  let plan = store
    .plan(&CleanupPlanRequest {
      scan_id: scan_id.clone(),
      node_ids: kids.clone(),
    })
    .unwrap_err();
  // Staging the whole virtual family at once must fail on the virtual root
  // or bundle leaf, never silently skip them.
  assert_eq!(plan.code, "Forbidden");

  // Virtual root alone: refused.
  let refused = store
    .plan(&CleanupPlanRequest {
      scan_id: scan_id.clone(),
      node_ids: vec![virtual_id.clone()],
    })
    .unwrap_err();
  assert_eq!(refused.code, "Forbidden");

  // Descendants stage; the physical size is freshly measured bytes.
  let loc_id = child_id_by_path(&store, &scan_id, &virtual_id, &loc);
  let plan = store
    .plan(&CleanupPlanRequest {
      scan_id: scan_id.clone(),
      node_ids: vec![loc_id],
    })
    .unwrap();
  assert_eq!(plan.entries.len(), 1);
  assert_eq!(plan.entries[0].size, 1024);
}

#[test]
fn configured_roots_stay_refused_on_the_app_route() {
  let fixture = Fixture::tagged("configured");
  let loc = fixture.dir("loc");
  fixture.file("loc/data.bin", 512);
  // The location IS a served root: the shared policy must still refuse it as
  // a configured root.
  let snap = snapshot(AUDIT_ID, &[("app1", vec![loc.clone()])], &[]);
  let report = audit_report(vec![report_entry(
    "app1",
    "App",
    None,
    0,
    std::slice::from_ref(&loc),
  )]);
  let mut store = EngineStore::new(vec![loc.clone()]);
  let scan_id = run_app_scan(&mut store, &snap, &report, "app1");
  let virtual_id = vroot(&mut store, &scan_id);
  let kids = children_of(&store, &scan_id, &virtual_id);
  let refused = store
    .plan(&CleanupPlanRequest {
      scan_id: scan_id.clone(),
      node_ids: kids,
    })
    .unwrap_err();
  assert_eq!(refused.code, "Forbidden");
}

// Walker-driven app scans need the pinned local traversal backend, which is
// unix-only (kuntu NativeBackend answers Unsupported elsewhere) — same gate
// as the protected-cleanup tests in local_scan.rs. Refusal-semantics tests
// below stay ungated: on Windows they assert the typed-failure path.
#[cfg(unix)]
#[test]
fn stale_audit_id_rejected_and_pinned_generation_cleans_up() {
  let fixture = Fixture::tagged("pinned");
  let loc = fixture.dir("loc");
  fixture.file("loc/data.bin", 1024);
  let snap = snapshot(AUDIT_ID, &[("app1", vec![loc.clone()])], &[]);
  let report = audit_report(vec![report_entry(
    "app1",
    "App",
    None,
    0,
    std::slice::from_ref(&loc),
  )]);
  let mut store = EngineStore::new(vec![fixture.root.clone()]);

  // Stale generation: prepare refuses without touching the filesystem walk.
  let stale_request: AppScanStartRequest = serde_json::from_value(serde_json::json!({
    "auditId": "audit_000000000000000000000000", "appId": "app1",
  }))
  .unwrap();
  let stale = match store.prepare_app_scan(stale_request, &snap, &report) {
    Ok(_) => panic!("stale audit must be refused"),
    Err(problem) => problem,
  };
  assert_eq!(stale.code, "InvalidRequest");

  // The pinned generation plans and executes with no snapshot in hand —
  // finish/plan/execute consume only the stored context (A7).
  let scan_id = run_app_scan(&mut store, &snap, &report, "app1");
  let virtual_id = vroot(&mut store, &scan_id);
  let kids = children_of(&store, &scan_id, &virtual_id);
  let plan = store
    .plan(&CleanupPlanRequest {
      scan_id: scan_id.clone(),
      node_ids: kids,
    })
    .unwrap();
  let outcome = store
    .execute(&CleanupExecuteRequest {
      plan_id: plan.plan_id,
      confirm: true,
    })
    .unwrap();
  assert_eq!(outcome.trashed.len() + outcome.failed.len(), 1);
  if !outcome.trashed.is_empty() {
    assert_eq!(outcome.bytes_freed, 1024);
    assert!(store.status(&scan_id).unwrap().message.contains("rescan"));
  }
}

// Walker-driven app scans need the pinned local traversal backend, which is
// unix-only (kuntu NativeBackend answers Unsupported elsewhere) — same gate
// as the protected-cleanup tests in local_scan.rs. Refusal-semantics tests
// below stay ungated: on Windows they assert the typed-failure path.
#[cfg(unix)]
#[test]
fn nested_locations_partition_and_accept_separate_roots() {
  let fixture = Fixture::tagged("nested");
  let parent = fixture.dir("parent");
  let child = fixture.dir("parent/child");
  fixture.file("parent/top.bin", 4096);
  fixture.file("parent/child/nested.bin", 4096);
  let snap = snapshot(
    AUDIT_ID,
    &[("app1", vec![parent.clone(), child.clone()])],
    &[],
  );
  let report = audit_report(vec![report_entry(
    "app1",
    "App",
    None,
    0,
    &[parent.clone(), child.clone()],
  )]);
  let mut store = EngineStore::new(vec![fixture.root.clone()]);
  let scan_id = run_app_scan(&mut store, &snap, &report, "app1");

  // Both roots are whole-folder cleanable: the only issues are separate-root
  // partitions between authorized roots (A1 evidence).
  let virtual_id = vroot(&mut store, &scan_id);
  let kids = children_of(&store, &scan_id, &virtual_id);
  assert_eq!(kids.len(), 2);
  let plan = store
    .plan(&CleanupPlanRequest {
      scan_id: scan_id.clone(),
      node_ids: kids,
    })
    .unwrap();
  // Pruned to the top-most ancestor with the full physical union measured.
  assert_eq!(plan.entries.len(), 1);
  assert_eq!(plan.entries[0].size, 8192);
}

fn synthetic_report(
  nodes: Vec<LocalScanNode>,
  issues: Vec<ScanIssue>,
  issues_truncated: bool,
) -> LocalScanReport {
  LocalScanReport {
    nodes,
    volumes: Vec::new(),
    coverage: ScanCoverage {
      mode: "local-only".into(),
      protection: "test".into(),
      size_metric: "allocated".into(),
      logical_bytes: 0,
      files: 0,
      directories: 0,
      skipped_count: 0,
      denied_count: 0,
      issue_count: issues.len() as u64,
      issues_truncated,
      issues,
      elapsed_ms: 0,
    },
  }
}

fn complete_dir(path: &Path, children: Vec<LocalScanNode>, state: &str) -> LocalScanNode {
  LocalScanNode {
    name: path.to_string_lossy().into_owned(),
    path: path.to_path_buf(),
    size: 100,
    children,
    depth: 0,
    ignored: false,
    collapsed: false,
    logical_size: 0,
    is_directory: true,
    scan_state: state.into(),
    skip_reason: None,
  }
}

#[test]
fn incomplete_evidence_disables_folder_staging_but_allows_contents() {
  let fixture = Fixture::tagged("evidence");
  let loc = fixture.dir("loc");
  let inner = fixture.dir("loc/inner");
  fixture.file("loc/inner/keep.bin", 512);
  let snap = snapshot(AUDIT_ID, &[("app1", vec![loc.clone()])], &[]);
  let report = audit_report(vec![report_entry(
    "app1",
    "App",
    None,
    0,
    std::slice::from_ref(&loc),
  )]);
  let mut store = EngineStore::new(vec![fixture.root.clone()]);

  // Prepare for real, then finish with a synthetic report: a TCC-style denial
  // under the location plus an iterator failure, on top of nesting.
  let request: AppScanStartRequest = serde_json::from_value(serde_json::json!({
    "auditId": AUDIT_ID, "appId": "app1",
  }))
  .unwrap();
  let origin = AppScanOrigin {
    app_id: "app1".into(),
    app_name: "App".into(),
    audit_id: AUDIT_ID.into(),
  };
  let (session, task) = store.prepare_app_scan(request, &snap, &report).unwrap();
  drop(task);
  let file_node = LocalScanNode {
    name: inner.join("keep.bin").to_string_lossy().into_owned(),
    path: inner.join("keep.bin"),
    size: 512,
    children: Vec::new(),
    depth: 0,
    ignored: false,
    collapsed: false,
    logical_size: 512,
    is_directory: false,
    scan_state: "complete".into(),
    skip_reason: None,
  };
  let inner_node = complete_dir(&inner, vec![file_node], "complete");
  let root_node = complete_dir(&loc, vec![inner_node], "partial");
  let finished = store
    .finish_app_scan(
      &session.scan_id,
      Ok(synthetic_report(
        vec![root_node],
        vec![
          ScanIssue {
            path: inner.clone(),
            reason: "separate-root".into(),
            message: "nested".into(),
          },
          ScanIssue {
            path: loc.join("denied"),
            reason: "denied".into(),
            message: "tcc".into(),
          },
          ScanIssue {
            path: loc.clone(),
            reason: "iterator-failure".into(),
            message: "read_dir failed".into(),
          },
        ],
        false,
      )),
      origin,
      vec![loc.clone()],
      None,
    )
    .unwrap();
  assert_eq!(finished.state, "ready");
  let scan_id = session.scan_id;

  // Folder-level staging of the location root is disabled (contents-only).
  let virtual_id = vroot(&mut store, &scan_id);
  let loc_id = child_id_by_path(&store, &scan_id, &virtual_id, &loc);
  let refused = store
    .plan(&CleanupPlanRequest {
      scan_id: scan_id.clone(),
      node_ids: vec![loc_id.clone()],
    })
    .unwrap_err();
  assert_eq!(refused.code, "InvalidRequest");

  // Its fully measured contents still stage.
  let inner_kids = children_of(&store, &scan_id, &loc_id);
  assert!(!inner_kids.is_empty());
  let plan = store
    .plan(&CleanupPlanRequest {
      scan_id: scan_id.clone(),
      node_ids: inner_kids,
    })
    .unwrap();
  assert_eq!(plan.entries.len(), 1);
}

#[test]
fn truncated_evidence_is_conservatively_incomplete() {
  let fixture = Fixture::tagged("truncated");
  let parent = fixture.dir("parent");
  let child = fixture.dir("parent/child");
  fixture.file("parent/top.bin", 1024);
  fixture.file("parent/child/n.bin", 1024);
  let snap = snapshot(
    AUDIT_ID,
    &[("app1", vec![parent.clone(), child.clone()])],
    &[],
  );
  let report = audit_report(vec![report_entry(
    "app1",
    "App",
    None,
    0,
    &[parent.clone(), child.clone()],
  )]);
  let mut store = EngineStore::new(vec![fixture.root.clone()]);
  let request: AppScanStartRequest = serde_json::from_value(serde_json::json!({
    "auditId": AUDIT_ID, "appId": "app1",
  }))
  .unwrap();
  let origin = AppScanOrigin {
    app_id: "app1".into(),
    app_name: "App".into(),
    audit_id: AUDIT_ID.into(),
  };
  let (session, task) = store.prepare_app_scan(request, &snap, &report).unwrap();
  drop(task);
  // An identical nesting shape, but the issue list was truncated: the engine
  // cannot prove completeness, so folder staging stays disabled.
  let child_node = complete_dir(&child, Vec::new(), "complete");
  let parent_node = complete_dir(&parent, Vec::new(), "partial");
  store
    .finish_app_scan(
      &session.scan_id,
      Ok(synthetic_report(
        vec![parent_node, child_node],
        vec![ScanIssue {
          path: child.clone(),
          reason: "separate-root".into(),
          message: String::new(),
        }],
        true,
      )),
      origin,
      vec![parent.clone(), child.clone()],
      None,
    )
    .unwrap();
  let virtual_id = vroot(&mut store, &session.scan_id);
  let kids = children_of(&store, &session.scan_id, &virtual_id);
  let refused = store
    .plan(&CleanupPlanRequest {
      scan_id: session.scan_id.clone(),
      node_ids: kids,
    })
    .unwrap_err();
  assert_eq!(refused.code, "InvalidRequest");
}

// Walker-driven app scans need the pinned local traversal backend, which is
// unix-only (kuntu NativeBackend answers Unsupported elsewhere) — same gate
// as the protected-cleanup tests in local_scan.rs. Refusal-semantics tests
// below stay ungated: on Windows they assert the typed-failure path.
#[cfg(unix)]
#[test]
fn grandchild_drift_fails_the_engine_plan_stale() {
  let fixture = Fixture::tagged("drift");
  let loc = fixture.dir("loc");
  fixture.file("loc/data.bin", 4096);
  let snap = snapshot(AUDIT_ID, &[("app1", vec![loc.clone()])], &[]);
  let report = audit_report(vec![report_entry(
    "app1",
    "App",
    None,
    0,
    std::slice::from_ref(&loc),
  )]);
  let mut store = EngineStore::new(vec![fixture.root.clone()]);
  let scan_id = run_app_scan(&mut store, &snap, &report, "app1");
  let virtual_id = vroot(&mut store, &scan_id);
  let kids = children_of(&store, &scan_id, &virtual_id);
  let plan = store
    .plan(&CleanupPlanRequest {
      scan_id: scan_id.clone(),
      node_ids: kids,
    })
    .unwrap();
  // Grow a grandchild file: the recursive manifest must catch the drift even
  // when the top-level fingerprint is untouched.
  fixture.file("loc/data.bin", 8192);
  let stale = store
    .execute(&CleanupExecuteRequest {
      plan_id: plan.plan_id,
      confirm: true,
    })
    .unwrap_err();
  assert_eq!(stale.code, "StalePlan");
}

#[test]
fn file_locations_become_stat_leaves_and_bundle_only_sessions_work() {
  let fixture = Fixture::tagged("files");
  let plist = fixture.file("Library/Preferences/com.example.plist", 256);
  let bundle = fixture.dir("Solo.app");
  // App with one file location + bundle.
  let snap = snapshot(
    AUDIT_ID,
    &[("app1", vec![plist.clone()])],
    &[("app1", bundle.clone())],
  );
  let report = audit_report(vec![report_entry(
    "app1",
    "Solo",
    Some(bundle.clone()),
    777,
    std::slice::from_ref(&plist),
  )]);
  let mut store = EngineStore::new(vec![fixture.root.clone()]);
  let scan_id = run_app_scan(&mut store, &snap, &report, "app1");
  let virtual_id = vroot(&mut store, &scan_id);
  // The file leaf stages alone with its stat size; staging the whole virtual
  // family is refused on the bundle leaf.
  let plist_id = child_id_by_path(&store, &scan_id, &virtual_id, &plist);
  let plan = store
    .plan(&CleanupPlanRequest {
      scan_id: scan_id.clone(),
      node_ids: vec![plist_id],
    })
    .unwrap();
  assert_eq!(plan.entries.len(), 1);
  assert_eq!(plan.entries[0].size, 256);
  let kids = children_of(&store, &scan_id, &virtual_id);
  let refused = store
    .plan(&CleanupPlanRequest {
      scan_id: scan_id.clone(),
      node_ids: kids,
    })
    .unwrap_err();
  assert_eq!(refused.code, "Forbidden");

  // Bundle-only app: no locations at all, still a valid session.
  let bundle_only = fixture.dir("Only.app");
  let snap2 = snapshot(
    AUDIT_ID,
    &[("solo", vec![])],
    &[("solo", bundle_only.clone())],
  );
  let mut report2 = audit_report(vec![report_entry(
    "solo",
    "Only",
    Some(bundle_only.clone()),
    4242,
    &[],
  )]);
  report2.apps[0].locations = Vec::new();
  let scan2 = run_app_scan(&mut store, &snap2, &report2, "solo");
  let virtual2 = vroot(&mut store, &scan2);
  let kids2 = children_of(&store, &scan2, &virtual2);
  assert_eq!(kids2.len(), 1);
  let refused = store
    .plan(&CleanupPlanRequest {
      scan_id: scan2.clone(),
      node_ids: kids2,
    })
    .unwrap_err();
  assert_eq!(refused.code, "Forbidden");
}

// Walker-driven app scans need the pinned local traversal backend, which is
// unix-only (kuntu NativeBackend answers Unsupported elsewhere) — same gate
// as the protected-cleanup tests in local_scan.rs. Refusal-semantics tests
// below stay ungated: on Windows they assert the typed-failure path.
#[cfg(unix)]
#[test]
fn bundle_containment_runs_both_ways_on_the_engine_route() {
  let fixture = Fixture::tagged("containment");
  // The reported location contains the bundle: no separate leaf is injected
  // (no double count) and every overlapping target is refused.
  let loc = fixture.dir("apps-data");
  let bundle = fixture.dir("apps-data/Foo.app");
  fixture.file("apps-data/Foo.app/Contents/bin", 512);
  fixture.file("apps-data/notes.txt", 128);
  let snap = snapshot(
    AUDIT_ID,
    &[("app1", vec![loc.clone()])],
    &[("app1", bundle.clone())],
  );
  let report = audit_report(vec![report_entry(
    "app1",
    "Foo",
    Some(bundle.clone()),
    5000,
    std::slice::from_ref(&loc),
  )]);
  let mut store = EngineStore::new(vec![fixture.root.clone()]);
  let scan_id = run_app_scan(&mut store, &snap, &report, "app1");
  let virtual_id = vroot(&mut store, &scan_id);
  let kids = children_of(&store, &scan_id, &virtual_id);
  // Only the location child: the bundle leaf was correctly withheld.
  assert_eq!(kids.len(), 1);

  // The location itself contains the bundle: refused.
  let refused = store
    .plan(&CleanupPlanRequest {
      scan_id: scan_id.clone(),
      node_ids: kids,
    })
    .unwrap_err();
  assert_eq!(refused.code, "Forbidden");

  // A target inside the bundle is refused; a disjoint sibling stages.
  let loc_id = children_of(&store, &scan_id, &virtual_id)[0].clone();
  let inner: HashMap<String, String> = store
    .children(&ChildrenPageRequest {
      scan_id: scan_id.clone(),
      node_id: loc_id,
      offset: 0,
      limit: 512,
      sort: "name".into(),
    })
    .unwrap()
    .items
    .into_iter()
    .map(|item| (item.name, item.id))
    .collect();
  let refused = store
    .plan(&CleanupPlanRequest {
      scan_id: scan_id.clone(),
      node_ids: vec![inner["Foo.app"].clone()],
    })
    .unwrap_err();
  assert_eq!(refused.code, "Forbidden");
  let plan = store
    .plan(&CleanupPlanRequest {
      scan_id: scan_id.clone(),
      node_ids: vec![inner["notes.txt"].clone()],
    })
    .unwrap();
  assert_eq!(plan.entries.len(), 1);
  assert_eq!(plan.entries[0].size, 128);
}

#[test]
fn cross_app_ownership_overlap_is_refused() {
  let fixture = Fixture::tagged("overlap");
  let owned = fixture.dir("shared");
  let nested = fixture.dir("shared/sub");
  fixture.file("shared/top.bin", 512);
  fixture.file("shared/sub/n.bin", 512);
  // Manually constructed overlapping ownership: app2 owns a path nested
  // inside app1's location.
  let snap = snapshot(
    AUDIT_ID,
    &[
      ("app1", vec![owned.clone()]),
      ("app2", vec![nested.clone()]),
    ],
    &[],
  );
  let report = audit_report(vec![
    report_entry("app1", "One", None, 0, std::slice::from_ref(&owned)),
    report_entry("app2", "Two", None, 0, std::slice::from_ref(&nested)),
  ]);
  let mut store = EngineStore::new(vec![fixture.root.clone()]);
  let scan_id = run_app_scan(&mut store, &snap, &report, "app1");
  let virtual_id = vroot(&mut store, &scan_id);
  let kids = children_of(&store, &scan_id, &virtual_id);
  // app1's location contains app2's owned path: the whole-folder stage is
  // refused even with complete coverage evidence.
  let refused = store
    .plan(&CleanupPlanRequest {
      scan_id: scan_id.clone(),
      node_ids: kids,
    })
    .unwrap_err();
  assert_eq!(refused.code, "Forbidden");
}

// Walker-driven app scans need the pinned local traversal backend, which is
// unix-only (kuntu NativeBackend answers Unsupported elsewhere) — same gate
// as the protected-cleanup tests in local_scan.rs. Refusal-semantics tests
// below stay ungated: on Windows they assert the typed-failure path.
#[cfg(unix)]
#[test]
fn dual_api_equivalence_for_identical_targets() {
  let fixture = Fixture::tagged("dual");
  let loc = fixture.dir("loc");
  fixture.file("loc/a.bin", 2048);
  fixture.file("loc/sub/b.bin", 1024);
  std::fs::create_dir_all(loc.join("sub")).unwrap();
  let bundle = fixture.dir("Dual.app");
  let snap = snapshot(
    AUDIT_ID,
    &[("app1", vec![loc.clone()])],
    &[("app1", bundle.clone())],
  );
  let report = audit_report(vec![report_entry(
    "app1",
    "Dual",
    Some(bundle.clone()),
    111,
    std::slice::from_ref(&loc),
  )]);
  let mut store = EngineStore::new(vec![fixture.root.clone()]);
  let scan_id = run_app_scan(&mut store, &snap, &report, "app1");
  let virtual_id = vroot(&mut store, &scan_id);
  let loc_id = child_id_by_path(&store, &scan_id, &virtual_id, &loc);
  let engine_plan = store
    .plan(&CleanupPlanRequest {
      scan_id: scan_id.clone(),
      node_ids: vec![loc_id],
    })
    .unwrap();

  let mut standalone = AppCleanupState::default();
  let value = plan_app_cleanup(
    &mut standalone,
    &snap,
    &AppCleanupPlanRequest {
      audit_id: AUDIT_ID.into(),
      app_id: "app1".into(),
      paths: vec![loc.to_string_lossy().into_owned()],
    },
    &store_roots(&store),
  )
  .unwrap();
  let standalone_entries = value["entries"].as_array().unwrap();
  assert_eq!(standalone_entries.len(), 1);
  // Identical targets, identical physical sizes through both APIs.
  assert_eq!(
    standalone_entries[0]["size"].as_u64().unwrap(),
    engine_plan.entries[0].size
  );
  assert_eq!(engine_plan.entries[0].size, 3072);

  // Both APIs refuse the bundle itself.
  let bundle_forbidden = plan_app_cleanup(
    &mut standalone,
    &snap,
    &AppCleanupPlanRequest {
      audit_id: AUDIT_ID.into(),
      app_id: "app1".into(),
      paths: vec![bundle.to_string_lossy().into_owned()],
    },
    &store_roots(&store),
  )
  .unwrap_err();
  assert_eq!(bundle_forbidden["problem"]["code"], "Forbidden");
}

fn store_roots(store: &EngineStore) -> Vec<PathBuf> {
  let roots: serde_json::Value = serde_json::to_value(store.roots()).unwrap();
  roots
    .as_array()
    .unwrap()
    .iter()
    .map(|root| PathBuf::from(root["path"].as_str().unwrap()))
    .collect()
}

// Walker-driven app scans need the pinned local traversal backend, which is
// unix-only (kuntu NativeBackend answers Unsupported elsewhere) — same gate
// as the protected-cleanup tests in local_scan.rs. Refusal-semantics tests
// below stay ungated: on Windows they assert the typed-failure path.
#[cfg(unix)]
#[test]
fn symlink_ancestor_drift_is_stale_and_synthetic_inputs_rejected() {
  let fixture = Fixture::tagged("symlink");
  let loc = fixture.dir("loc");
  fixture.file("loc/data.bin", 256);
  let snap = snapshot(AUDIT_ID, &[("app1", vec![loc.clone()])], &[]);
  let report = audit_report(vec![report_entry(
    "app1",
    "App",
    None,
    0,
    std::slice::from_ref(&loc),
  )]);
  let mut store = EngineStore::new(vec![fixture.root.clone()]);
  let scan_id = run_app_scan(&mut store, &snap, &report, "app1");
  let virtual_id = vroot(&mut store, &scan_id);
  let kids = children_of(&store, &scan_id, &virtual_id);

  // Swap the location for a symlink to elsewhere: fail closed.
  let moved = fixture.root.join("loc-moved");
  std::fs::rename(&loc, &moved).unwrap();
  #[cfg(unix)]
  std::os::unix::fs::symlink(&moved, &loc).unwrap();
  let stale = store
    .plan(&CleanupPlanRequest {
      scan_id: scan_id.clone(),
      node_ids: kids,
    })
    .unwrap_err();
  #[cfg(unix)]
  assert_eq!(stale.code, "StalePlan");
  #[cfg(not(unix))]
  assert_eq!(stale.code, "StalePlan");
  let _ = std::fs::remove_dir_all(&moved);

  // Synthetic identifiers never become scan inputs (A10).
  let synthetic_request = serde_json::from_value(serde_json::json!({
    "paths": ["app-storage:deadbeef"], "localOnly": true,
  }))
  .unwrap();
  let synthetic = match store.prepare_local_scan(synthetic_request) {
    Ok(_) => panic!("synthetic paths must be refused"),
    Err(problem) => problem,
  };
  assert_eq!(synthetic.code, "InvalidRequest");

  // ... nor reveal targets.
  let forbidden = reveal_app_location(Some(&snap), "app1", "app-storage:deadbeef").unwrap_err();
  assert_eq!(forbidden["problem"]["code"], "Forbidden");
}

#[test]
fn session_isolation_at_the_engine_boundary() {
  let fixture = Fixture::tagged("isolation");
  let loc_a = fixture.dir("a");
  let loc_b = fixture.dir("b");
  fixture.file("a/x.bin", 128);
  fixture.file("b/y.bin", 128);
  let snap = snapshot(
    AUDIT_ID,
    &[("app1", vec![loc_a.clone()]), ("app2", vec![loc_b.clone()])],
    &[],
  );
  let report = audit_report(vec![
    report_entry("app1", "One", None, 0, std::slice::from_ref(&loc_a)),
    report_entry("app2", "Two", None, 0, std::slice::from_ref(&loc_b)),
  ]);
  let mut store = EngineStore::new(vec![fixture.root.clone()]);
  let scan_a = run_app_scan(&mut store, &snap, &report, "app1");
  let scan_b = run_app_scan(&mut store, &snap, &report, "app2");

  // Every session carries its own origin marker on status and list.
  assert_eq!(
    store.status(&scan_a).unwrap().app.clone().unwrap().app_id,
    "app1"
  );
  assert_eq!(
    store.status(&scan_b).unwrap().app.clone().unwrap().app_id,
    "app2"
  );
  let listed: HashMap<String, String> = store
    .list()
    .into_iter()
    .map(|status| (status.scan_id, status.app.unwrap().app_id))
    .collect();
  assert_eq!(listed[&scan_a], "app1");
  assert_eq!(listed[&scan_b], "app2");

  // Plans are scoped by scan id: a node from scan A is unknown to scan B.
  let vroot_a = vroot(&mut store, &scan_a);
  let kids_a = children_of(&store, &scan_a, &vroot_a);
  let foreign = store
    .plan(&CleanupPlanRequest {
      scan_id: scan_b.clone(),
      node_ids: kids_a,
    })
    .unwrap_err();
  assert_eq!(foreign.code, "InvalidRequest");

  // Cancelling one session leaves the other ready.
  store.cancel_scan(&scan_a).unwrap();
  assert_eq!(store.status(&scan_a).unwrap().state, "ready");
  assert_eq!(store.status(&scan_b).unwrap().state, "ready");
}

// Walker-driven app scans need the pinned local traversal backend, which is
// unix-only (kuntu NativeBackend answers Unsupported elsewhere) — same gate
// as the protected-cleanup tests in local_scan.rs. Refusal-semantics tests
// below stay ungated: on Windows they assert the typed-failure path.
#[cfg(unix)]
#[test]
fn discovery_never_surfaces_the_bundle_leaf() {
  let fixture = Fixture::tagged("discovery");
  let loc = fixture.dir("loc");
  fixture.file("loc/big.bin", 8192);
  let bundle = fixture.dir("node_modules");
  fixture.file("node_modules/package.json", 64);
  let snap = snapshot(
    AUDIT_ID,
    &[("app1", vec![loc.clone()])],
    &[("app1", bundle.clone())],
  );
  let report = audit_report(vec![report_entry(
    "app1",
    "App",
    Some(bundle.clone()),
    64,
    std::slice::from_ref(&loc),
  )]);
  let mut store = EngineStore::new(vec![fixture.root.clone()]);
  let scan_id = run_app_scan(&mut store, &snap, &report, "app1");
  let request = DiscoveryRequest {
    scan_id: scan_id.clone(),
    kind: DiscoveryKind::LargeFiles,
    min_size: 0,
    offset: 0,
    limit: 200,
  };
  let data = match store.prepare_discovery(&request).unwrap() {
    PreparedDiscovery::Ready(data) => Some(data),
    PreparedDiscovery::Cached => None,
    PreparedDiscovery::Walk(_) => panic!("app discovery must stay in memory"),
  };
  let page = store.finish_discovery(&request, data).unwrap();
  assert!(page
    .items
    .iter()
    .any(|item| item.node.path.ends_with("big.bin")));
  assert!(!page
    .items
    .iter()
    .any(|item| item.node.path == bundle.to_string_lossy()));
}
