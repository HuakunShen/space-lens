use space_lens_desktop_lib::engine::{
  run_discovery, ChildrenPageRequest, CleanupExecuteRequest, CleanupPlanRequest, DiscoveryKind,
  DiscoveryPage, DiscoveryRequest, EngineStore, PreparedDiscovery, ScanStartRequest,
};
use std::path::{Path, PathBuf};

struct Fixture(PathBuf);
impl Fixture {
  fn new() -> Self {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
      "sl-native-local-{}-{}",
      std::process::id(),
      NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
    ));
    std::fs::create_dir_all(path.join("data")).unwrap();
    std::fs::write(path.join("data/file"), vec![b'x'; 8192]).unwrap();
    Self(std::fs::canonicalize(path).unwrap())
  }
  fn request(&self) -> ScanStartRequest {
    serde_json::from_value(serde_json::json!({
      "paths": [self.0], "localOnly": true, "respectGitignore": true,
      "ignoredMode": "summarize", "ignoreHidden": false,
    }))
    .unwrap()
  }
}
impl Drop for Fixture {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.0);
  }
}

fn root_children(
  store: &mut EngineStore,
  scan_id: &str,
) -> space_lens_desktop_lib::engine::ChildrenPage {
  let root_id = store.status(scan_id).unwrap().root_ids[0].clone();
  store
    .children(&ChildrenPageRequest {
      scan_id: scan_id.to_string(),
      node_id: root_id,
      offset: 0,
      limit: 200,
      sort: "size".into(),
    })
    .unwrap()
}

/// Runs the three-view derivation through the public flow and returns the
/// requested kind's page. The caller must not pass `gitignored` on an
/// unclassified scan (that is a typed error, tested separately).
fn discover_protected(
  store: &mut EngineStore,
  scan_id: &str,
  kind: DiscoveryKind,
) -> DiscoveryPage {
  let request = DiscoveryRequest {
    scan_id: scan_id.to_string(),
    kind,
    min_size: 0,
    offset: 0,
    limit: 200,
  };
  let data = match store.prepare_discovery(&request).unwrap() {
    PreparedDiscovery::Ready(data) => Some(data),
    PreparedDiscovery::Cached => None,
    PreparedDiscovery::Walk(_) => {
      panic!("protected discovery must never snapshot legacy walk inputs")
    }
  };
  let page = store.finish_discovery(&request, data).unwrap();
  assert_eq!(page.scan_id, request.scan_id);
  page
}

#[cfg(unix)]
#[test]
fn local_only_scan_reports_coverage_derives_discovery_and_allows_cleanup() {
  let fixture = Fixture::new();
  let mut store = EngineStore::new(vec![fixture.0.clone()]);
  let session = store.start_scan(fixture.request()).unwrap();
  let status = serde_json::to_value(store.status(&session.scan_id).unwrap()).unwrap();
  assert_eq!(status["coverage"]["mode"], "local-only");
  assert_eq!(status["coverage"]["sizeMetric"], "allocated");
  assert_eq!(status["volumes"][0]["isLocal"], true);
  let node = root_children(&mut store, &session.scan_id)
    .items
    .into_iter()
    .find(|node| node.name == "data")
    .unwrap();
  let summary = serde_json::to_value(&node).unwrap();
  assert_eq!(summary["isDirectory"], true);
  assert_eq!(summary["logicalSize"], 8192);
  assert_eq!(summary["scanState"], "complete");
  // A complete protected entry plans with the freshly measured fingerprint
  // as the plan-time baseline.
  let plan = store
    .plan(&CleanupPlanRequest {
      scan_id: session.scan_id.clone(),
      node_ids: vec![node.id.clone()],
    })
    .unwrap();
  assert_eq!(plan.mode, "trash");
  assert_eq!(plan.entries.len(), 1);
  assert_eq!(
    plan.entries[0].path,
    fixture.0.join("data").to_string_lossy().to_string()
  );
  assert_eq!(
    plan.entries[0].fingerprint.size,
    std::fs::symlink_metadata(fixture.0.join("data"))
      .unwrap()
      .len()
  );
  // Discovery is derived in memory from the indexed report; no legacy walk
  // inputs are ever produced for a protected scan.
  let page = discover_protected(&mut store, &session.scan_id, DiscoveryKind::LargeFiles);
  assert_eq!(page.total, 1);
  assert_eq!(
    page.items[0].node.path,
    fixture.0.join("data/file").to_string_lossy().to_string()
  );
  assert_eq!(page.items[0].category, "Large file");
  assert!(fixture.0.join("data/file").exists());
}

