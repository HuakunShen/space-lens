//! Measures the engine's memory after a protected scan, stage by stage.
//!
//! `cargo run --release --example scan_footprint -- / /System/Volumes/Data`
//!
//! Prints the process's physical footprint (what Activity Monitor shows as
//! "Memory" on macOS) after the walk, after indexing, and after the
//! large-files discovery, plus the lifetime peak.

use std::path::PathBuf;
use std::time::Instant;

use space_lens::local_scan::scan_local_directory_with_progress;
use space_lens_desktop_lib::engine::{
  ChildrenPageRequest, DiscoveryKind, DiscoveryRequest, EngineStore, PreparedDiscovery,
  ScanStartRequest, TreeSliceRequest, WireIgnoredMode,
};

#[cfg(target_os = "macos")]
fn footprint() -> (u64, u64) {
  let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
  // SAFETY: the buffer is a zeroed rusage_info_v4 matching the flavor.
  let ok = unsafe {
    libc::proc_pid_rusage(
      std::process::id() as i32,
      libc::RUSAGE_INFO_V4,
      &mut info as *mut _ as *mut libc::rusage_info_t,
    )
  };
  if ok != 0 {
    return (0, 0);
  }
  (info.ri_phys_footprint, info.ri_lifetime_max_phys_footprint)
}

#[cfg(not(target_os = "macos"))]
fn footprint() -> (u64, u64) {
  (0, 0)
}

fn relieve() {
  #[cfg(target_os = "macos")]
  {
    extern "C" {
      fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
    }
    // SAFETY: null zone = all zones; no borrowed memory.
    unsafe {
      malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
    }
  }
}

fn report(stage: &str, started: Instant) {
  let (now, peak) = footprint();
  println!(
    "{stage:<28} footprint {:>8.1} MiB   peak {:>8.1} MiB   t+{:.1}s",
    now as f64 / 1048576.0,
    peak as f64 / 1048576.0,
    started.elapsed().as_secs_f64()
  );
}

fn main() {
  let paths: Vec<String> = std::env::args().skip(1).collect();
  let paths = if paths.is_empty() {
    vec!["/".to_string()]
  } else {
    paths
  };
  let started = Instant::now();
  report("baseline", started);
  let mut engine = EngineStore::new(vec![PathBuf::from("/")]);
  let (session, task) = engine
    .prepare_local_scan(ScanStartRequest {
      paths,
      local_only: true,
      ignore_hidden: false,
      respect_gitignore: true,
      ignored_mode: WireIgnoredMode::Summarize,
      label: None,
    })
    .expect("prepare");
  let result = scan_local_directory_with_progress(task.options, task.cancellation, |_| {});
  report("walk finished (report)", started);
  let status = engine
    .finish_local_scan(&session.scan_id, result)
    .expect("finish");
  println!(
    "entries {} bytes {} state {}",
    status.entries_scanned, status.bytes_scanned, status.state
  );
  report("indexed (report dropped)", started);
  let root = status.root_ids[0].clone();
  let slice = engine
    .slice(&TreeSliceRequest {
      scan_id: session.scan_id.clone(),
      node_id: root.clone(),
      depth: 2,
      max_children_per_node: 50,
    })
    .expect("slice");
  let page = engine
    .children(&ChildrenPageRequest {
      scan_id: session.scan_id.clone(),
      node_id: root,
      offset: 0,
      limit: 100,
      sort: "size".into(),
    })
    .expect("children");
  println!(
    "slice children {} page total {}",
    slice.tree.children.len(),
    page.total
  );
  report("slice + children", started);
  for kind in [
    DiscoveryKind::LargeFiles,
    DiscoveryKind::Caches,
    DiscoveryKind::Gitignored,
  ] {
    let request = DiscoveryRequest {
      scan_id: session.scan_id.clone(),
      kind,
      min_size: 0,
      offset: 0,
      limit: 200,
    };
    let prepared = engine.prepare_discovery(&request).expect("discovery");
    report("  prepared", started);
    relieve();
    report("  prepared + relief", started);
    let data = match prepared {
      PreparedDiscovery::Ready(data) => Some(data),
      PreparedDiscovery::Cached => None,
      PreparedDiscovery::Walk(_) => panic!("protected scans never walk"),
    };
    let page = engine.finish_discovery(&request, data).expect("finish");
    println!("{kind:?}: total {} size {}", page.total, page.total_size);
    report("discovery", started);
  }
  if let Ok(seconds) = std::env::var("SCAN_FOOTPRINT_HOLD") {
    // Keep the process alive for `vmmap --summary <pid>` or `footprint`.
    println!("pid {} holding {seconds}s", std::process::id());
    std::thread::sleep(std::time::Duration::from_secs(
      seconds.parse().unwrap_or(60),
    ));
  }
}
