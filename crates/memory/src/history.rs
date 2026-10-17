//! Bounded history rings and the downsampled query behind them (§6.6 and T6 of
//! docs/memory-cleaner-plan.md). [`History`] is fed one [`MemorySnapshot`] per
//! sampler tick and keeps three things: a system ring of at most
//! [`SYSTEM_RING_CAP`] points, a footprint/CPU ring of at most
//! [`GROUP_RING_CAP`] points for each of at most [`MAX_TRACKED_GROUPS`] groups
//! (the largest by footprint in each sample), and the last time each group was
//! seen frontmost.
//!
//! [`History::query`] downsamples the system ring to at most
//! [`MAX_QUERY_POINTS`] points (`step_ms` states the spacing in milliseconds)
//! and aligns each requested group series with those points, filling gaps with
//! `null`. Nothing here touches the operating system.

use crate::ids::GroupId;
use crate::model::{
  GroupSeries, HistoryRequest, MemoryHistory, MemorySnapshot, PressureLevel, ProcessGroup,
  SystemPoint,
};
use std::collections::{HashMap, HashSet, VecDeque};

/// How many system points the ring keeps: one hour at a one-second interval.
pub const SYSTEM_RING_CAP: usize = 3600;
/// How many points each tracked group's ring keeps: ten minutes at one second.
pub const GROUP_RING_CAP: usize = 600;
/// How many groups may be tracked at once.
pub const MAX_TRACKED_GROUPS: usize = 200;
/// The most points one query returns.
pub const MAX_QUERY_POINTS: usize = 600;
/// The most group series one query returns.
pub const MAX_QUERY_GROUPS: usize = 8;

/// One stored system point plus the wall-clock instant it was pushed at.
#[derive(Debug, Clone)]
struct SystemSample {
  ms: i64,
  at: String,
  used_bytes: u64,
  compressed_bytes: Option<u64>,
  swap_used_bytes: u64,
  pressure: PressureLevel,
}

impl SystemSample {
  /// The wire point, dropping the millisecond instant used for alignment.
  fn to_point(&self) -> SystemPoint {
    SystemPoint {
      at: self.at.clone(),
      used_bytes: self.used_bytes,
      compressed_bytes: self.compressed_bytes,
      swap_used_bytes: self.swap_used_bytes,
      pressure: self.pressure,
    }
  }
}

/// One stored group point.
#[derive(Debug, Clone, Copy)]
struct GroupPoint {
  ms: i64,
  footprint_bytes: Option<u64>,
  cpu_percent: Option<f32>,
}

/// The ring of one tracked group plus its most recent name.
#[derive(Debug)]
struct GroupHistory {
  name: String,
  points: VecDeque<GroupPoint>,
}

/// The bounded system, per-group and frontmost history.
#[derive(Debug, Default)]
pub struct History {
  system: VecDeque<SystemSample>,
  groups: HashMap<GroupId, GroupHistory>,
  last_frontmost: HashMap<GroupId, i64>,
  opened_ms: Option<i64>,
  last_ms: Option<i64>,
  interval_ms: u32,
}

impl History {
  /// An empty history.
  pub fn new() -> Self {
    History::default()
  }

  /// Record one snapshot at `now_ms`.
  ///
  /// The system ring is capped at [`SYSTEM_RING_CAP`]; the group rings are kept
  /// for the [`MAX_TRACKED_GROUPS`] largest groups in this snapshot (ties broken
  /// by group id), each capped at [`GROUP_RING_CAP`]; a ring whose group is no
  /// longer among the tracked set is dropped. Frontmost observations are
  /// recorded for every group, tracked or not.
  pub fn push(&mut self, snap: &MemorySnapshot, now_ms: i64) {
    if self.opened_ms.is_none() {
      self.opened_ms = Some(now_ms);
    }
    self.last_ms = Some(now_ms);
    self.interval_ms = snap.interval_ms;

    self.system.push_back(SystemSample {
      ms: now_ms,
      at: snap.sampled_at.clone(),
      used_bytes: snap.breakdown.used_bytes,
      compressed_bytes: snap.breakdown.compressed_bytes,
      swap_used_bytes: snap.breakdown.swap_used_bytes,
      pressure: snap.pressure.level,
    });
    while self.system.len() > SYSTEM_RING_CAP {
      self.system.pop_front();
    }

    let mut ranked: Vec<&ProcessGroup> = snap.groups.iter().collect();
    ranked.sort_by(|left, right| {
      right
        .footprint_bytes
        .cmp(&left.footprint_bytes)
        .then(left.group_id.as_str().cmp(right.group_id.as_str()))
    });
    let tracked: Vec<&ProcessGroup> = ranked.into_iter().take(MAX_TRACKED_GROUPS).collect();
    let kept: HashSet<&GroupId> = tracked.iter().map(|group| &group.group_id).collect();
    self.groups.retain(|id, _| kept.contains(id));
    for group in &tracked {
      let entry = self
        .groups
        .entry(group.group_id.clone())
        .or_insert_with(|| GroupHistory {
          name: group.name.clone(),
          points: VecDeque::new(),
        });
      entry.name = group.name.clone();
      entry.points.push_back(GroupPoint {
        ms: now_ms,
        footprint_bytes: Some(group.footprint_bytes),
        cpu_percent: group.cpu_percent,
      });
      while entry.points.len() > GROUP_RING_CAP {
        entry.points.pop_front();
      }
    }

    for group in &snap.groups {
      if group.frontmost {
        self.last_frontmost.insert(group.group_id.clone(), now_ms);
      }
    }
  }

