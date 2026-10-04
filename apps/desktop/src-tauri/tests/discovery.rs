use space_lens_desktop_lib::engine::{
  run_discovery, CleanupPlanRequest, DiscoveryKind, DiscoveryPage, DiscoveryRequest, EngineStore,
  ScanStartRequest,
};
use std::path::{Path, PathBuf};

struct Fixture(PathBuf);
impl Fixture {
  fn new() -> Self {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
      "sl-discovery-{}-{}",
      std::process::id(),
      NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&path).unwrap();
    Self(std::fs::canonicalize(path).unwrap())
  }
  fn file(&self, path: &str, bytes: usize) -> PathBuf {
    let full = self.0.join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(&full, vec![b'x'; bytes]).unwrap();
    full
  }
  fn scan(&self, respect_gitignore: bool) -> (EngineStore, String) {
    let mut store = EngineStore::new(vec![self.0.clone()], true);
    let scan = store
      .start_scan(ScanStartRequest {
        paths: vec![self.0.to_string_lossy().into_owned()],
        local_only: false,
        ignore_hidden: false,
        respect_gitignore,
        ignored_mode: Default::default(),
        label: None,
      })
      .unwrap();
    (store, scan.scan_id)
  }
}
impl Drop for Fixture {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.0);
  }
}

fn discover(
  store: &mut EngineStore,
  scan_id: &str,
  kind: DiscoveryKind,
  min_size: u64,
  offset: usize,
  limit: usize,
) -> DiscoveryPage {
  let request = DiscoveryRequest {
    scan_id: scan_id.into(),
    kind,
    min_size,
    offset,
    limit,
  };
  let data = store
    .discovery_input(&request)
    .unwrap()
    .map(run_discovery)
    .transpose()
    .unwrap();
  store.finish_discovery(&request, data).unwrap()
}

#[test]
fn finds_large_files_inside_summarized_ignored_dirs_without_expanding_browse() {
  let fixture = Fixture::new();
  fixture.file(".gitignore", 0);
  std::fs::write(fixture.0.join(".gitignore"), "node_modules/\n").unwrap();
  let large = fixture.file("node_modules/pkg/large.bin", 65536);
  fixture.file("small.txt", 1);
  fixture.file("package.json", 0);
  let (mut store, scan_id) = fixture.scan(true);
  let root_id = store.status(&scan_id).unwrap().root_ids[0].clone();
  let children_request = space_lens_desktop_lib::engine::ChildrenPageRequest {
    scan_id: scan_id.clone(),
    node_id: root_id,
    offset: 0,
    limit: 100,
    sort: "size".into(),
  };
  let before = store.children(&children_request).unwrap();
  let ignored = before
    .items
    .iter()
    .find(|node| node.name == "node_modules")
    .unwrap();
  assert!(ignored.collapsed);
  let ignored_id = ignored.id.clone();
  let page = discover(
    &mut store,
    &scan_id,
    DiscoveryKind::LargeFiles,
    8192,
    0,
    200,
  );
  assert_eq!(page.total, 1);
  assert_eq!(page.items[0].node.path, large.to_string_lossy());
  assert!(!page.items[0].is_directory);
  assert!(page.items[0].node.ignored);
  let plan = store
    .plan(&CleanupPlanRequest {
      scan_id: scan_id.clone(),
      node_ids: vec![page.items[0].node.id.clone()],
    })
    .unwrap();
  assert_eq!(plan.entries[0].path, large.to_string_lossy());
  assert!(store
    .children(&space_lens_desktop_lib::engine::ChildrenPageRequest {
      node_id: ignored_id,
      ..children_request
    })
    .unwrap()
    .items
    .is_empty());
}

