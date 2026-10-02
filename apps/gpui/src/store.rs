//! GPUI entity wrapper around the session layer (plan §4.1/§4.2): the scan
//! index built from a finished scan plus the collector's staged paths.
//! Views observe [`StoreEvent`] and read on demand.

use gpui_kit::*;
use space_lens::ScanNode;
use std::path::PathBuf;
use std::time::Instant;

use crate::session::index::{IndexEntry, ScanIndex, SortMode};
use crate::session::plan::{mtime_ms, stage_entry, EntryFingerprint, StagedPath};

pub enum StoreEvent {
  /// A fresh scan tree was ingested: every focus position and table is
  /// stale and must reset.
  Ingested,
  /// The collector queue changed.
  StagedChanged,
  /// A dry-run plan is ready for confirmation.
  PlanReady,
  /// A background candidates/dirty-git search started or finished; the
  /// status bar renders this as its loading spinner.
  SearchChanged,
}

/// One planned removal: the staged path plus the identity captured at plan
/// time (engine.rs:471-485 semantics — stat'd size, mtime in ms).
#[derive(Clone)]
pub struct PlannedEntry {
  pub path: String,
  pub size: u64,
  pub fingerprint: EntryFingerprint,
}

/// The dry-run plan built from the collector queue (engine.rs:478-497
/// semantics, minus the wire shapes and the preset field a staged queue
/// cannot know).
pub struct PlannedCleanup {
  pub entries: Vec<PlannedEntry>,
  pub total_size: u64,
  created_at: Instant,
}

impl PlannedCleanup {
  /// engine.rs:509-510 — a plan older than 600 seconds must be re-planned.
  pub fn expired(&self, now: Instant) -> bool {
    plan_expired(self.created_at, now)
  }
}

/// Plans live for 600 seconds (engine.rs:509).
const PLAN_TTL_SECS: u64 = 600;

fn plan_expired(created_at: Instant, now: Instant) -> bool {
  now.duration_since(created_at).as_secs() > PLAN_TTL_SECS
}

/// Outcome of the trash-only execution (engine.rs:533-546): one failed entry
/// never stops the rest.
#[derive(Clone, Default)]
pub struct TrashedEntry {
  pub path: String,
  /// Part of the desktop outcome shape (engine.rs:188); the panel reports
  /// totals via `bytes_freed`, so the per-entry size is currently unread.
  #[allow(dead_code)]
  pub size: u64,
}

#[derive(Clone, Default)]
pub struct FailedEntry {
  pub path: String,
  pub message: String,
}

#[derive(Clone, Default)]
pub struct CleanupOutcome {
  pub trashed: Vec<TrashedEntry>,
  pub bytes_freed: u64,
  pub failed: Vec<FailedEntry>,
}

pub struct ScanStore {
  index: Option<ScanIndex>,
  /// The scanned roots, kept for the follow-up engine calls (candidate and
  /// dirty-git discovery operate on the same roots as the scan).
  roots: Vec<PathBuf>,
  staged: Vec<StagedPath>,
  plan: Option<PlannedCleanup>,
  /// Whether any panel's background engine call is in flight; the status
  /// bar's spinner reads this (plan §3.1).
  searching: bool,
}

impl EventEmitter<StoreEvent> for ScanStore {}

impl ScanStore {
  pub fn new() -> Self {
    Self {
      index: None,
      roots: Vec::new(),
      staged: Vec::new(),
      plan: None,
      searching: false,
    }
  }

  /// Flattens the finished scan into the index and clears the collector —
  /// a new scan invalidates every staged path and any pending plan (the
  /// generation rule of plan §4.3 applied to the queue, too).
  pub fn ingest(&mut self, tree: Vec<ScanNode>, cx: &mut Context<Self>) {
    self.roots = tree.iter().map(|root| root.path.clone()).collect();
    self.index = Some(ScanIndex::new(&tree));
    self.staged.clear();
    self.plan = None;
    cx.emit(StoreEvent::Ingested);
  }

  /// The roots of the current scan; empty before the first scan lands.
  pub fn roots(&self) -> &[PathBuf] {
    &self.roots
  }

  pub fn index(&self) -> Option<&ScanIndex> {
    self.index.as_ref()
  }

  /// Root node ids of the current scan, outermost first.
  pub fn root_ids(&self) -> &[String] {
    match &self.index {
      Some(index) => index.root_ids(),
      None => &[],
    }
  }

