//! The "Free up memory" candidate rules (§7 and T6 of
//! docs/memory-cleaner-plan.md). [`candidates`] turns the latest snapshot plus
//! the [`History`] rings into the short, ranked list of groups whose quitting
//! would plausibly give memory back, each carrying the typed reasons that made
//! it a candidate and the risks the quit would carry.
//!
//! The thresholds are `pub const` here, so changing one is a deliberate edit and
//! always changes a test. Nothing here touches the operating system: the inputs
//! are already-sampled snapshots and rings.

use crate::history::History;
use crate::ids::GroupId;
use crate::model::{
  CandidateReason, CandidatesRequest, FreeUpCandidate, FreeUpCandidates, MemorySnapshot,
  ProcessGroup, ProcessKind, ReasonCode, RiskFlag,
};
use std::collections::HashSet;

/// The floor for the "large" threshold, 256 MiB.
pub const MIN_FLOOR_BYTES: u64 = 256 * 1024 * 1024;
/// The fraction of total memory that raises the floor, one hundredth.
pub const MIN_TOTAL_DIVISOR: u64 = 100;
/// How long a view must be observed before a group can look idle.
pub const IDLE_MINUTES: u32 = 5;
/// The mean CPU below which a group counts as idle, in percent.
pub const IDLE_CPU_PERCENT: f32 = 1.0;
/// The percentage rise that counts as growing.
pub const GROWING_PERCENT: u64 = 30;
/// The smallest rise that counts as growing, 256 MiB.
pub const GROWING_MIN_BYTES: u64 = 256 * 1024 * 1024;
/// The window a growing footprint is measured over, in milliseconds.
pub const GROWING_WINDOW_MS: i64 = 600_000;
/// The window a growing footprint is measured over, in whole minutes.
pub const GROWING_MINUTES: u32 = 10;
/// How many processes a group needs before it is called many-process.
pub const MANY_PROCESSES: usize = 10;
/// The most candidates one response holds.
pub const MAX_CANDIDATES: usize = 8;
/// A group seen frontmost this recently is never suggested.
pub const FRONTMOST_WINDOW_MS: i64 = 600_000;

/// A reason under construction, so each reason's few optional fields are named.
#[derive(Default)]
struct ReasonBuilder {
  bytes: Option<u64>,
  minutes: Option<u32>,
  cpu_percent: Option<f32>,
  count: Option<u32>,
  from_bytes: Option<u64>,
}

impl ReasonBuilder {
  /// Finish the reason with its code.
  fn build(self, code: ReasonCode) -> CandidateReason {
    CandidateReason {
      code,
      bytes: self.bytes,
      minutes: self.minutes,
      cpu_percent: self.cpu_percent,
      count: self.count,
      from_bytes: self.from_bytes,
    }
  }
}

