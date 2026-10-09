use space_lens_desktop_lib::engine::{
  CleanupExecuteRequest, CleanupPlanRequest, EngineStore, ScanStartRequest,
};

fn fixture() -> (tempdir::TempDir, EngineStore) {
  let dir = tempdir::TempDir::new("sl-desktop").unwrap();
  std::fs::create_dir_all(dir.path().join("data")).unwrap();
  std::fs::write(dir.path().join("data").join("f.txt"), "x".repeat(1024)).unwrap();
  let store = EngineStore::new(vec![dir.path().to_path_buf()]);
  (dir, store)
}

// A tiny stand-in for the tempfile crate: no extra dependency for one test.
mod tempdir {
  use std::path::PathBuf;
  use std::sync::atomic::{AtomicU64, Ordering};

  pub struct TempDir(PathBuf);

  impl TempDir {
    pub fn new(prefix: &str) -> std::io::Result<Self> {
      static COUNTER: AtomicU64 = AtomicU64::new(0);
      let unique = format!(
        "{prefix}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
      );
      let path = std::env::temp_dir().join(unique);
      std::fs::create_dir_all(&path)?;
      Ok(TempDir(path))
    }
    pub fn path(&self) -> &PathBuf {
      &self.0
    }
  }

  impl Drop for TempDir {
    fn drop(&mut self) {
      let _ = std::fs::remove_dir_all(&self.0);
    }
  }
}

#[test]
fn scan_slice_children_round_trip() {
  let (_dir, mut store) = fixture();
  let session = store
    .start_scan(ScanStartRequest {
      paths: vec![_dir.path().to_string_lossy().to_string()],
      local_only: false,
      ignore_hidden: false,
      respect_gitignore: true,
      ignored_mode: Default::default(),
      label: None,
    })
    .unwrap();
  assert_eq!(session.root_ids.len(), 1);
  let status = store.status(&session.scan_id).unwrap();
  assert_eq!(status.state, "ready");

  let slice = store
    .slice(&space_lens_desktop_lib::engine::TreeSliceRequest {
      scan_id: session.scan_id.clone(),
      node_id: session.root_ids[0].clone(),
      depth: 2,
      max_children_per_node: 10,
    })
    .unwrap();
  assert_eq!(
    slice.focus_node.name,
    _dir.path().file_name().unwrap().to_string_lossy()
  );
  assert!(slice
    .tree
    .children
    .iter()
    .any(|child| child.summary.name == "data"));

  let children = store
    .children(&space_lens_desktop_lib::engine::ChildrenPageRequest {
      scan_id: session.scan_id,
      node_id: session.root_ids[0].clone(),
      offset: 0,
      limit: 10,
      sort: "size".into(),
    })
    .unwrap();
  assert!(children.total >= 1);
}

#[test]
fn refuses_paths_outside_roots() {
  let (_dir, mut store) = fixture();
  // The temp dir is the fixture root's parent, so it exists on every
  // platform and is always outside the served roots — unlike a hardcoded
  // FHS path such as /usr, which means nothing on Windows.
  let error = store
    .start_scan(ScanStartRequest {
      paths: vec![std::env::temp_dir().to_string_lossy().to_string()],
      local_only: false,
      ignore_hidden: false,
      respect_gitignore: true,
      ignored_mode: Default::default(),
      label: None,
    })
    .unwrap_err();
  assert_eq!(error.code, "Forbidden");
}

#[test]
fn execute_requires_confirm_and_rejects_unknown_plans() {
  let (_dir, mut store) = fixture();
  let refused = store
    .execute(&CleanupExecuteRequest {
      plan_id: "plan_none".into(),
      confirm: false,
    })
    .unwrap_err();
  assert_eq!(refused.code, "InvalidRequest");
  let missing = store
    .execute(&CleanupExecuteRequest {
      plan_id: "plan_none".into(),
      confirm: true,
    })
    .unwrap_err();
  assert_eq!(missing.code, "NotFound");
}

#[test]
fn plan_execute_trashes_staged_entries() {
  let (dir, mut store) = fixture();
  let session = store
    .start_scan(ScanStartRequest {
      paths: vec![dir.path().to_string_lossy().to_string()],
      local_only: false,
      ignore_hidden: false,
      respect_gitignore: true,
      ignored_mode: Default::default(),
      label: None,
    })
    .unwrap();
  // plan the data directory itself
  let slice = store
    .slice(&space_lens_desktop_lib::engine::TreeSliceRequest {
      scan_id: session.scan_id.clone(),
      node_id: session.root_ids[0].clone(),
      depth: 2,
      max_children_per_node: 10,
    })
    .unwrap();
  let data = slice
    .tree
    .children
    .into_iter()
    .find(|c| c.summary.name == "data")
    .unwrap();
  let plan = store
    .plan(&CleanupPlanRequest {
      scan_id: session.scan_id.clone(),
      node_ids: vec![data.summary.id.clone()],
    })
    .unwrap();
  assert_eq!(plan.mode, "trash");
  let outcome = store
    .execute(&CleanupExecuteRequest {
      plan_id: plan.plan_id.clone(),
      confirm: true,
    })
    .unwrap();
  // Both endings are correct here: `trashed` when the test process may talk
  // to Finder, a typed `failed` entry when it may not (headless CI). What
  // the contract forbids is a silent success or an untyped crash.
  let attempted = outcome.trashed.len() + outcome.failed.len();
  assert_eq!(attempted, 1);
  if let Some(failure) = outcome.failed.first() {
    assert_eq!(failure.code, "Unavailable");
  }
  // one-shot plan
  let gone = store
    .execute(&CleanupExecuteRequest {
      plan_id: plan.plan_id,
      confirm: true,
    })
    .unwrap_err();
  assert_eq!(gone.code, "NotFound");
}

#[test]
fn roots_carry_live_capacity() {
  let (_dir, store) = fixture();
  let roots = store.roots();
  assert_eq!(roots.len(), 1);
  let root = &roots[0];
  // The fixture lives on a real filesystem, so capacity must be populated
  // and self-consistent: size is the volume total, used is within it.
  assert!(root.size > 0, "root size should be the volume total, got 0");
  let used = root.used.expect("root should carry used bytes pre-scan");
  assert!(used <= root.size, "used {used} exceeds total {}", root.size);
}

#[test]
fn roots_without_capacity_stay_unknown() {
  let store = EngineStore::new(vec![std::path::PathBuf::from("/definitely/not/here-9f8e7d6c")]);
  let roots = store.roots();
  assert_eq!(roots.len(), 1);
  assert_eq!(roots[0].size, 0);
  assert_eq!(roots[0].used, None, "unstatable roots must be unknown, not zero");
}

#[test]
fn volumes_lists_a_usable_startup_volume() {
  let (_dir, store) = fixture();
  let volumes = store.volumes();
  assert!(!volumes.is_empty(), "at least the startup volume must list");
  let root = volumes.iter().find(|v| v.path == std::path::Path::new("/")).expect("startup volume / must list");
  assert!(root.total_bytes > 0);
  assert!(root.free_bytes <= root.total_bytes);
  assert!(root.available_bytes <= root.total_bytes);
}