#[test]
fn protected_root_containment_is_lexical_even_when_the_path_does_not_exist() {
  let fixture = Fixture::new();
  let mut store = EngineStore::new(vec![fixture.0.clone()]);
  let request = serde_json::from_value(serde_json::json!({
    "paths": [fixture.0.join("../not-served-no-such-path")], "localOnly": true,
  }))
  .unwrap();
  assert_eq!(store.start_scan(request).unwrap_err().code, "Forbidden");
}

#[test]
fn protected_prepare_is_nonblocking_and_cancellation_is_terminal() {
  let fixture = Fixture::new();
  let mut store = EngineStore::new(vec![fixture.0.clone()]);
  // Preparing an in-scope missing path must not access the filesystem.
  let request = serde_json::from_value(serde_json::json!({
    "paths": [fixture.0.join("no-such-path")], "localOnly": true,
  }))
  .unwrap();
  let (session, task) = store.prepare_local_scan(request).unwrap();
  assert!(session.root_ids.is_empty());
  assert_eq!(store.status(&session.scan_id).unwrap().state, "scanning");
  assert_eq!(
    store
      .prepare_local_scan(fixture.request())
      .err()
      .unwrap()
      .code,
    "LimitExceeded"
  );
  let legacy_request = serde_json::from_value(serde_json::json!({
    "paths": [fixture.0], "localOnly": false,
  }))
  .unwrap();
  assert_eq!(
    store.start_scan(legacy_request).unwrap_err().code,
    "LimitExceeded"
  );
  assert_eq!(
    store.cancel_scan(&session.scan_id).unwrap().state,
    "cancelled"
  );
  assert!(task.cancellation.is_cancelled());
  assert_eq!(
    store
      .prepare_local_scan(fixture.request())
      .err()
      .unwrap()
      .code,
    "LimitExceeded",
    "cancelling must not release the running worker slot"
  );
  let legacy_request = serde_json::from_value(serde_json::json!({
    "paths": [fixture.0], "localOnly": false,
  }))
  .unwrap();
  assert_eq!(
    store.start_scan(legacy_request).unwrap_err().code,
    "LimitExceeded"
  );
  // A delayed worker error must not revive or overwrite a cancelled scan.
  let status = store
    .finish_local_scan(
      &session.scan_id,
      Err(std::io::Error::other("late worker error")),
    )
    .unwrap();
  assert_eq!(status.state, "cancelled");
  assert_eq!(status.message, "Local scan cancelled");
  let legacy_request = serde_json::from_value(serde_json::json!({
    "paths": [fixture.0], "localOnly": false,
  }))
  .unwrap();
  assert!(
    store.start_scan(legacy_request).is_ok(),
    "finished workers must release the slot for legacy scans"
  );
  assert!(
    store.prepare_local_scan(fixture.request()).is_ok(),
    "finished workers must release the slot for local scans"
  );
}

#[test]
fn protected_worker_failure_stays_failed_without_legacy_fallback() {
  let fixture = Fixture::new();
  let mut store = EngineStore::new(vec![fixture.0.clone()]);
  let (session, _task) = store.prepare_local_scan(fixture.request()).unwrap();
  let status = store
    .finish_local_scan(
      &session.scan_id,
      Err(std::io::Error::other("policy setup refused")),
    )
    .unwrap();
  assert_eq!(status.state, "failed");
  assert!(status.message.contains("policy setup refused"));
  assert!(status.root_ids.is_empty());
  assert!(status.coverage.is_none());
  assert!(fixture.0.join("data/file").exists());
}