#[test]
fn cache_classification_requires_project_markers_and_prunes_nested_candidates() {
  let fixture = Fixture::new();
  for path in [
    "app/node_modules/pkg/node_modules/a/file",
    "rust/target/debug/file",
    "fake/target/file",
    "python/__pycache__/a.pyc",
    "python/.pytest_cache/file",
    "python/.mypy_cache/file",
    "python/.ruff_cache/file",
    "web/.next/file",
    "web/.nuxt/file",
    "web/.turbo/file",
    "web/.parcel-cache/file",
    "web/.svelte-kit/file",
    "fake/.next/file",
    ".venv/__pycache__/file",
    "uv/cache/file",
  ] {
    fixture.file(path, 8192);
  }
  fixture.file("rust/Cargo.toml", 0);
  fixture.file("app/package.json", 0);
  fixture.file("app/node_modules/pkg/package.json", 0);
  fixture.file("web/package.json", 0);
  let (mut store, scan_id) = fixture.scan(true);
  let page = discover(&mut store, &scan_id, DiscoveryKind::Caches, 0, 0, 200);
  let paths: Vec<_> = page
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
  assert_eq!(paths.len(), 11, "{paths:?}");
  for expected in [
    "app/node_modules",
    "rust/target",
    "python/__pycache__",
    "python/.pytest_cache",
    "python/.mypy_cache",
    "python/.ruff_cache",
    "web/.next",
    "web/.nuxt",
    "web/.turbo",
    "web/.parcel-cache",
    "web/.svelte-kit",
  ] {
    assert!(
      paths.contains(&expected.to_string()),
      "missing {expected}: {paths:?}"
    );
  }
  assert!(page.items.iter().all(|item| item.is_directory));
  assert_eq!(
    page.total_size,
    page.items.iter().map(|item| item.node.size).sum::<u64>()
  );
}

#[test]
fn gitignored_discovery_classifies_rules_even_when_scan_did_not_respect_them() {
  let fixture = Fixture::new();
  std::fs::write(fixture.0.join(".gitignore"), "out/\n*.log\n").unwrap();
  fixture.file("out/nested/file", 8192);
  fixture.file("debug.log", 4096);
  fixture.file("keep.txt", 1);
  let (mut store, scan_id) = fixture.scan(false);
  let page = discover(&mut store, &scan_id, DiscoveryKind::Gitignored, 0, 0, 200);
  let names: Vec<_> = page
    .items
    .iter()
    .map(|item| item.node.name.as_str())
    .collect();
  assert_eq!(names.len(), 2, "{names:?}");
  assert!(names.contains(&"out") && names.contains(&"debug.log"));
  assert!(page.items.iter().all(|item| item.node.ignored));
}

#[test]
fn discovery_min_size_and_pages_use_filtered_totals_and_cached_results() {
  let fixture = Fixture::new();
  fixture.file("largest", 65536);
  fixture.file("medium", 32768);
  fixture.file("small", 1);
  let (mut store, scan_id) = fixture.scan(true);
  let page = discover(&mut store, &scan_id, DiscoveryKind::LargeFiles, 8192, 1, 1);
  assert_eq!(page.total, 2);
  assert_eq!(page.items[0].node.name, "medium");
  let all = discover(
    &mut store,
    &scan_id,
    DiscoveryKind::LargeFiles,
    8192,
    0,
    200,
  );
  assert_eq!(
    page.total_size,
    all.items.iter().map(|item| item.node.size).sum::<u64>()
  );
  std::fs::remove_file(fixture.0.join("largest")).unwrap();
  assert!(store
    .discovery_input(&DiscoveryRequest {
      scan_id,
      kind: DiscoveryKind::LargeFiles,
      min_size: 0,
      offset: 0,
      limit: 200
    })
    .unwrap()
    .is_none());
}

#[test]
fn cleanup_plan_deduplicates_and_prunes_descendants() {
  let fixture = Fixture::new();
  fixture.file("node_modules/pkg/file", 8192);
  fixture.file("package.json", 0);
  let (mut store, scan_id) = fixture.scan(true);
  let caches = discover(&mut store, &scan_id, DiscoveryKind::Caches, 0, 0, 200);
  let files = discover(&mut store, &scan_id, DiscoveryKind::LargeFiles, 0, 0, 200);
  let id = caches.items[0].node.id.clone();
  let plan = store
    .plan(&CleanupPlanRequest {
      scan_id,
      node_ids: vec![files.items[0].node.id.clone(), id.clone(), id],
    })
    .unwrap();
  assert_eq!(plan.entries.len(), 1);
  assert_eq!(
    plan.entries[0].path,
    fixture.0.join("node_modules").to_string_lossy()
  );
  assert_eq!(plan.total_size, caches.items[0].node.size);
}

