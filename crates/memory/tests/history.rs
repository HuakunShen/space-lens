//! History-ring cases from §6.6 and T6 of docs/memory-cleaner-plan.md: the
//! system ring keeps 3600 points, each tracked group's ring keeps 600, at most
//! 200 groups are tracked, and a query downsamples the system ring to at most
//! 600 points while aligning each requested group series with those points.
//! Everything is synthetic, so no real machine is touched.

use spacelens_memory::group::group_id_for;
use spacelens_memory::history::{
  History, GROUP_RING_CAP, MAX_QUERY_GROUPS, MAX_QUERY_POINTS, MAX_TRACKED_GROUPS, SYSTEM_RING_CAP,
};
use spacelens_memory::{
  GroupId, HistoryRequest, MemoryBreakdown, MemoryPressure, MemorySnapshot, Platform,
  PressureLevel, PressureSource, ProcessGroup, ProcessKind,
};

const MIB: u64 = 1024 * 1024;
const NOW: i64 = 1_700_000_000_000;

fn group(
  key: &str,
  footprint_bytes: u64,
  cpu_percent: Option<f32>,
  frontmost: bool,
) -> ProcessGroup {
  ProcessGroup {
    group_id: group_id_for(key),
    name: key.to_string(),
    bundle_id: None,
    bundle_path: None,
    kind: ProcessKind::App,
    main_pid: 1,
    pids: vec![1],
    cpu_percent,
    footprint_bytes,
    resident_bytes: footprint_bytes,
    frontmost,
    last_frontmost_at: None,
    started_at: "2026-10-18T00:00:00.000Z".to_string(),
    protection: None,
  }
}

fn snapshot(groups: Vec<ProcessGroup>, interval_ms: u32) -> MemorySnapshot {
  MemorySnapshot {
    sequence: 1,
    sampled_at: "2026-10-18T00:00:00.000Z".to_string(),
    interval_ms,
    platform: Platform::Macos,
    breakdown: MemoryBreakdown {
      total_bytes: 16 * 1024 * MIB,
      used_bytes: 8 * 1024 * MIB,
      free_bytes: 4 * 1024 * MIB,
      available_bytes: None,
      app_bytes: None,
      wired_bytes: None,
      compressed_bytes: Some(1024 * MIB),
      cached_files_bytes: None,
      swap_total_bytes: 0,
      swap_used_bytes: 0,
    },
    pressure: MemoryPressure {
      level: PressureLevel::Normal,
      source: PressureSource::Kernel,
      available_percent: Some(70.0),
    },
    groups,
    processes: Vec::new(),
    notes: Vec::new(),
  }
}

fn empty() -> HistoryRequest {
  HistoryRequest {
    group_ids: Vec::new(),
  }
}

#[test]
fn the_system_ring_holds_3600_points_and_the_query_downsamples_to_600() {
  let mut hist = History::new();
  let snap = snapshot(Vec::new(), 1_000);
  for tick in 0..(SYSTEM_RING_CAP as i64 + 400) {
    hist.push(&snap, NOW + tick * 1_000);
  }
  let out = hist.query(&empty());
  assert_eq!(out.points.len(), MAX_QUERY_POINTS);
  // 3600 points over 600 = a six-second step.
  assert_eq!(out.step_ms, 6_000);
}

#[test]
fn at_exactly_the_cap_the_step_is_one() {
  let mut hist = History::new();
  let snap = snapshot(Vec::new(), 1_000);
  for tick in 0..MAX_QUERY_POINTS as i64 {
    hist.push(&snap, NOW + tick * 1_000);
  }
  let out = hist.query(&empty());
  assert_eq!(out.points.len(), MAX_QUERY_POINTS);
  assert_eq!(out.step_ms, 1_000);

  hist.push(&snap, NOW + MAX_QUERY_POINTS as i64 * 1_000);
  let out = hist.query(&empty());
  assert!(out.points.len() <= MAX_QUERY_POINTS);
  assert_eq!(out.step_ms, 2_000);
}

#[test]
fn a_group_ring_keeps_only_the_last_six_hundred_points() {
  let mut hist = History::new();
  for tick in 0..1_000u64 {
    let snap = snapshot(vec![group("app", tick, None, false)], 1_000);
    hist.push(&snap, NOW + tick as i64 * 1_000);
  }
  // 1000 points capped at 600 means the oldest kept tick is 400.
  assert_eq!(hist.footprint_at(&group_id_for("app"), 999_000), Some(400));
  assert_eq!(GROUP_RING_CAP, 600);
}