#[test]
fn protected_prepare_preserves_explicit_nested_roots_and_only_deduplicates_exact_paths() {
  let fixture = Fixture::new();
  let mut store = EngineStore::new(vec![fixture.0.clone()]);
  let request = serde_json::from_value(serde_json::json!({
    "paths": [fixture.0, fixture.0.join("data"), fixture.0.join("./data")],
    "localOnly": true,
  }))
  .unwrap();
  let (_session, task) = store.prepare_local_scan(request).unwrap();
  assert_eq!(
    task.options.directories,
    vec![fixture.0.clone(), fixture.0.join("data")]
  );
}

#[cfg(unix)]
#[test]
fn protected_scan_keeps_skipped_symlinks_and_rejects_cleanup_of_skipped_entries() {
  let fixture = Fixture::new();
  std::os::unix::fs::symlink("no-such-target", fixture.0.join("link")).unwrap();
  // A skipped child marks its directory partial, so `part` is a plannable
  // (non-root) entry with scan_state "partial".
  std::fs::create_dir_all(fixture.0.join("part")).unwrap();
  std::os::unix::fs::symlink("no-such-target", fixture.0.join("part/inner")).unwrap();
  let mut store = EngineStore::new(vec![fixture.0.clone()]);
  let session = store.start_scan(fixture.request()).unwrap();
  let children = root_children(&mut store, &session.scan_id);
  let link = children
    .items
    .iter()
    .find(|node| node.name == "link")
    .unwrap();
  assert_eq!(link.scan_state.as_deref(), Some("skipped"));
  assert_eq!(link.skip_reason.as_deref(), Some("symlink"));
  assert_eq!(link.size, 0);
  std::fs::remove_file(fixture.0.join("link")).unwrap();
  // The guard fires before any metadata probe: the path is gone, yet the
  // rejection is about the skipped entry, not a stale path.
  assert_eq!(
    store
      .plan(&CleanupPlanRequest {
        scan_id: session.scan_id.clone(),
        node_ids: vec![link.id.clone()],
      })
      .unwrap_err()
      .code,
    "InvalidRequest"
  );
  let part = children
    .items
    .iter()
    .find(|node| node.name == "part")
    .unwrap();
  assert_eq!(part.scan_state.as_deref(), Some("partial"));
  assert_eq!(
    store
      .plan(&CleanupPlanRequest {
        scan_id: session.scan_id,
        node_ids: vec![part.id.clone()],
      })
      .unwrap_err()
      .code,
    "InvalidRequest"
  );
}

