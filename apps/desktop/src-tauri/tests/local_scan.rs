use space_lens_desktop_lib::engine::{
  ChildrenPageRequest, CleanupPlanRequest, DiscoveryKind, DiscoveryRequest, EngineStore,
  ScanStartRequest,
};
use std::path::PathBuf;

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

#[cfg(unix)]
#[test]
fn local_only_scan_reports_coverage_and_does_not_enter_legacy_cleanup_or_discovery() {
  let fixture = Fixture::new();
  let mut store = EngineStore::new(vec![fixture.0.clone()], true);
  let session = store.start_scan(fixture.request()).unwrap();
  let status = serde_json::to_value(store.status(&session.scan_id).unwrap()).unwrap();
  assert_eq!(status["coverage"]["mode"], "local-only");
  assert_eq!(status["coverage"]["sizeMetric"], "allocated");
  assert_eq!(status["volumes"][0]["isLocal"], true);
  let children = store
    .children(&ChildrenPageRequest {
      scan_id: session.scan_id.clone(),
      node_id: session.root_ids[0].clone(),
      offset: 0,
      limit: 200,
      sort: "size".into(),
    })
    .unwrap();
  let node = children
    .items
    .iter()
    .find(|node| node.name == "data")
    .unwrap();
  let summary = serde_json::to_value(node).unwrap();
  assert_eq!(summary["isDirectory"], true);
  assert_eq!(summary["logicalSize"], 8192);
  assert_eq!(summary["scanState"], "complete");
  assert_eq!(
    store
      .plan(&CleanupPlanRequest {
        scan_id: session.scan_id.clone(),
        node_ids: vec![node.id.clone()],
      })
      .unwrap_err()
      .code,
    "Forbidden"
  );
  assert_eq!(
    store
      .discovery_input(&DiscoveryRequest {
        scan_id: session.scan_id,
        kind: DiscoveryKind::LargeFiles,
        min_size: 0,
        offset: 0,
        limit: 200,
      })
      .err()
      .unwrap()
      .code,
    "Unavailable"
  );
  assert!(fixture.0.join("data/file").exists());
}

#[test]
fn protected_root_containment_is_lexical_even_when_the_path_does_not_exist() {
  let fixture = Fixture::new();
  let mut store = EngineStore::new(vec![fixture.0.clone()], true);
  let request = serde_json::from_value(serde_json::json!({
    "paths": [fixture.0.join("../not-served-no-such-path")], "localOnly": true,
  }))
  .unwrap();
  assert_eq!(store.start_scan(request).unwrap_err().code, "Forbidden");
}

#[test]
fn protected_prepare_is_nonblocking_and_cancellation_is_terminal() {
  let fixture = Fixture::new();
  let mut store = EngineStore::new(vec![fixture.0.clone()], true);
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
    "cancelling must not release the running worker slot",
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
  let mut store = EngineStore::new(vec![fixture.0.clone()], true);
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
  let mut store = EngineStore::new(vec![fixture.0.clone()], true);
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
fn protected_scan_keeps_skipped_symlinks_and_rejects_cleanup_before_metadata_probes() {
  let fixture = Fixture::new();
  std::os::unix::fs::symlink("no-such-target", fixture.0.join("link")).unwrap();
  let mut store = EngineStore::new(vec![fixture.0.clone()], true);
  let session = store.start_scan(fixture.request()).unwrap();
  let children = store
    .children(&ChildrenPageRequest {
      scan_id: session.scan_id.clone(),
      node_id: session.root_ids[0].clone(),
      offset: 0,
      limit: 200,
      sort: "name".into(),
    })
    .unwrap();
  let link = children
    .items
    .iter()
    .find(|node| node.name == "link")
    .unwrap();
  assert_eq!(link.scan_state.as_deref(), Some("skipped"));
  assert_eq!(link.skip_reason.as_deref(), Some("symlink"));
  assert_eq!(link.size, 0);
  std::fs::remove_file(fixture.0.join("link")).unwrap();
  assert_eq!(
    store
      .plan(&CleanupPlanRequest {
        scan_id: session.scan_id,
        node_ids: vec![link.id.clone()],
      })
      .unwrap_err()
      .code,
    "Forbidden"
  );
}