#[cfg(unix)]
#[test]
fn discovery_excludes_symlinks_and_cleanup_rejects_replaced_ancestors() {
  let fixture = Fixture::new();
  let outside = Fixture::new();
  outside.file("large.bin", 65536);
  std::os::unix::fs::symlink(&outside.0, fixture.0.join("node_modules")).unwrap();
  std::os::unix::fs::symlink(outside.0.join("large.bin"), fixture.0.join("large-link")).unwrap();
  fixture.file("data/file", 8192);
  fixture.file("package.json", 0);
  let (mut store, scan_id) = fixture.scan(true);
  assert_eq!(
    discover(&mut store, &scan_id, DiscoveryKind::Caches, 0, 0, 200).total,
    0
  );
  let files = discover(&mut store, &scan_id, DiscoveryKind::LargeFiles, 0, 0, 200);
  assert!(!files
    .items
    .iter()
    .any(|item| item.node.name == "large-link"));
  let file = files
    .items
    .iter()
    .find(|item| item.node.name == "file")
    .unwrap();
  std::fs::remove_dir_all(fixture.0.join("data")).unwrap();
  std::os::unix::fs::symlink(&outside.0, fixture.0.join("data")).unwrap();
  assert!(store
    .plan(&CleanupPlanRequest {
      scan_id,
      node_ids: vec![file.node.id.clone()]
    })
    .is_err());
}

#[test]
fn discovery_rejects_invalid_limits_and_unknown_scans() {
  let fixture = Fixture::new();
  let (store, scan_id) = fixture.scan(true);
  for limit in [0, 1001] {
    let error = store
      .discovery_input(&DiscoveryRequest {
        scan_id: scan_id.clone(),
        kind: DiscoveryKind::Caches,
        min_size: 0,
        offset: 0,
        limit,
      })
      .err()
      .unwrap();
    assert_eq!(error.code, "InvalidRequest");
  }
  assert_eq!(
    store
      .discovery_input(&DiscoveryRequest {
        scan_id: "unknown".into(),
        kind: DiscoveryKind::Caches,
        min_size: 0,
        offset: 0,
        limit: 200
      })
      .err()
      .unwrap()
      .code,
    "NotFound"
  );
}

#[cfg(unix)]
#[test]
fn execution_rejects_symlinked_ancestor_even_when_the_fingerprint_still_matches() {
  let fixture = Fixture::new();
  fixture.file("data/file", 8192);
  let (mut store, scan_id) = fixture.scan(true);
  let files = discover(&mut store, &scan_id, DiscoveryKind::LargeFiles, 0, 0, 200);
  let plan = store
    .plan(&CleanupPlanRequest {
      scan_id,
      node_ids: vec![files.items[0].node.id.clone()],
    })
    .unwrap();
  std::fs::rename(fixture.0.join("data"), fixture.0.join("moved")).unwrap();
  std::os::unix::fs::symlink(fixture.0.join("moved"), fixture.0.join("data")).unwrap();
  let error = store
    .execute(&space_lens_desktop_lib::engine::CleanupExecuteRequest {
      plan_id: plan.plan_id,
      confirm: true,
    })
    .unwrap_err();
  assert_eq!(error.code, "StalePlan");
  assert!(fixture.0.join("moved/file").exists());
}

#[test]
fn overlapping_scan_roots_do_not_duplicate_discovery_rows_or_total_bytes() {
  let fixture = Fixture::new();
  let file = fixture.file("app/large", 8192);
  let mut store = EngineStore::new(vec![fixture.0.clone()], true);
  let session = store
    .start_scan(ScanStartRequest {
      paths: vec![
        fixture.0.to_string_lossy().into_owned(),
        fixture.0.join("app").to_string_lossy().into_owned(),
      ],
      local_only: false,
      ignore_hidden: false,
      respect_gitignore: true,
      ignored_mode: Default::default(),
      label: None,
    })
    .unwrap();
  let page = discover(
    &mut store,
    &session.scan_id,
    DiscoveryKind::LargeFiles,
    0,
    0,
    200,
  );
  assert_eq!(page.total, 1);
  assert_eq!(page.items[0].node.path, file.to_string_lossy());
  assert_eq!(page.total_size, page.items[0].node.size);
}