#[cfg(unix)]
#[test]
fn protected_discovery_classifies_caches_gitignored_and_dedupes_children() {
  let fixture = Fixture::new();
  // Marker-gated caches: node_modules needs package.json, target needs Cargo.toml.
  std::fs::create_dir_all(fixture.0.join("project/node_modules")).unwrap();
  std::fs::write(fixture.0.join("project/package.json"), b"{}").unwrap();
  std::fs::write(fixture.0.join("project/node_modules/x"), vec![b'x'; 4096]).unwrap();
  std::fs::create_dir_all(fixture.0.join("crate/target")).unwrap();
  std::fs::write(fixture.0.join("crate/Cargo.toml"), b"[package]\n").unwrap();
  std::fs::write(fixture.0.join("crate/target/x"), vec![b'x'; 4096]).unwrap();
  // Manifest-free caches classify unconditionally.
  std::fs::create_dir_all(fixture.0.join("ab/__pycache__")).unwrap();
  std::fs::write(fixture.0.join("ab/__pycache__/b.pyc"), vec![b'x'; 1024]).unwrap();
  std::fs::create_dir_all(fixture.0.join("za/__pycache__")).unwrap();
  std::fs::write(fixture.0.join("za/__pycache__/a.pyc"), vec![b'x'; 1024]).unwrap();
  // A virtualenv environment is excluded from cache classification.
  std::fs::create_dir_all(fixture.0.join(".venv/lib/__pycache__")).unwrap();
  std::fs::write(
    fixture.0.join(".venv/lib/__pycache__/a.pyc"),
    vec![b'x'; 1024],
  )
  .unwrap();
  // Gitignored rows keep only the outermost directory of a nested chain.
  std::fs::write(fixture.0.join(".gitignore"), b"cache_out/\n").unwrap();
  std::fs::create_dir_all(fixture.0.join("cache_out/nested")).unwrap();
  std::fs::write(fixture.0.join("cache_out/nested/blob"), vec![b'x'; 4096]).unwrap();
  let mut store = EngineStore::new(vec![fixture.0.clone()]);
  let session = store.start_scan(fixture.request()).unwrap();
  let caches = discover_protected(&mut store, &session.scan_id, DiscoveryKind::Caches);
  let names: Vec<String> = caches
    .items
    .iter()
    .map(|item| {
      Path::new(&item.node.path)
        .strip_prefix(&fixture.0)
        .unwrap()
        .to_string_lossy()
        .into_owned()
    })
    .collect();
  assert_eq!(
    names,
    vec![
      "ab/__pycache__",
      "crate/target",
      "project/node_modules",
      "za/__pycache__",
    ],
    "equal allocated sizes fall back to path byte order; no .venv entry"
  );
  assert_eq!(caches.items[0].category, "Python bytecode");
  assert_eq!(caches.items[1].category, "Rust build output");
  assert_eq!(caches.items[2].category, "Node dependencies");
  assert!(caches.items.iter().all(|item| item.is_directory));
  let gitignored = discover_protected(&mut store, &session.scan_id, DiscoveryKind::Gitignored);
  let names: Vec<String> = gitignored
    .items
    .iter()
    .map(|item| {
      Path::new(&item.node.path)
        .strip_prefix(&fixture.0)
        .unwrap()
        .to_string_lossy()
        .into_owned()
    })
    .collect();
  assert_eq!(
    names,
    vec!["cache_out"],
    "nested ignored children dedupe away"
  );
  assert_eq!(gitignored.items[0].category, "Gitignored");
  let files = discover_protected(&mut store, &session.scan_id, DiscoveryKind::LargeFiles);
  assert!(
    files
      .items
      .iter()
      .any(|item| item.node.path.ends_with("cache_out/nested/blob") && item.node.ignored),
    "ignored files still surface as large files"
  );
  assert!(
    !files
      .items
      .iter()
      .any(|item| item.node.path.ends_with("/__pycache__")),
    "directories never appear in the large-files view"
  );
}

#[test]
fn protected_scan_without_gitignore_classification_rejects_gitignored_discovery() {
  let fixture = Fixture::new();
  let request = serde_json::from_value(serde_json::json!({
    "paths": [fixture.0], "localOnly": true, "respectGitignore": false,
    "ignoredMode": "summarize", "ignoreHidden": false,
  }))
  .unwrap();
  let mut store = EngineStore::new(vec![fixture.0.clone()]);
  let session = store.start_scan(request).unwrap();
  // Match instead of unwrap_err: the ok type carries no Debug impl, and a
  // successful prepare here would itself be the bug.
  let error = match store.prepare_discovery(&DiscoveryRequest {
    scan_id: session.scan_id,
    kind: DiscoveryKind::Gitignored,
    min_size: 0,
    offset: 0,
    limit: 200,
  }) {
    Err(error) => error,
    Ok(_) => panic!("an unclassified protected scan must refuse gitignored discovery"),
  };
  assert_eq!(error.code, "UnsupportedOperation");
}

