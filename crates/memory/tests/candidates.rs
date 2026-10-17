//! Free-up candidate cases from §7 of docs/memory-cleaner-plan.md. Every reason
//! is checked at its threshold and one step past it, and the frontmost, exclude,
//! preselection, cap and release-estimate rules each get their own case.
//! Snapshots and [`History`] rings are built by hand, so a test controls exactly
//! which reason can fire and no real machine is involved.

use spacelens_memory::candidates::{
  candidates, FRONTMOST_WINDOW_MS, GROWING_PERCENT, IDLE_CPU_PERCENT, IDLE_MINUTES, MANY_PROCESSES,
  MAX_CANDIDATES, MIN_FLOOR_BYTES, MIN_TOTAL_DIVISOR,
};
use spacelens_memory::group::group_id_for;
use spacelens_memory::history::History;
use spacelens_memory::{
  CandidatesRequest, FreeUpCandidate, MemoryBreakdown, MemoryPressure, MemorySnapshot, Platform,
  PressureLevel, PressureSource, ProcessGroup, ProcessKind, ReasonCode,
};

const MIB: u64 = 1024 * 1024;
/// A moment far enough from the Unix epoch to be unambiguous.
const NOW: i64 = 1_700_000_000_000;
/// A total small enough that the 256 MiB floor wins over `total / 100`.
const SMALL_TOTAL: u64 = 16 * 1024 * MIB;
/// A total whose hundredth (1 GiB) is above the floor.
const LARGE_TOTAL: u64 = 100 * 1024 * MIB;

fn group_cpu(
  key: &str,
  kind: ProcessKind,
  footprint_bytes: u64,
  cpu_percent: f32,
  pids: Vec<u32>,
  frontmost: bool,
) -> ProcessGroup {
  let main_pid = pids.first().copied().unwrap_or(0);
  ProcessGroup {
    group_id: group_id_for(key),
    name: key.to_string(),
    bundle_id: None,
    bundle_path: None,
    kind,
    main_pid,
    pids,
    cpu_percent: Some(cpu_percent),
    footprint_bytes,
    resident_bytes: footprint_bytes,
    frontmost,
    last_frontmost_at: None,
    started_at: "2026-10-18T00:00:00.000Z".to_string(),
    protection: None,
  }
}

fn group(
  key: &str,
  kind: ProcessKind,
  footprint_bytes: u64,
  pids: Vec<u32>,
  frontmost: bool,
) -> ProcessGroup {
  group_cpu(key, kind, footprint_bytes, 0.0, pids, frontmost)
}