  /// The downsampled system history and the requested group series.
  ///
  /// At most [`MAX_QUERY_POINTS`] system points are returned, newest point
  /// included, with [`MemoryHistory::step_ms`] set to the spacing between them.
  /// Each requested group (at most [`MAX_QUERY_GROUPS`]) produces one series the
  /// same length as `points`, `null` where the group was not observed then;
  /// unknown groups are skipped.
  pub fn query(&self, req: &HistoryRequest) -> MemoryHistory {
    let len = self.system.len();
    let step = len.div_ceil(MAX_QUERY_POINTS).max(1);
    let mut indices: Vec<usize> = (0..len).rev().step_by(step).collect();
    indices.reverse();

    let points: Vec<SystemPoint> = indices
      .iter()
      .map(|index| self.system[*index].to_point())
      .collect();
    let times: Vec<i64> = indices.iter().map(|index| self.system[*index].ms).collect();

    let mut groups = Vec::new();
    for id in req.group_ids.iter().take(MAX_QUERY_GROUPS) {
      let Some(history) = self.groups.get(id) else {
        continue;
      };
      let by_ms: HashMap<i64, Option<u64>> = history
        .points
        .iter()
        .map(|point| (point.ms, point.footprint_bytes))
        .collect();
      let footprint_bytes = times
        .iter()
        .map(|ms| by_ms.get(ms).copied().flatten())
        .collect();
      groups.push(GroupSeries {
        group_id: id.clone(),
        name: history.name.clone(),
        footprint_bytes,
      });
    }

    MemoryHistory {
      step_ms: self.interval_ms.saturating_mul(step as u32),
      points,
      groups,
    }
  }

  /// How long the view has been observed, in whole minutes.
  pub fn observed_minutes(&self) -> u32 {
    match (self.opened_ms, self.last_ms) {
      (Some(opened), Some(last)) if last > opened => ((last - opened) / 60_000) as u32,
      _ => 0,
    }
  }

  /// The mean CPU of one group over the last `window_ms`, if any sample exists.
  ///
  /// Only samples that carry a CPU reading count; a group with no readable CPU
  /// in the window is unobserved and yields `None`.
  pub fn mean_cpu(&self, g: &GroupId, window_ms: i64) -> Option<f32> {
    let history = self.groups.get(g)?;
    let last = self.last_ms?;
    let cutoff = last - window_ms;
    let mut sum = 0.0f32;
    let mut count: u32 = 0;
    for point in &history.points {
      if point.ms < cutoff {
        continue;
      }
      if let Some(cpu) = point.cpu_percent {
        sum += cpu;
        count += 1;
      }
    }
    if count == 0 {
      None
    } else {
      Some(sum / count as f32)
    }
  }

  /// The footprint of one group closest to `ago_ms` before now, if observed.
  pub fn footprint_at(&self, g: &GroupId, ago_ms: i64) -> Option<u64> {
    let history = self.groups.get(g)?;
    let last = self.last_ms?;
    let target = last - ago_ms;
    let point = history
      .points
      .iter()
      .min_by_key(|point| (point.ms - target).abs())?;
    point.footprint_bytes
  }

  /// When one group was last observed frontmost, if ever.
  pub fn last_frontmost_ms(&self, g: &GroupId) -> Option<i64> {
    self.last_frontmost.get(g).copied()
  }
}