#[test]
fn at_most_two_hundred_groups_are_tracked() {
  let groups: Vec<ProcessGroup> = (0..MAX_TRACKED_GROUPS + 1)
    .map(|index| {
      let key = format!("g{index}");
      group(&key, 1_000_000 - index as u64, None, false)
    })
    .collect();
  let mut hist = History::new();
  hist.push(&snapshot(groups, 1_000), NOW);

  assert!(hist.footprint_at(&group_id_for("g0"), 0).is_some());
  assert!(hist
    .footprint_at(&group_id_for(&format!("g{MAX_TRACKED_GROUPS}")), 0)
    .is_none());
}

#[test]
fn observed_minutes_track_the_open_window() {
  let mut hist = History::new();
  let snap = snapshot(Vec::new(), 1_000);
  assert_eq!(hist.observed_minutes(), 0);
  hist.push(&snap, NOW);
  assert_eq!(hist.observed_minutes(), 0);
  hist.push(&snap, NOW + 300_000);
  assert_eq!(hist.observed_minutes(), 5);
  hist.push(&snap, NOW + 359_000);
  assert_eq!(hist.observed_minutes(), 5);
}

#[test]
fn mean_cpu_averages_only_the_window() {
  let mut hist = History::new();
  hist.push(
    &snapshot(vec![group("app", MIB, Some(2.0), false)], 1_000),
    NOW,
  );
  hist.push(
    &snapshot(vec![group("app", MIB, Some(4.0), false)], 1_000),
    NOW + 1_000,
  );

  assert_eq!(hist.mean_cpu(&group_id_for("app"), 100_000), Some(3.0));
  assert_eq!(hist.mean_cpu(&group_id_for("app"), 500), Some(4.0));
  assert_eq!(hist.mean_cpu(&group_id_for("absent"), 100_000), None);

  let mut none_hist = History::new();
  none_hist.push(&snapshot(vec![group("app", MIB, None, false)], 1_000), NOW);
  assert_eq!(none_hist.mean_cpu(&group_id_for("app"), 100_000), None);
}

#[test]
fn footprint_at_returns_the_point_closest_to_the_asked_time() {
  let mut hist = History::new();
  hist.push(&snapshot(vec![group("app", 10, None, false)], 1_000), NOW);
  hist.push(
    &snapshot(vec![group("app", 20, None, false)], 1_000),
    NOW + 1_000,
  );

  assert_eq!(hist.footprint_at(&group_id_for("app"), 1_000), Some(10));
  assert_eq!(hist.footprint_at(&group_id_for("app"), 0), Some(20));
  assert_eq!(hist.footprint_at(&group_id_for("absent"), 0), None);
}

#[test]
fn frontmost_observations_are_remembered() {
  let mut hist = History::new();
  let front = snapshot(vec![group("app", MIB, None, true)], 1_000);
  hist.push(&front, NOW + 5_000);
  assert_eq!(
    hist.last_frontmost_ms(&group_id_for("app")),
    Some(NOW + 5_000)
  );

  let quiet = snapshot(vec![group("app", MIB, None, false)], 1_000);
  hist.push(&quiet, NOW + 9_000);
  assert_eq!(
    hist.last_frontmost_ms(&group_id_for("app")),
    Some(NOW + 5_000)
  );
  assert_eq!(hist.last_frontmost_ms(&group_id_for("absent")), None);
}

#[test]
fn a_query_aligns_each_group_series_with_the_system_points() {
  let mut hist = History::new();
  for tick in 0..10u64 {
    let snap = snapshot(vec![group("app", tick, None, false)], 1_000);
    hist.push(&snap, NOW + tick as i64 * 1_000);
  }
  let out = hist.query(&HistoryRequest {
    group_ids: vec![group_id_for("app")],
  });

  assert_eq!(out.points.len(), 10);
  assert_eq!(out.step_ms, 1_000);
  assert_eq!(out.groups.len(), 1);
  assert_eq!(out.groups[0].group_id, group_id_for("app"));
  assert_eq!(out.groups[0].name, "app");
  let expected: Vec<Option<u64>> = (0..10u64).map(Some).collect();
  assert_eq!(out.groups[0].footprint_bytes, expected);
  assert_eq!(out.groups[0].footprint_bytes.len(), out.points.len());
}

#[test]
fn a_query_returns_at_most_eight_series_and_skips_unknown_groups() {
  let mut hist = History::new();
  let groups: Vec<ProcessGroup> = (0..10)
    .map(|index| {
      let key = format!("g{index}");
      group(&key, MIB + index, None, false)
    })
    .collect();
  hist.push(&snapshot(groups, 1_000), NOW);

  let mut ids: Vec<GroupId> = (0..10)
    .map(|index| group_id_for(&format!("g{index}")))
    .collect();
  ids.push(group_id_for("unknown"));
  let out = hist.query(&HistoryRequest { group_ids: ids });

  assert_eq!(out.groups.len(), MAX_QUERY_GROUPS);
  assert!(out
    .groups
    .iter()
    .all(|series| series.group_id != group_id_for("unknown")));
}