/// Rank the free-up candidates for one snapshot.
///
/// A group is eligible when it is unprotected, is an app, agent or CLI, is not
/// in `req.exclude`, is not frontmost now, and has not been seen frontmost in
/// the last [`FRONTMOST_WINDOW_MS`]. It is a candidate when it is large
/// (footprint at least `max(256 MiB, total / 100)`) **and** is idle, a
/// background agent, or growing. Candidates are sorted by footprint descending
/// and capped at [`MAX_CANDIDATES`]; `potential_release_bytes` sums the kept
/// ones.
pub fn candidates(
  snap: &MemorySnapshot,
  hist: &History,
  req: &CandidatesRequest,
  now_ms: i64,
) -> FreeUpCandidates {
  let exclude: HashSet<&GroupId> = req.exclude.iter().collect();
  let min_bytes = MIN_FLOOR_BYTES.max(snap.breakdown.total_bytes / MIN_TOTAL_DIVISOR);
  let observed_minutes = hist.observed_minutes();
  let observed_ms = i64::from(observed_minutes) * 60_000;
  let observed_start = now_ms - observed_ms;

  let mut list: Vec<FreeUpCandidate> = Vec::new();
  for group in &snap.groups {
    if group.protection.is_some() {
      continue;
    }
    if !eligible_kind(group.kind) {
      continue;
    }
    if exclude.contains(&group.group_id) {
      continue;
    }
    if group.frontmost {
      continue;
    }
    let last_frontmost = hist.last_frontmost_ms(&group.group_id);
    if let Some(at) = last_frontmost {
      if now_ms - at <= FRONTMOST_WINDOW_MS {
        continue;
      }
    }

    let large = group.footprint_bytes >= min_bytes;
    let mean_cpu = hist.mean_cpu(&group.group_id, observed_ms);
    let frontmost_while_observed = last_frontmost
      .map(|at| at >= observed_start)
      .unwrap_or(false);
    let idle = observed_minutes >= IDLE_MINUTES
      && !frontmost_while_observed
      && matches!(mean_cpu, Some(cpu) if cpu < IDLE_CPU_PERCENT);
    let background = group.kind == ProcessKind::Agent && large;
    let from_bytes = if observed_minutes >= GROWING_MINUTES {
      hist.footprint_at(&group.group_id, GROWING_WINDOW_MS)
    } else {
      None
    };
    let growing = is_growing(from_bytes, group.footprint_bytes);

    if !(large && (idle || background || growing)) {
      continue;
    }

    let mut reasons = Vec::new();
    if large {
      reasons.push(
        ReasonBuilder {
          bytes: Some(group.footprint_bytes),
          ..ReasonBuilder::default()
        }
        .build(ReasonCode::Large),
      );
    }
    if idle {
      reasons.push(
        ReasonBuilder {
          minutes: Some(observed_minutes),
          cpu_percent: mean_cpu,
          ..ReasonBuilder::default()
        }
        .build(ReasonCode::Idle),
      );
    }
    if background {
      reasons.push(ReasonBuilder::default().build(ReasonCode::Background));
    }
    if growing {
      reasons.push(
        ReasonBuilder {
          bytes: Some(group.footprint_bytes),
          minutes: Some(GROWING_MINUTES),
          from_bytes,
          ..ReasonBuilder::default()
        }
        .build(ReasonCode::Growing),
      );
    }
    if group.pids.len() >= MANY_PROCESSES {
      reasons.push(
        ReasonBuilder {
          count: Some(group.pids.len() as u32),
          ..ReasonBuilder::default()
        }
        .build(ReasonCode::ManyProcesses),
      );
    }

    list.push(FreeUpCandidate {
      group_id: group.group_id.clone(),
      name: group.name.clone(),
      footprint_bytes: group.footprint_bytes,
      reasons,
      risks: candidate_risks(group),
      preselected: idle && group.kind == ProcessKind::App,
    });
  }

  list.sort_by(|left, right| {
    right
      .footprint_bytes
      .cmp(&left.footprint_bytes)
      .then(left.group_id.as_str().cmp(right.group_id.as_str()))
  });
  list.truncate(MAX_CANDIDATES);
  let potential_release_bytes = list.iter().fold(0u64, |total, candidate| {
    total.saturating_add(candidate.footprint_bytes)
  });

  FreeUpCandidates {
    sequence: snap.sequence,
    pressure: snap.pressure.clone(),
    observed_minutes,
    candidates: list,
    potential_release_bytes,
  }
}

/// Whether a group of this kind may be suggested at all (§7).
fn eligible_kind(kind: ProcessKind) -> bool {
  matches!(
    kind,
    ProcessKind::App | ProcessKind::Agent | ProcessKind::Cli
  )
}

/// Whether a footprint rose by at least [`GROWING_PERCENT`] and
/// [`GROWING_MIN_BYTES`] over the measured window.
fn is_growing(from_bytes: Option<u64>, current: u64) -> bool {
  match from_bytes {
    Some(from) if from > 0 => {
      let rose = current.saturating_sub(from);
      rose >= GROWING_MIN_BYTES && rose.saturating_mul(100) >= from.saturating_mul(GROWING_PERCENT)
    }
    _ => false,
  }
}

/// The risks a graceful quit of this group would carry (S21), so the dialog and
/// the AI summary can show them before the user ticked the box.
fn candidate_risks(group: &ProcessGroup) -> Vec<RiskFlag> {
  let mut risks = Vec::new();
  if matches!(group.kind, ProcessKind::App | ProcessKind::Agent) {
    risks.push(RiskFlag::AppMayPromptToSave);
  }
  if group.pids.len() > 1 {
    risks.push(RiskFlag::EndsChildProcesses);
  }
  if group.kind == ProcessKind::Agent {
    risks.push(RiskFlag::BackgroundAgent);
    risks.push(RiskFlag::Restarts);
  }
  risks
}