#[test]
fn cleanup_rejects_scan_and_configured_roots_but_accepts_descendants() {
  let fixture = Fixture::new();
  fixture.file("protected/file", 8192);
  fixture.file("ordinary/file", 8192);
  let mut store = EngineStore::new(vec![fixture.0.clone(), fixture.0.join("protected")], true);
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
  assert_eq!(
    store
      .plan(&CleanupPlanRequest {
        scan_id: session.scan_id.clone(),
        node_ids: session.root_ids.clone()
      })
      .unwrap_err()
      .code,
    "Forbidden"
  );
  let children = store
    .children(&space_lens_desktop_lib::engine::ChildrenPageRequest {
      scan_id: session.scan_id.clone(),
      node_id: session.root_ids[0].clone(),
      offset: 0,
      limit: 200,
      sort: "name".into(),
    })
    .unwrap();
  let protected = children
    .items
    .iter()
    .find(|node| node.name == "protected")
    .unwrap();
  assert_eq!(
    store
      .plan(&CleanupPlanRequest {
        scan_id: session.scan_id.clone(),
        node_ids: vec![protected.id.clone()]
      })
      .unwrap_err()
      .code,
    "Forbidden"
  );
  let ordinary = children
    .items
    .iter()
    .find(|node| node.name == "ordinary")
    .unwrap();
  assert_eq!(
    store
      .plan(&CleanupPlanRequest {
        scan_id: session.scan_id,
        node_ids: vec![ordinary.id.clone()]
      })
      .unwrap()
      .entries
      .len(),
    1
  );
}

#[test]
fn planning_rejects_files_replaced_after_discovery_including_ignored_files() {
  for respect_gitignore in [false, true] {
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join(".gitignore"), "node_modules/\n").unwrap();
    let file = fixture.file("node_modules/pkg/large.bin", 8192);
    let (mut store, scan_id) = fixture.scan(respect_gitignore);
    let page = discover(
      &mut store,
      &scan_id,
      DiscoveryKind::LargeFiles,
      8192,
      0,
      200,
    );
    assert_eq!(page.total, 1);
    std::fs::remove_file(&file).unwrap();
    std::fs::write(&file, vec![b'y'; 16384]).unwrap();
    let error = store
      .plan(&CleanupPlanRequest {
        scan_id,
        node_ids: vec![page.items[0].node.id.clone()],
      })
      .unwrap_err();
    assert_eq!(error.code, "StaleSnapshot");
    assert!(file.exists());
  }
}

#[test]
fn planning_rejects_files_changed_after_the_original_scan_without_discovery() {
  let fixture = Fixture::new();
  let file = fixture.file("file", 8192);
  let (mut store, scan_id) = fixture.scan(true);
  let root_id = store.status(&scan_id).unwrap().root_ids[0].clone();
  let children = store
    .children(&space_lens_desktop_lib::engine::ChildrenPageRequest {
      scan_id: scan_id.clone(),
      node_id: root_id,
      offset: 0,
      limit: 200,
      sort: "name".into(),
    })
    .unwrap();
  std::fs::write(&file, vec![b'y'; 16384]).unwrap();
  assert_eq!(
    store
      .plan(&CleanupPlanRequest {
        scan_id,
        node_ids: vec![children.items[0].id.clone()]
      })
      .unwrap_err()
      .code,
    "StaleSnapshot"
  );
}

#[test]
fn discovery_refreshes_cleanup_snapshot_without_changing_original_browse_sizes() {
  let fixture = Fixture::new();
  let file = fixture.file("file", 8192);
  let (mut store, scan_id) = fixture.scan(true);
  let root_id = store.status(&scan_id).unwrap().root_ids[0].clone();
  let request = space_lens_desktop_lib::engine::ChildrenPageRequest {
    scan_id: scan_id.clone(),
    node_id: root_id,
    offset: 0,
    limit: 200,
    sort: "name".into(),
  };
  let original = store.children(&request).unwrap();
  std::fs::write(&file, vec![b'y'; 65536]).unwrap();
  let page = discover(
    &mut store,
    &scan_id,
    DiscoveryKind::LargeFiles,
    8192,
    0,
    200,
  );
  assert!(page.items[0].node.size > original.items[0].size);
  assert_eq!(
    store.children(&request).unwrap().items[0].size,
    original.items[0].size
  );
  let plan = store
    .plan(&CleanupPlanRequest {
      scan_id,
      node_ids: vec![page.items[0].node.id.clone()],
    })
    .unwrap();
  assert_eq!(plan.total_size, page.items[0].node.size);
  assert_eq!(plan.entries[0].fingerprint.size, 65536);
}