#[test]
fn protected_cleanup_fails_closed_when_paths_change_between_plan_and_execute() {
  let fixture = Fixture::new();
  let mut store = EngineStore::new(vec![fixture.0.clone()]);
  let session = store.start_scan(fixture.request()).unwrap();
  // Plan a file: writing inside the parent directory would not change the
  // directory's own fingerprint, but it does change the file's.
  let planned_file = |store: &mut EngineStore, scan_id: &str| {
    let root = store.status(scan_id).unwrap().root_ids[0].clone();
    let children = |node_id: String| {
      store
        .children(&ChildrenPageRequest {
          scan_id: scan_id.to_string(),
          node_id,
          offset: 0,
          limit: 200,
          sort: "size".into(),
        })
        .unwrap()
        .items
    };
    let data = children(root)
      .into_iter()
      .find(|node| node.name == "data")
      .unwrap();
    children(data.id)
      .into_iter()
      .find(|node| node.name == "file")
      .unwrap()
  };
  // A modified planned path is stale at execute time.
  let node = planned_file(&mut store, &session.scan_id);
  let plan = store
    .plan(&CleanupPlanRequest {
      scan_id: session.scan_id.clone(),
      node_ids: vec![node.id.clone()],
    })
    .unwrap();
  std::fs::write(fixture.0.join("data/file"), vec![b'y'; 16384]).unwrap();
  assert_eq!(
    store
      .execute(&CleanupExecuteRequest {
        plan_id: plan.plan_id,
        confirm: true,
      })
      .unwrap_err()
      .code,
    "StalePlan"
  );
  // A vanished planned path is stale already at plan time.
  let node = planned_file(&mut store, &session.scan_id);
  std::fs::remove_file(fixture.0.join("data/file")).unwrap();
  assert_eq!(
    store
      .plan(&CleanupPlanRequest {
        scan_id: session.scan_id,
        node_ids: vec![node.id.clone()],
      })
      .unwrap_err()
      .code,
    "StalePlan"
  );
}

#[test]
fn protected_cleanup_executes_through_trash_or_reports_a_typed_failure() {
  let fixture = Fixture::new();
  let mut store = EngineStore::new(vec![fixture.0.clone()]);
  let session = store.start_scan(fixture.request()).unwrap();
  let node = root_children(&mut store, &session.scan_id)
    .items
    .into_iter()
    .find(|node| node.name == "data")
    .unwrap();
  let plan = store
    .plan(&CleanupPlanRequest {
      scan_id: session.scan_id,
      node_ids: vec![node.id],
    })
    .unwrap();
  let outcome = store
    .execute(&CleanupExecuteRequest {
      plan_id: plan.plan_id,
      confirm: true,
    })
    .unwrap();
  // Both endings are correct: `trashed` when the test process may talk to
  // Finder, a typed `failed` entry when it may not (headless CI). What the
  // contract forbids is a silent success or an untyped crash.
  let attempted = outcome.trashed.len() + outcome.failed.len();
  assert_eq!(attempted, 1);
  if let Some(failure) = outcome.failed.first() {
    assert_eq!(failure.code, "Unavailable");
  }
}

#[test]
fn legacy_discovery_still_walks_the_filesystem_outside_protected_scans() {
  // The legacy (non-local-only) flow keeps its walk-based discovery: derive
  // inputs snapshot under the lock, run_discovery works without a store.
  let fixture = Fixture::new();
  let mut store = EngineStore::new(vec![fixture.0.clone()]);
  let session = store
    .start_scan(ScanStartRequest {
      paths: vec![fixture.0.to_string_lossy().into_owned()],
      local_only: false,
      ignore_hidden: false,
      respect_gitignore: true,
      ignored_mode: Default::default(),
      label: None,
    })
    .unwrap();
  let request = DiscoveryRequest {
    scan_id: session.scan_id.clone(),
    kind: DiscoveryKind::LargeFiles,
    min_size: 0,
    offset: 0,
    limit: 200,
  };
  match store.prepare_discovery(&request).unwrap() {
    PreparedDiscovery::Walk(input) => {
      let data = run_discovery(input).unwrap();
      let page = store.finish_discovery(&request, Some(data)).unwrap();
      assert!(page
        .items
        .iter()
        .any(|item| item.node.path.ends_with("data/file")));
    }
    _ => panic!("legacy scans still discover through the filesystem walk"),
  }
  // A second request hits the cache instead of walking again.
  assert!(matches!(
    store.prepare_discovery(&request).unwrap(),
    PreparedDiscovery::Cached
  ));
  assert_eq!(
    store
      .plan(&CleanupPlanRequest {
        scan_id: session.scan_id,
        node_ids: session.root_ids,
      })
      .unwrap_err()
      .code,
    "Forbidden",
    "scan roots still cannot be cleaned up"
  );
}