fn snapshot(groups: Vec<ProcessGroup>, total_bytes: u64) -> MemorySnapshot {
  MemorySnapshot {
    sequence: 7,
    sampled_at: "2026-10-18T00:00:00.000Z".to_string(),
    interval_ms: 1_000,
    platform: Platform::Macos,
    breakdown: MemoryBreakdown {
      total_bytes,
      used_bytes: total_bytes / 2,
      free_bytes: total_bytes / 4,
      available_bytes: Some(total_bytes / 4),
      app_bytes: None,
      wired_bytes: None,
      compressed_bytes: None,
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

fn request() -> CandidatesRequest {
  CandidatesRequest {
    exclude: Vec::new(),
  }
}

/// A history observed for exactly five minutes with the snapshot's own CPU.
fn idle_history(snap: &MemorySnapshot) -> History {
  let mut hist = History::new();
  hist.push(snap, NOW);
  hist.push(snap, NOW + i64::from(IDLE_MINUTES) * 60_000);
  hist
}

fn codes(candidate: &FreeUpCandidate) -> Vec<ReasonCode> {
  candidate.reasons.iter().map(|reason| reason.code).collect()
}

fn run(snap: &MemorySnapshot, hist: &History) -> Vec<FreeUpCandidate> {
  candidates(
    snap,
    hist,
    &request(),
    NOW + i64::from(IDLE_MINUTES) * 60_000,
  )
  .candidates
}

#[test]
fn large_uses_the_floor_and_the_total_fraction() {
  // The floor wins on a small machine: one byte under 256 MiB is not large.
  let below = snapshot(
    vec![group(
      "below",
      ProcessKind::App,
      MIN_FLOOR_BYTES - 1,
      vec![1],
      false,
    )],
    SMALL_TOTAL,
  );
  assert!(run(&below, &idle_history(&below)).is_empty());

  let at_floor = snapshot(
    vec![group(
      "floor",
      ProcessKind::App,
      MIN_FLOOR_BYTES,
      vec![1],
      false,
    )],
    SMALL_TOTAL,
  );
  let kept = run(&at_floor, &idle_history(&at_floor));
  assert_eq!(kept.len(), 1);
  assert!(codes(&kept[0]).contains(&ReasonCode::Large));

  // On a big machine the hundredth wins: a footprint under total / 100 is not
  // large even though it is far above 256 MiB.
  let total_fraction = LARGE_TOTAL / MIN_TOTAL_DIVISOR;
  let below_fraction = snapshot(
    vec![group(
      "below-fraction",
      ProcessKind::App,
      total_fraction - 1,
      vec![1],
      false,
    )],
    LARGE_TOTAL,
  );
  assert!(run(&below_fraction, &idle_history(&below_fraction)).is_empty());

  let at_fraction = snapshot(
    vec![group(
      "fraction",
      ProcessKind::App,
      total_fraction,
      vec![1],
      false,
    )],
    LARGE_TOTAL,
  );
  assert_eq!(run(&at_fraction, &idle_history(&at_fraction)).len(), 1);
}

#[test]
fn idle_fires_below_the_cpu_threshold_and_at_the_minute_threshold() {
  let snap = snapshot(
    vec![group_cpu(
      "idle",
      ProcessKind::App,
      MIN_FLOOR_BYTES,
      IDLE_CPU_PERCENT - 0.01,
      vec![1],
      false,
    )],
    SMALL_TOTAL,
  );
  let kept = run(&snap, &idle_history(&snap));
  assert_eq!(kept.len(), 1);
  assert!(kept[0].preselected);
  let idle = kept[0]
    .reasons
    .iter()
    .find(|reason| reason.code == ReasonCode::Idle)
    .expect("an idle reason");
  assert_eq!(idle.minutes, Some(IDLE_MINUTES));

  // Exactly the threshold is not below it.
  let busy = snapshot(
    vec![group_cpu(
      "busy",
      ProcessKind::App,
      MIN_FLOOR_BYTES,
      IDLE_CPU_PERCENT,
      vec![1],
      false,
    )],
    SMALL_TOTAL,
  );
  assert!(run(&busy, &idle_history(&busy)).is_empty());

  // One minute short of the observation threshold.
  let short = snapshot(
    vec![group_cpu(
      "short",
      ProcessKind::App,
      MIN_FLOOR_BYTES,
      0.0,
      vec![1],
      false,
    )],
    SMALL_TOTAL,
  );
  let mut short_hist = History::new();
  short_hist.push(&short, NOW);
  short_hist.push(&short, NOW + i64::from(IDLE_MINUTES - 1) * 60_000);
  let out = candidates(
    &short,
    &short_hist,
    &request(),
    NOW + i64::from(IDLE_MINUTES - 1) * 60_000,
  );
  assert!(out.candidates.is_empty());
}

#[test]
fn background_needs_an_agent_and_a_large_footprint() {
  // Large alone is not enough: an app with no other reason is never suggested.
  let app = snapshot(
    vec![group_cpu(
      "app",
      ProcessKind::App,
      MIN_FLOOR_BYTES,
      5.0,
      vec![1],
      false,
    )],
    SMALL_TOTAL,
  );
  let mut app_hist = History::new();
  app_hist.push(&app, NOW);
  assert!(candidates(&app, &app_hist, &request(), NOW)
    .candidates
    .is_empty());

  // An agent below the large threshold is not a candidate either.
  let small_agent = snapshot(
    vec![group_cpu(
      "small-agent",
      ProcessKind::Agent,
      MIN_FLOOR_BYTES - 1,
      5.0,
      vec![1],
      false,
    )],
    SMALL_TOTAL,
  );
  let mut small_hist = History::new();
  small_hist.push(&small_agent, NOW);
  assert!(candidates(&small_agent, &small_hist, &request(), NOW)
    .candidates
    .is_empty());

  let agent = snapshot(
    vec![group_cpu(
      "agent",
      ProcessKind::Agent,
      MIN_FLOOR_BYTES,
      5.0,
      vec![1],
      false,
    )],
    SMALL_TOTAL,
  );
  let mut agent_hist = History::new();
  agent_hist.push(&agent, NOW);
  let out = candidates(&agent, &agent_hist, &request(), NOW).candidates;
  assert_eq!(out.len(), 1);
  assert!(codes(&out[0]).contains(&ReasonCode::Background));
  // Agents are listed, never ticked.
  assert!(!out[0].preselected);
}

#[test]
fn growing_needs_ten_minutes_and_both_thresholds() {
  // A start where thirty percent is also above 256 MiB.
  let start = 1024 * MIB;
  let thirty_percent = (start * GROWING_PERCENT).div_ceil(100);

  // The smallest rise that is still "thirty percent up": fires.
  let before = snapshot(
    vec![group_cpu(
      "grow",
      ProcessKind::App,
      start,
      5.0,
      vec![1],
      false,
    )],
    SMALL_TOTAL,
  );
  let after = snapshot(
    vec![group_cpu(
      "grow",
      ProcessKind::App,
      start + thirty_percent,
      5.0,
      vec![1],
      false,
    )],
    SMALL_TOTAL,
  );
  let mut hist = History::new();
  hist.push(&before, NOW);
  hist.push(&after, NOW + 600_000);
  let out = candidates(&after, &hist, &request(), NOW + 600_000).candidates;
  assert_eq!(out.len(), 1);
  let growing = out[0]
    .reasons
    .iter()
    .find(|reason| reason.code == ReasonCode::Growing)
    .expect("a growing reason");
  assert_eq!(growing.from_bytes, Some(start));
  assert_eq!(growing.bytes, Some(start + thirty_percent));
  assert_eq!(growing.minutes, Some(10));
  // Growing-only candidates are listed, not ticked.
  assert!(!out[0].preselected);

  // One byte under thirty percent: silent.
  let nearly = snapshot(
    vec![group_cpu(
      "grow",
      ProcessKind::App,
      start + thirty_percent - 1,
      5.0,
      vec![1],
      false,
    )],
    SMALL_TOTAL,
  );
  let mut nearly_hist = History::new();
  nearly_hist.push(&before, NOW);
  nearly_hist.push(&nearly, NOW + 600_000);
  assert!(candidates(&nearly, &nearly_hist, &request(), NOW + 600_000)
    .candidates
    .is_empty());

  // A rise of more than thirty percent but under 256 MiB is still silent.
  let tiny_start = 100 * MIB;
  let tiny_before = snapshot(
    vec![group_cpu(
      "tiny",
      ProcessKind::App,
      tiny_start,
      5.0,
      vec![1],
      false,
    )],
    SMALL_TOTAL,
  );
  let tiny_after = snapshot(
    vec![group_cpu(
      "tiny",
      ProcessKind::App,
      tiny_start + 200 * MIB,
      5.0,
      vec![1],
      false,
    )],
    SMALL_TOTAL,
  );
  let mut tiny_hist = History::new();
  tiny_hist.push(&tiny_before, NOW);
  tiny_hist.push(&tiny_after, NOW + 600_000);
  assert!(
    candidates(&tiny_after, &tiny_hist, &request(), NOW + 600_000)
      .candidates
      .is_empty()
  );

  // Growing needs the full ten minutes of history.
  let mut short_hist = History::new();
  short_hist.push(&before, NOW);
  short_hist.push(&after, NOW + 599_000);
  assert!(candidates(&after, &short_hist, &request(), NOW + 599_000)
    .candidates
    .is_empty());
}

#[test]
fn many_processes_needs_ten_members() {
  let pids: Vec<u32> = (1..=MANY_PROCESSES as u32).collect();
  let many = snapshot(
    vec![group(
      "many",
      ProcessKind::App,
      MIN_FLOOR_BYTES,
      pids,
      false,
    )],
    SMALL_TOTAL,
  );
  let out = run(&many, &idle_history(&many));
  assert_eq!(out.len(), 1);
  assert!(codes(&out[0]).contains(&ReasonCode::ManyProcesses));

  let few_pids: Vec<u32> = (1..MANY_PROCESSES as u32).collect();
  let few = snapshot(
    vec![group(
      "few",
      ProcessKind::App,
      MIN_FLOOR_BYTES,
      few_pids,
      false,
    )],
    SMALL_TOTAL,
  );
  let out = run(&few, &idle_history(&few));
  assert_eq!(out.len(), 1);
  assert!(!codes(&out[0]).contains(&ReasonCode::ManyProcesses));
}

#[test]
fn the_frontmost_group_is_never_suggested() {
  // Frontmost right now.
  let now = snapshot(
    vec![group(
      "front",
      ProcessKind::App,
      MIN_FLOOR_BYTES,
      vec![1],
      true,
    )],
    SMALL_TOTAL,
  );
  assert!(run(&now, &idle_history(&now)).is_empty());

  // Frontmost within the observation window, then brought to the back.
  let was_front = snapshot(
    vec![group(
      "front",
      ProcessKind::App,
      MIN_FLOOR_BYTES,
      vec![1],
      true,
    )],
    SMALL_TOTAL,
  );
  let quiet = snapshot(
    vec![group(
      "front",
      ProcessKind::App,
      MIN_FLOOR_BYTES,
      vec![1],
      false,
    )],
    SMALL_TOTAL,
  );
  let mut hist = History::new();
  hist.push(&was_front, NOW);
  hist.push(&quiet, NOW + FRONTMOST_WINDOW_MS);
  let seen_at = NOW + FRONTMOST_WINDOW_MS;
  assert!(
    candidates(&quiet, &hist, &request(), seen_at)
      .candidates
      .is_empty(),
    "seen frontmost exactly ten minutes ago is still too recent"
  );

  let mut later = History::new();
  later.push(&was_front, NOW);
  later.push(&quiet, NOW + FRONTMOST_WINDOW_MS + 1);
  let out = candidates(&quiet, &later, &request(), NOW + FRONTMOST_WINDOW_MS + 1).candidates;
  assert_eq!(out.len(), 1, "one millisecond later it may be suggested");
}

#[test]
fn the_exclude_list_is_honoured() {
  let snap = snapshot(
    vec![group(
      "keep",
      ProcessKind::App,
      MIN_FLOOR_BYTES,
      vec![1],
      false,
    )],
    SMALL_TOTAL,
  );
  let hist = idle_history(&snap);
  let out = candidates(
    &snap,
    &hist,
    &CandidatesRequest {
      exclude: vec![group_id_for("keep")],
    },
    NOW + i64::from(IDLE_MINUTES) * 60_000,
  );
  assert!(out.candidates.is_empty());
}

#[test]
fn preselection_is_idle_apps_only() {
  let app = snapshot(
    vec![group(
      "idle-app",
      ProcessKind::App,
      MIN_FLOOR_BYTES,
      vec![1],
      false,
    )],
    SMALL_TOTAL,
  );
  assert!(run(&app, &idle_history(&app))[0].preselected);

  let agent = snapshot(
    vec![group(
      "idle-agent",
      ProcessKind::Agent,
      MIN_FLOOR_BYTES,
      vec![1],
      false,
    )],
    SMALL_TOTAL,
  );
  let agents = run(&agent, &idle_history(&agent));
  assert_eq!(agents.len(), 1);
  assert!(!agents[0].preselected);
}

#[test]
fn at_most_eight_candidates_are_returned_and_the_release_is_summed() {
  let groups: Vec<ProcessGroup> = (0..12u32)
    .map(|index| {
      group(
        &format!("group-{index}"),
        ProcessKind::App,
        MIN_FLOOR_BYTES + u64::from(12 - index) * MIB,
        vec![index + 1],
        false,
      )
    })
    .collect();
  let snap = snapshot(groups, SMALL_TOTAL);
  let hist = idle_history(&snap);
  let out = candidates(
    &snap,
    &hist,
    &request(),
    NOW + i64::from(IDLE_MINUTES) * 60_000,
  );

  assert_eq!(out.candidates.len(), MAX_CANDIDATES);
  assert_eq!(out.sequence, snap.sequence);
  assert_eq!(out.pressure.level, PressureLevel::Normal);
  assert_eq!(out.observed_minutes, IDLE_MINUTES);
  for pair in out.candidates.windows(2) {
    assert!(pair[0].footprint_bytes >= pair[1].footprint_bytes);
  }
  let expected: u64 = out.candidates.iter().map(|c| c.footprint_bytes).sum();
  assert_eq!(out.potential_release_bytes, expected);
}

#[test]
fn a_protected_group_is_never_suggested() {
  let mut protected = group(
    "protected",
    ProcessKind::App,
    MIN_FLOOR_BYTES,
    vec![1],
    false,
  );
  protected.protection = Some(spacelens_memory::Protection::SelfOrAncestor);
  let snap = snapshot(vec![protected], SMALL_TOTAL);
  assert!(run(&snap, &idle_history(&snap)).is_empty());
}

#[test]
fn groups_outside_the_eligible_kinds_are_ignored() {
  for kind in [ProcessKind::Helper, ProcessKind::System, ProcessKind::Other] {
    let snap = snapshot(
      vec![group("ignored", kind, MIN_FLOOR_BYTES, vec![1], false)],
      SMALL_TOTAL,
    );
    assert!(run(&snap, &idle_history(&snap)).is_empty(), "{kind:?}");
  }
}