  /// All direct children of `node_id`, fully sorted — no paging.
  pub fn children(&self, node_id: &str, sort: SortMode) -> Option<Vec<&IndexEntry>> {
    self.index.as_ref()?.children(node_id, sort)
  }

  /// Breadcrumb chain for `node_id`: root-first, excluding the node itself.
  pub fn ancestors(&self, node_id: &str) -> Option<Vec<&IndexEntry>> {
    self.index.as_ref()?.ancestors(node_id)
  }

  /// Stages a path under the collector's ancestor-supersedes-descendant rule
  /// (plan §4.2).
  pub fn stage(&mut self, entry: StagedPath, cx: &mut Context<Self>) {
    stage_entry(&mut self.staged, entry);
    cx.emit(StoreEvent::StagedChanged);
  }

  /// Removes one staged path from the collector queue.
  pub fn unstage(&mut self, node_id: &str, cx: &mut Context<Self>) {
    self.staged.retain(|staged| staged.node_id != node_id);
    cx.emit(StoreEvent::StagedChanged);
  }

  pub fn staged(&self) -> &[StagedPath] {
    &self.staged
  }

  /// Flips the workbench-wide "a background search is running" flag the
  /// status bar renders; set by the candidates and dirty-git panels around
  /// their engine calls.
  pub fn set_searching(&mut self, searching: bool, cx: &mut Context<Self>) {
    if self.searching == searching {
      return;
    }
    self.searching = searching;
    cx.emit(StoreEvent::SearchChanged);
  }

  pub fn searching(&self) -> bool {
    self.searching
  }

  /// Builds the dry-run plan from the staged queue, capturing every
  /// fingerprint now (engine.rs:471-485). A path that no longer stats fails
  /// the whole build. Pure data on the caller's thread — plan §4.2. Returns
  /// the entry count.
  pub fn build_plan(&mut self, cx: &mut Context<Self>) -> Result<usize, String> {
    self.plan = None;
    let mut entries = Vec::with_capacity(self.staged.len());
    let mut total_size = 0u64;
    for staged in &self.staged {
      let metadata =
        std::fs::metadata(&staged.path).map_err(|_| format!("cannot stat {}", staged.path))?;
      // Display size comes from the index (allocated blocks); the
      // fingerprint carries the stat'd size like the desktop engine does.
      let size = self
        .index
        .as_ref()
        .and_then(|index| index.get(&staged.node_id))
        .map(|entry| entry.size)
        .unwrap_or(0);
      entries.push(PlannedEntry {
        path: staged.path.clone(),
        size,
        fingerprint: EntryFingerprint {
          size: metadata.len(),
          mtime_ms: mtime_ms(metadata.modified().ok()),
        },
      });
      total_size += size;
    }
    let count = entries.len();
    self.plan = Some(PlannedCleanup {
      entries,
      total_size,
      created_at: Instant::now(),
    });
    cx.emit(StoreEvent::PlanReady);
    Ok(count)
  }

  pub fn plan(&self) -> Option<&PlannedCleanup> {
    self.plan.as_ref()
  }

  pub fn clear_plan(&mut self) {
    self.plan = None;
  }

  /// Drops every trashed path from the collector queue and the pending plan
  /// (the web workbench filters trashed paths out the same way,
  /// +page.svelte:283-284).
  pub fn remove_trashed(&mut self, trashed: &[TrashedEntry], cx: &mut Context<Self>) {
    let paths: Vec<&str> = trashed.iter().map(|entry| entry.path.as_str()).collect();
    self
      .staged
      .retain(|staged| !paths.contains(&staged.path.as_str()));
    self.plan = None;
    cx.emit(StoreEvent::StagedChanged);
  }
}

#[cfg(test)]
mod tests {
  // Explicit imports, not `use super::*`: the file's top-level
  // `use gpui_kit::*` glob would pull the whole kit prelude into the test
  // module and blow the `#[test]` expansion over the recursion limit.
  use super::{plan_expired, PLAN_TTL_SECS};
  use std::time::{Duration, Instant};

  #[test]
  fn plan_expires_after_ten_minutes() {
    let created = Instant::now();
    assert!(!plan_expired(created, created));
    assert!(!plan_expired(
      created,
      created + Duration::from_secs(PLAN_TTL_SECS)
    ));
    assert!(plan_expired(
      created,
      created + Duration::from_secs(PLAN_TTL_SECS + 1)
    ));
  }
}
