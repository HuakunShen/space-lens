//! Native workbench: explorer, inspector tabs, and a collapsible collector.
//! The discovery panels run their engine calls on background tasks
//! with the generation scheme of plan §4.3: every message carries the
//! generation that started it, and both `recv` arms reset the panel's UI
//! state.

use std::cmp::Reverse;
use std::path::Path;
use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::component::progress::ProgressCircle;
use gpui_kit::component::table::{Column, ColumnSort, DataTable, TableDelegate, TableState};
// `Icon`/`IconName` are glob-re-exported from the component root (the `icon`
// module itself is private).
use gpui_kit::component::{
  h_flex, v_flex, ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _,
};
use gpui_kit::*;
use smol::channel;
use space_lens::{
  find_candidates, find_dirty_git_repos, CandidateOptions, CleanupCandidate, CleanupPreset,
  DirtyGitRepo, DirtyGitRepoOptions,
};

use crate::session::format::{display_parent, format_bytes, format_count};
use crate::session::index::node_id_of;
use crate::session::plan::{mtime_ms, verify_fingerprints, EntryFingerprint, StagedPath};
use crate::store::{
  CleanupOutcome, FailedEntry, PlannedCleanup, ScanStore, StoreEvent, TrashedEntry,
};
use crate::views::explorer::ExplorerView;
use crate::views::presentation::{InspectorTab, WorkbenchPresentation};
use crate::views::theme::{inspector, segment_selected, segment_track, toolbar};

/// Messages from the background candidate search, tagged with the generation
/// that started it (plan §4.3).
enum CandidatesMsg {
  Done(u64, Vec<CleanupCandidate>),
  Failed(u64, String),
}

#[derive(Clone)]
struct CandidateRow {
  node_id: String,
  path: String,
  preset: &'static str,
  reason: String,
  size: u64,
}

struct CandidatesDelegate {
  rows: Vec<CandidateRow>,
  columns: Vec<Column>,
  scan_store: Entity<ScanStore>,
}

impl CandidatesDelegate {
  fn new(scan_store: Entity<ScanStore>) -> Self {
    Self {
      rows: Vec::new(),
      columns: vec![
        Column::new("path", "Path").width(px(420.)),
        Column::new("preset", "Preset").width(px(90.)),
        Column::new("size", "Size").width(px(100.)).sortable(),
        Column::new("reason", "Reason").width(px(220.)),
        Column::new("collect", "").width(px(80.)),
      ],
      scan_store,
    }
  }
}

impl TableDelegate for CandidatesDelegate {
  fn columns_count(&self, _: &App) -> usize {
    self.columns.len()
  }

  fn rows_count(&self, _: &App) -> usize {
    self.rows.len()
  }

  fn column(&self, col_ix: usize, _: &App) -> Column {
    self.columns[col_ix].clone()
  }

  fn render_td(
    &mut self,
    row_ix: usize,
    col_ix: usize,
    _: &mut Window,
    _: &mut Context<TableState<Self>>,
  ) -> impl IntoElement {
    let row = &self.rows[row_ix];
    match self.columns[col_ix].key.as_ref() {
      "path" => row.path.clone().into_any_element(),
      "preset" => row.preset.into_any_element(),
      "size" => format_bytes(row.size).into_any_element(),
      "reason" => row.reason.clone().into_any_element(),
      "collect" => {
        // Staging straight from the cell: the store deduplicates and the
        // collector panel reacts to the store event.
        let scan_store = self.scan_store.clone();
        let entry = StagedPath {
          node_id: row.node_id.clone(),
          path: row.path.clone(),
        };
        Button::new(format!("collect-{}", row.node_id))
          .ghost()
          .xsmall()
          .label("Collect")
          .on_click(move |_, _, cx| {
            scan_store.update(cx, |store, cx| store.stage(entry.clone(), cx));
          })
          .into_any_element()
      }
      _ => String::new().into_any_element(),
    }
  }

  fn perform_sort(
    &mut self,
    col_ix: usize,
    sort: ColumnSort,
    _: &mut Window,
    _: &mut Context<TableState<Self>>,
  ) {
    match (self.columns[col_ix].key.as_ref(), sort) {
      ("path", ColumnSort::Ascending) => self.rows.sort_by(|a, b| a.path.cmp(&b.path)),
      ("path", ColumnSort::Descending) => self.rows.sort_by(|a, b| b.path.cmp(&a.path)),
      ("size", ColumnSort::Ascending) => self.rows.sort_by_key(|row| row.size),
      ("size", ColumnSort::Descending) => self.rows.sort_by_key(|row| Reverse(row.size)),
      _ => {}
    }
  }
}

/// Kebab-case label for a preset, matching its serde wire name.
fn preset_label(preset: CleanupPreset) -> &'static str {
  match preset {
    CleanupPreset::Node => "node",
    CleanupPreset::Rust => "rust",
    CleanupPreset::Gitignored => "gitignored",
  }
}

/// Right dock tab: cleanup candidates. Preset toggles re-run the engine's
/// `find_candidates` on background tasks under the generation scheme.
pub struct CandidatesView {
  scan_store: Entity<ScanStore>,
  table: Entity<TableState<CandidatesDelegate>>,
  /// Currently toggled presets; Node and Rust default on, the gitignored
  /// preset starts off because it re-scans every root twice.
  presets: Vec<CleanupPreset>,
  scanning: bool,
  generation: u64,
  error: Option<String>,
  focus_handle: FocusHandle,
  _scan_task: Option<Task<()>>,
  _subscriptions: Vec<Subscription>,
}

impl CandidatesView {
  pub fn new(scan_store: Entity<ScanStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
    let table = cx.new(|cx| {
      TableState::new(CandidatesDelegate::new(scan_store.clone()), window, cx).sortable(true)
    });
    let subscription = cx.subscribe(&scan_store, |this, _, event, cx| {
      if matches!(event, StoreEvent::Ingested) {
        // The old scan's candidates are stale; re-run for the new roots.
        this
          .table
          .update(cx, |state, _| state.delegate_mut().rows = Vec::new());
        this.start_search(cx);
      }
    });
    Self {
      scan_store,
      table,
      presets: vec![CleanupPreset::Node, CleanupPreset::Rust],
      scanning: false,
      generation: 0,
      error: None,
      focus_handle: cx.focus_handle(),
      _scan_task: None,
      _subscriptions: vec![subscription],
    }
  }

  fn start_search(&mut self, cx: &mut Context<Self>) {
    // A request during a running search supersedes it: the generation bump
    // below voids the old result when it lands (plan §4.3) — toggling a
    // preset mid-search must not be silently swallowed.
    self.scanning = false;
    self
      .scan_store
      .update(cx, |store, cx| store.set_searching(false, cx));
    let roots = self.scan_store.read(cx).roots().to_vec();
    if roots.is_empty() {
      return; // nothing scanned yet; the panel stays in its empty state
    }
    if self.presets.is_empty() {
      return; // the engine reads empty presets as "all of them" (see toggle_preset)
    }
    self.generation += 1;
    let generation = self.generation;
    let options = CandidateOptions {
      roots,
      presets: self.presets.clone(),
      // Engine defaults (plan §7 risk 7); no picker options here.
      ignore_hidden: false,
      follow_symlinks: false,
    };
    self.scanning = true;
    self
      .scan_store
      .update(cx, |store, cx| store.set_searching(true, cx));
    self.error = None;

    let (tx, rx) = channel::bounded::<CandidatesMsg>(1);
    cx.background_spawn(async move {
      let candidates = find_candidates(options);
      // A send error means the receiving view is gone; drop the result.
      let _ = tx.send(CandidatesMsg::Done(generation, candidates)).await;
    })
    .detach();

    self._scan_task = Some(cx.spawn(async move |this, cx| {
      // Both recv arms reset the UI: a panicked background task closes the
      // channel and would leave `scanning` stuck forever if swallowed
      // (plan §4.3).
      let outcome = match rx.recv().await {
        Ok(msg) => msg,
        Err(_) => {
          CandidatesMsg::Failed(generation, "the background candidates task aborted".into())
        }
      };
      this
        .update(cx, |this, cx| {
          match outcome {
            CandidatesMsg::Done(gen, candidates) if gen == this.generation => {
              this.scanning = false;
              this
                .scan_store
                .update(cx, |store, cx| store.set_searching(false, cx));
              this.error = None;
              let rows = candidates
                .iter()
                .map(|candidate| CandidateRow {
                  node_id: node_id_of(&candidate.path.to_string_lossy()),
                  path: candidate.path.to_string_lossy().to_string(),
                  preset: preset_label(candidate.preset),
                  reason: candidate.reason.clone(),
                  size: candidate.size,
                })
                .collect();
              this
                .table
                .update(cx, |state, _| state.delegate_mut().rows = rows);
            }
            CandidatesMsg::Failed(gen, message) if gen == this.generation => {
              // Retryable: the toggles re-run the search.
              this.scanning = false;
              this
                .scan_store
                .update(cx, |store, cx| store.set_searching(false, cx));
              this.error = Some(message);
            }
            // Stale generation — a newer search owns the panel.
            _ => {}
          }
          cx.notify(); // every path notifies; the panel can never wedge
        })
        .ok(); // the view was closed; nothing left to reset
    }));

    cx.notify();
  }

  fn toggle_preset(&mut self, preset: CleanupPreset, cx: &mut Context<Self>) {
    if let Some(position) = self.presets.iter().position(|toggled| *toggled == preset) {
      self.presets.remove(position);
    } else {
      self.presets.push(preset);
    }
    if self.presets.is_empty() {
      // With nothing toggled, run nothing: the engine would read an empty
      // preset list as "all presets" (clean.rs:60-68), the exact opposite
      // of what the toggles show. Void a running search too, or its stale
      // result would refill the cleared table.
      self.generation += 1;
      self.scanning = false;
      self
        .scan_store
        .update(cx, |store, cx| store.set_searching(false, cx));
      self._scan_task = None;
      self
        .table
        .update(cx, |state, _| state.delegate_mut().rows = Vec::new());
      cx.notify();
      return;
    }
    self.start_search(cx);
  }

  fn render_preset_toggles(&self, cx: &Context<Self>) -> Div {
    h_flex().flex_wrap().gap(px(12.)).children(
      [
        (CleanupPreset::Node, "Node"),
        (CleanupPreset::Rust, "Rust"),
        (CleanupPreset::Gitignored, "Ignored"),
      ]
      .map(|(preset, label)| {
        Checkbox::new(format!("preset-{}", preset_label(preset)))
          .small()
          .label(label)
          .checked(self.presets.contains(&preset))
          .on_click(cx.listener(move |this, _, _, cx| this.toggle_preset(preset, cx)))
      }),
    )
  }

  fn render_status(&self, cx: &Context<Self>) -> Div {
    if self.scanning {
      return h_flex()
        .gap_2()
        .child(ProgressCircle::new("candidates-progress").loading(true))
        .child(
          div()
            .text_size(px(11.))
            .text_color(cx.theme().colors.muted_foreground)
            .child("Searching candidates…"),
        );
    }
    let count = self.table.read(cx).delegate().rows.len();
    h_flex().gap_2().child(
      div()
        .text_sm()
        .text_color(cx.theme().colors.muted_foreground)
        .child(format!("{} candidate(s)", format_count(count))),
    )
  }
}

impl Focusable for CandidatesView {
  fn focus_handle(&self, _: &App) -> FocusHandle {
    self.focus_handle.clone()
  }
}

impl EventEmitter<PanelEvent> for CandidatesView {}

impl BasePanel for CandidatesView {
  fn panel_name(&self) -> &'static str {
    "candidates"
  }
}

impl Panel for CandidatesView {}

impl Render for CandidatesView {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let rows = self.table.read(cx).delegate().rows.clone();
    let muted = cx.theme().muted_foreground;
    let store = self.scan_store.clone();
    let list = v_flex()
      .id("cleanup-candidates")
      .flex_1()
      .min_h_0()
      .overflow_y_scroll()
      .children(rows.iter().enumerate().map(|(ix, row)| {
        let path = Path::new(&row.path);
        let name = path
          .file_name()
          .unwrap_or(path.as_os_str())
          .to_string_lossy()
          .to_string();
        let parent = display_parent(path, self.scan_store.read(cx).roots());
        let entry = StagedPath {
          node_id: row.node_id.clone(),
          path: row.path.clone(),
        };
        let store = store.clone();
        h_flex()
          .id(ix)
          .gap(px(8.))
          .px(px(6.))
          .py(px(12.))
          .w_full()
          .border_b_1()
          .border_color(cx.theme().border)
          .child(
            v_flex()
              .flex_1()
              .min_w_0()
              .gap(px(3.))
              .child(div().truncate().text_size(px(13.)).child(name))
              .child(
                div()
                  .truncate()
                  .text_size(px(11.))
                  .text_color(muted)
                  .child(parent),
              )
              .child(div().text_size(px(11.)).text_color(muted).child(format!(
                "{} · {}",
                row.preset,
                format_bytes(row.size)
              ))),
          )
          .child(
            Button::new(format!("collect-candidate-{ix}"))
              .ghost()
              .xsmall()
              .accessibility_label(format!("Collect {}", row.path))
              .tooltip(format!("{}\n{}", row.path, row.reason))
              .icon(Icon::new(IconName::Plus).size(px(12.)))
              .on_click(move |_, _, cx| {
                store.update(cx, |store, cx| store.stage(entry.clone(), cx))
              }),
          )
      }));
    v_flex()
      .size_full()
      .gap(px(12.))
      .child(self.render_preset_toggles(cx))
      .children(self.presets.contains(&CleanupPreset::Gitignored).then(|| {
        div()
          .text_size(px(11.))
          .text_color(muted)
          .child("Ignored entries take longer to discover.")
      }))
      .children(self.error.clone().map(|message| {
        div()
          .text_size(px(12.))
          .text_color(cx.theme().danger)
          .child(message)
      }))
      .child(list)
      .child(self.render_status(cx))
  }
}

/// Phases of the cleanup flow on this panel (plan §5): plan → confirm →
/// execute → report. The confirm phase renders the hard gate in place —
/// the danger button is the only way past it.
enum CleanupFlow {
  Idle,
  Confirming,
  Executing,
  Reported(Box<CleanupOutcome>),
}

/// Messages from the background trash execution, tagged with the generation
/// that started it (plan §4.3).
enum ExecuteMsg {
  Done(u64, CleanupOutcome),
  Failed(u64, String),
}

struct CleanupFinished;

impl EventEmitter<CleanupFinished> for CollectorView {}

/// Bottom dock: the collector queue. Sizes resolve through the scan index;
/// the cleanup flow is plan → confirm → trash-only execute (plan §5).
pub struct CollectorView {
  scan_store: Entity<ScanStore>,
  table: Entity<TableState<CollectorDelegate>>,
  flow: CleanupFlow,
  generation: u64,
  error: Option<String>,
  focus_handle: FocusHandle,
  _execute_task: Option<Task<()>>,
  _subscriptions: Vec<Subscription>,
}

impl CollectorView {
  pub fn new(scan_store: Entity<ScanStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
    let table =
      cx.new(|cx| TableState::new(CollectorDelegate::new(scan_store.clone()), window, cx));
    let subscription = cx.subscribe(&scan_store, |this, _, event, cx| {
      if matches!(event, StoreEvent::StagedChanged | StoreEvent::Ingested) {
        this.rebuild_rows(cx);
      }
    });
    Self {
      scan_store,
      table,
      flow: CleanupFlow::Idle,
      generation: 0,
      error: None,
      focus_handle: cx.focus_handle(),
      _execute_task: None,
      _subscriptions: vec![subscription],
    }
  }

  fn rebuild_rows(&mut self, cx: &mut Context<Self>) {
    let store = self.scan_store.read(cx);
    let index = store.index();
    let rows: Vec<CollectorRow> = store
      .staged()
      .iter()
      .map(|staged| CollectorRow {
        node_id: staged.node_id.clone(),
        path: staged.path.clone(),
        // Sizes come from the scan index; a path staged from the candidates
        // table that the tree no longer contains shows as unknown.
        size: index
          .and_then(|index| index.get(&staged.node_id))
          .map(|entry| entry.size),
      })
      .collect();
    self
      .table
      .update(cx, |state, _| state.delegate_mut().rows = rows);
    cx.notify();
  }

  /// Step 1+2 of plan §5: build the dry-run plan (a stat per staged path,
  /// deliberately synchronous — plan §4.2) and open the confirm gate.
  fn plan_cleanup(&mut self, cx: &mut Context<Self>) {
    if !matches!(self.flow, CleanupFlow::Idle | CleanupFlow::Reported(_)) {
      return;
    }
    match self.scan_store.update(cx, |store, cx| store.build_plan(cx)) {
      Ok(_) => {
        self.flow = CleanupFlow::Confirming;
        self.error = None;
      }
      Err(message) => self.error = Some(message),
    }
    cx.notify();
  }

  fn cancel_confirm(&mut self, cx: &mut Context<Self>) {
    if matches!(self.flow, CleanupFlow::Confirming) {
      self.flow = CleanupFlow::Idle;
      self.scan_store.update(cx, |store, _| store.clear_plan());
      cx.notify();
    }
  }

  fn dismiss_result(&mut self, cx: &mut Context<Self>) {
    if matches!(self.flow, CleanupFlow::Reported(_)) {
      self.flow = CleanupFlow::Idle;
      cx.notify();
    }
  }

  /// Steps 3-5 of plan §5, behind the confirm gate: re-verify every
  /// fingerprint (one mismatch rejects the whole plan, engine.rs:512-531),
  /// then execute trash-only on a background task.
  fn confirm_execute(&mut self, cx: &mut Context<Self>) {
    if !matches!(self.flow, CleanupFlow::Confirming) {
      return;
    }
    let Some(plan) = self.scan_store.read(cx).plan() else {
      return;
    };
    if plan.expired(Instant::now()) {
      self.flow = CleanupFlow::Idle;
      self.error = Some("the plan expired — plan again from the queue".into());
      self.scan_store.update(cx, |store, _| store.clear_plan());
      cx.notify();
      return;
    }
    // Collect the changed paths alongside the pairs so the rejection can
    // name them (plan §5 step 3); the same rule as `verify_fingerprints`
    // decides a match, and that checked function stays the plan gate.
    let mut pairs: Vec<(EntryFingerprint, Option<EntryFingerprint>)> =
      Vec::with_capacity(plan.entries.len());
    let mut stale: Vec<String> = Vec::new();
    for entry in &plan.entries {
      let current = std::fs::metadata(&entry.path)
        .ok()
        .map(|metadata| EntryFingerprint {
          size: metadata.len(),
          mtime_ms: mtime_ms(metadata.modified().ok()),
        });
      let matches = match &current {
        Some(current) => *current == entry.fingerprint,
        None => false,
      };
      if !matches {
        stale.push(entry.path.clone());
      }
      pairs.push((entry.fingerprint, current));
    }
    if let Err(changed) = verify_fingerprints(&pairs) {
      self.flow = CleanupFlow::Idle;
      self.error = Some(format!(
        "{changed} of {} entries changed since the plan was made — plan again. Changed: {}",
        plan.entries.len(),
        stale.join(" · ")
      ));
      self.scan_store.update(cx, |store, _| store.clear_plan());
      cx.notify();
      return;
    }

    self.generation += 1;
    let generation = self.generation;
    let entries = plan.entries.clone();
    self.flow = CleanupFlow::Executing;

    let (tx, rx) = channel::bounded::<ExecuteMsg>(1);
    cx.background_spawn(async move {
      let mut outcome = CleanupOutcome::default();
      for entry in &entries {
        // Trash-only: the engine's permanent-delete APIs
        // (execute_removal_plan / delete_path) are never imported here, so
        // nothing in this binary can reach them (plan §5).
        match trash::delete(&entry.path) {
          Ok(()) => {
            outcome.bytes_freed += entry.size;
            outcome.trashed.push(TrashedEntry {
              path: entry.path.clone(),
              size: entry.size,
            });
          }
          Err(error) => outcome.failed.push(FailedEntry {
            path: entry.path.clone(),
            message: format!("trash failed: {error}"),
          }),
        }
      }
      // A send error means the receiving view is gone; drop the result.
      let _ = tx.send(ExecuteMsg::Done(generation, outcome)).await;
    })
    .detach();

    self._execute_task = Some(cx.spawn(async move |this, cx| {
      // Both recv arms reset the panel: a panicked background task closes
      // the channel and would leave `Executing` stuck forever (plan §4.3).
      let outcome = match rx.recv().await {
        Ok(msg) => msg,
        Err(_) => ExecuteMsg::Failed(generation, "the background trash task aborted".into()),
      };
      this
        .update(cx, |this, cx| {
          match outcome {
            ExecuteMsg::Done(gen, outcome) if gen == this.generation => {
              // Filter the trashed paths out of the queue, then report.
              this.scan_store.update(cx, |store, cx| {
                store.remove_trashed(&outcome.trashed, cx);
              });
              this.flow = CleanupFlow::Reported(Box::new(outcome));
              this.error = None;
              cx.emit(CleanupFinished);
            }
            ExecuteMsg::Failed(gen, message) if gen == this.generation => {
              this.flow = CleanupFlow::Idle;
              this.error = Some(message);
              cx.emit(CleanupFinished);
            }
            // Stale generation — a newer run owns the panel.
            _ => {}
          }
          cx.notify(); // every path notifies; the panel can never wedge
        })
        .ok(); // the view was closed; nothing left to reset
    }));

    cx.notify();
  }
}

struct CollectorRow {
  node_id: String,
  path: String,
  size: Option<u64>,
}

struct CollectorDelegate {
  rows: Vec<CollectorRow>,
  columns: Vec<Column>,
  scan_store: Entity<ScanStore>,
}

impl CollectorDelegate {
  fn new(scan_store: Entity<ScanStore>) -> Self {
    Self {
      rows: Vec::new(),
      columns: vec![
        Column::new("path", "Path").width(px(420.)),
        Column::new("size", "Size").width(px(100.)),
        Column::new("remove", "").width(px(80.)),
      ],
      scan_store,
    }
  }
}

impl TableDelegate for CollectorDelegate {
  fn columns_count(&self, _: &App) -> usize {
    self.columns.len()
  }

  fn rows_count(&self, _: &App) -> usize {
    self.rows.len()
  }

  fn column(&self, col_ix: usize, _: &App) -> Column {
    self.columns[col_ix].clone()
  }

  fn render_td(
    &mut self,
    row_ix: usize,
    col_ix: usize,
    _: &mut Window,
    _: &mut Context<TableState<Self>>,
  ) -> impl IntoElement {
    let row = &self.rows[row_ix];
    match self.columns[col_ix].key.as_ref() {
      "path" => row.path.clone().into_any_element(),
      "size" => row
        .size
        .map(format_bytes)
        .unwrap_or_else(|| "—".to_string())
        .into_any_element(),
      "remove" => {
        let scan_store = self.scan_store.clone();
        let node_id = row.node_id.clone();
        Button::new(format!("remove-{}", row.node_id))
          .ghost()
          .xsmall()
          .label("Remove")
          .on_click(move |_, _, cx| {
            scan_store.update(cx, |store, cx| store.unstage(&node_id, cx));
          })
          .into_any_element()
      }
      _ => String::new().into_any_element(),
    }
  }
}

impl Focusable for CollectorView {
  fn focus_handle(&self, _: &App) -> FocusHandle {
    self.focus_handle.clone()
  }
}

impl EventEmitter<PanelEvent> for CollectorView {}

impl BasePanel for CollectorView {
  fn panel_name(&self) -> &'static str {
    "collector"
  }
}

impl Panel for CollectorView {}

impl Render for CollectorView {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    // The confirm gate replaces the whole panel while it is up.
    if let CleanupFlow::Confirming = self.flow {
      if let Some(plan) = self.scan_store.read(cx).plan() {
        return self.render_confirm(cx, plan);
      }
      self.flow = CleanupFlow::Idle; // plan vanished; fall through to the queue
    }

    let delegate = self.table.read(cx).delegate();
    let count = delegate.rows.len();
    let total = delegate
      .rows
      .iter()
      .map(|row| row.size.unwrap_or(0))
      .sum::<u64>();
    let unknown = delegate
      .rows
      .iter()
      .filter(|row| row.size.is_none())
      .count();
    let executing = matches!(self.flow, CleanupFlow::Executing);
    // `Reported` is NOT idle: an all-success trash empties the queue, and
    // the outcome report must stay visible until dismissed.
    let idle = matches!(self.flow, CleanupFlow::Idle);
    if count == 0 && idle && self.error.is_none() {
      return placeholder("Use + in Contents or Cleanup to collect paths for review.")
        .into_any_element();
    }
    v_flex()
      .size_full()
      .gap_1()
      .child(
        div().flex_1().overflow_hidden().child(
          DataTable::new(&self.table)
            .stripe(true)
            .bordered(true)
            .scrollbar_visible(true, true),
        ),
      )
      .children(self.error.clone().map(|message| {
        div()
          .text_sm()
          .text_color(cx.theme().colors.danger)
          .child(message)
      }))
      .children(match &self.flow {
        CleanupFlow::Reported(outcome) => Some(self.render_outcome(cx, outcome)),
        _ => None,
      })
      .child(
        h_flex()
          .gap_2()
          .p_2()
          .child(div().text_sm().child(format!(
            "{} path(s) · {}",
            format_count(count),
            format_bytes(total)
          )))
          .children((unknown > 0).then(|| {
            div()
              .text_sm()
              .text_color(cx.theme().colors.muted_foreground)
              .child(format!("{unknown} path(s) not in the scan index"))
          }))
          .child(div().flex_1())
          .child(
            Button::new("plan-cleanup")
              .primary()
              .label(if executing {
                "Moving to Trash…"
              } else {
                "Plan Cleanup…"
              })
              .disabled(executing || count == 0)
              .on_click(cx.listener(|this, _, _, cx| this.plan_cleanup(cx))),
          ),
      )
      .into_any_element()
  }
}

impl CollectorView {
  /// The hard gate (plan §5 step 4): the summary names the count, the size,
  /// every path, and states the trash-only guarantee; reaching the execute
  /// step requires pressing the danger button.
  // Returns `AnyElement` so every return path of `Render::render` above is
  // the same concrete type — an `impl IntoElement` return gets pinned to the
  // first one.
  fn render_confirm(&self, cx: &Context<Self>, plan: &PlannedCleanup) -> AnyElement {
    v_flex()
      .size_full()
      .min_h_0()
      .gap_2()
      .child(div().flex_shrink_0().font_weight(FontWeight::SEMIBOLD).child("Move to Trash"))
      .child(v_flex().id("cleanup-confirm-paths").flex_1().min_h_0().gap_2().overflow_y_scroll()
        .child(div().flex_shrink_0().text_sm().child(format!(
          "{} path(s) · {} will be moved to the Trash. Nothing is permanently deleted — items can be restored from the Trash.",
          format_count(plan.entries.len()),
          format_bytes(plan.total_size)
        )))
      .child(
        v_flex()
          .flex_shrink_0()
          .gap_1()
          .children(plan.entries.iter().map(|entry| {
            div()
              .text_sm()
              .text_color(cx.theme().colors.muted_foreground)
              .child(entry.path.clone())
          })),
      ))
      .child(
        h_flex()
          .flex_shrink_0()
          .gap_2()
          .justify_end()
          .child(
            Button::new("confirm-cancel")
              .ghost()
              .label("Cancel")
              .on_click(cx.listener(|this, _, _, cx| this.cancel_confirm(cx))),
          )
          .child(
            Button::new("confirm-trash")
              .danger()
              .label("Move to Trash")
              .on_click(cx.listener(|this, _, _, cx| this.confirm_execute(cx))),
          ),
      )
      .into_any_element()
  }

  /// Step 6 of plan §5: outcome summary with the failure list. The web
  /// workbench surfaces only the first failure (+page.svelte:285-287); every
  /// failure is shown here.
  fn render_outcome(&self, cx: &Context<Self>, outcome: &CleanupOutcome) -> Div {
    h_flex()
      .items_start()
      .flex_shrink_0()
      .gap_2()
      .p_2()
      .child(div().text_sm().child(format!(
        "Trashed {} path(s) · {} freed",
        format_count(outcome.trashed.len()),
        format_bytes(outcome.bytes_freed)
      )))
      .children((!outcome.failed.is_empty()).then(|| {
        div()
          .text_sm()
          .text_color(cx.theme().colors.danger)
          .child(format!("{} failed:", outcome.failed.len()))
      }))
      .child(
        v_flex()
          .id("cleanup-failures")
          .flex_1()
          .min_w_0()
          .max_h(px(70.))
          .overflow_y_scroll()
          .children(
            outcome
              .failed
              .iter()
              .map(|failure| {
                div()
                  .text_sm()
                  .text_color(cx.theme().colors.danger)
                  .child(format!("{} — {}", failure.path, failure.message))
              })
              .collect::<Vec<_>>(),
          ),
      )
      .child(
        Button::new("dismiss-result")
          .ghost()
          .xsmall()
          .label("Dismiss")
          .on_click(cx.listener(|this, _, _, cx| this.dismiss_result(cx))),
      )
  }
}

/// Messages from the background dirty-git walk, tagged with the generation
/// that started it (plan §4.3).
enum DirtyGitMsg {
  Done(u64, Vec<DirtyGitRepo>),
  Failed(u64, String),
}

struct DirtyGitRow {
  path: String,
  dirty_entries: u32,
}

struct DirtyGitDelegate {
  rows: Vec<DirtyGitRow>,
  columns: Vec<Column>,
}

impl DirtyGitDelegate {
  fn new() -> Self {
    Self {
      rows: Vec::new(),
      columns: vec![
        Column::new("path", "Path").width(px(420.)),
        Column::new("dirty", "Dirty entries").width(px(120.)),
      ],
    }
  }
}

impl TableDelegate for DirtyGitDelegate {
  fn columns_count(&self, _: &App) -> usize {
    self.columns.len()
  }

  fn rows_count(&self, _: &App) -> usize {
    self.rows.len()
  }

  fn column(&self, col_ix: usize, _: &App) -> Column {
    self.columns[col_ix].clone()
  }

  fn render_td(
    &mut self,
    row_ix: usize,
    col_ix: usize,
    _: &mut Window,
    _: &mut Context<TableState<Self>>,
  ) -> impl IntoElement {
    let row = &self.rows[row_ix];
    match self.columns[col_ix].key.as_ref() {
      "path" => row.path.clone().into_any_element(),
      "dirty" => format_count(row.dirty_entries as usize).into_any_element(),
      _ => String::new().into_any_element(),
    }
  }
}

/// Right dock tab: dirty git repositories found under the scanned roots.
pub struct DirtyGitView {
  scan_store: Entity<ScanStore>,
  table: Entity<TableState<DirtyGitDelegate>>,
  scanning: bool,
  generation: u64,
  error: Option<String>,
  focus_handle: FocusHandle,
  _scan_task: Option<Task<()>>,
  _subscriptions: Vec<Subscription>,
}

impl DirtyGitView {
  pub fn new(scan_store: Entity<ScanStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
    let table = cx.new(|cx| TableState::new(DirtyGitDelegate::new(), window, cx));
    let subscription = cx.subscribe(&scan_store, |this, _, event, cx| {
      if matches!(event, StoreEvent::Ingested) {
        this
          .table
          .update(cx, |state, _| state.delegate_mut().rows = Vec::new());
        this.start_search(cx);
      }
    });
    Self {
      scan_store,
      table,
      scanning: false,
      generation: 0,
      error: None,
      focus_handle: cx.focus_handle(),
      _scan_task: None,
      _subscriptions: vec![subscription],
    }
  }

  fn start_search(&mut self, cx: &mut Context<Self>) {
    // A request during a running walk supersedes it: the generation bump
    // below voids the old result when it lands (plan §4.3).
    self.scanning = false;
    self
      .scan_store
      .update(cx, |store, cx| store.set_searching(false, cx));
    let roots = self.scan_store.read(cx).roots().to_vec();
    if roots.is_empty() {
      return;
    }
    self.generation += 1;
    let generation = self.generation;
    let options = DirtyGitRepoOptions {
      roots,
      // Engine defaults (plan §7 risk 7); no picker options here.
      ignore_hidden: false,
      follow_symlinks: false,
    };
    self.scanning = true;
    self
      .scan_store
      .update(cx, |store, cx| store.set_searching(true, cx));
    self.error = None;

    let (tx, rx) = channel::bounded::<DirtyGitMsg>(1);
    cx.background_spawn(async move {
      let repos = find_dirty_git_repos(options);
      let _ = tx.send(DirtyGitMsg::Done(generation, repos)).await;
    })
    .detach();

    self._scan_task = Some(cx.spawn(async move |this, cx| {
      let outcome = match rx.recv().await {
        Ok(msg) => msg,
        Err(_) => DirtyGitMsg::Failed(generation, "the background dirty-git task aborted".into()),
      };
      this
        .update(cx, |this, cx| {
          match outcome {
            DirtyGitMsg::Done(gen, repos) if gen == this.generation => {
              this.scanning = false;
              this
                .scan_store
                .update(cx, |store, cx| store.set_searching(false, cx));
              this.error = None;
              let rows = repos
                .iter()
                .map(|repo| DirtyGitRow {
                  path: repo.path.to_string_lossy().to_string(),
                  dirty_entries: repo.dirty_entries,
                })
                .collect();
              this
                .table
                .update(cx, |state, _| state.delegate_mut().rows = rows);
            }
            DirtyGitMsg::Failed(gen, message) if gen == this.generation => {
              this.scanning = false;
              this
                .scan_store
                .update(cx, |store, cx| store.set_searching(false, cx));
              this.error = Some(message);
            }
            _ => {}
          }
          cx.notify();
        })
        .ok();
    }));

    cx.notify();
  }
}

impl Focusable for DirtyGitView {
  fn focus_handle(&self, _: &App) -> FocusHandle {
    self.focus_handle.clone()
  }
}

impl EventEmitter<PanelEvent> for DirtyGitView {}

impl BasePanel for DirtyGitView {
  fn panel_name(&self) -> &'static str {
    "dirty-git"
  }
}

impl Panel for DirtyGitView {}

impl Render for DirtyGitView {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    let rows = &self.table.read(cx).delegate().rows;
    v_flex()
      .size_full()
      .gap(px(12.))
      .children(self.error.clone().map(|message| {
        div()
          .text_size(px(12.))
          .text_color(cx.theme().danger)
          .child(message)
      }))
      .child(
        v_flex()
          .id("git-repositories")
          .flex_1()
          .min_h_0()
          .overflow_y_scroll()
          .children(rows.iter().enumerate().map(|(ix, row)| {
            let path = Path::new(&row.path);
            let name = path
              .file_name()
              .unwrap_or(path.as_os_str())
              .to_string_lossy()
              .to_string();
            h_flex()
              .id(ix)
              .gap(px(8.))
              .px(px(6.))
              .py(px(10.))
              .border_b_1()
              .border_color(cx.theme().border)
              .child(
                Icon::new(IconName::GitBranch)
                  .small()
                  .text_color(cx.theme().primary),
              )
              .child(
                v_flex()
                  .flex_1()
                  .min_w_0()
                  .gap(px(3.))
                  .child(div().text_size(px(13.)).truncate().child(name))
                  .child(
                    div()
                      .text_size(px(11.))
                      .text_color(muted)
                      .truncate()
                      .child(row.path.clone()),
                  ),
              )
              .child(
                div()
                  .text_size(px(11.))
                  .text_color(muted)
                  .child(format!("{} changes", row.dirty_entries)),
              )
          })),
      )
      .child(
        h_flex()
          .gap_2()
          .text_size(px(11.))
          .text_color(muted)
          .children(
            self
              .scanning
              .then(|| ProgressCircle::new("git-search").xsmall().loading(true)),
          )
          .child(if self.scanning {
            "Checking repositories…".to_string()
          } else {
            format!(
              "{} repositories with uncommitted changes",
              format_count(rows.len())
            )
          }),
      )
  }
}

fn placeholder(message: &str) -> Div {
  div()
    .size_full()
    .flex()
    .items_center()
    .justify_center()
    .text_sm()
    .child(message.to_string())
}

/// Fixed native panes keep navigation, exploration, and review visually stable.
/// Panel entities retain their existing scan generations and cleanup safety gates.
pub struct WorkbenchView {
  scan_store: Entity<ScanStore>,
  explorer: Entity<ExplorerView>,
  candidates: Entity<CandidatesView>,
  dirty_git: Entity<DirtyGitView>,
  collector: Entity<CollectorView>,
  presentation: WorkbenchPresentation,
  _subscriptions: Vec<Subscription>,
}

impl WorkbenchView {
  pub fn new(scan_store: Entity<ScanStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
    let explorer = cx.new(|cx| ExplorerView::new(scan_store.clone(), window, cx));
    let candidates = cx.new(|cx| CandidatesView::new(scan_store.clone(), window, cx));
    let dirty_git = cx.new(|cx| DirtyGitView::new(scan_store.clone(), window, cx));
    let collector = cx.new(|cx| CollectorView::new(scan_store.clone(), window, cx));
    let mut subscriptions = Vec::new();
    subscriptions.push(cx.observe(&explorer, |_, _, cx| cx.notify()));
    subscriptions.push(
      cx.subscribe(&collector, |this, _, _: &CleanupFinished, cx| {
        this.presentation.cleanup_finished();
        cx.notify();
      }),
    );
    subscriptions.push(
      cx.subscribe(&scan_store, |this, store, event: &StoreEvent, cx| {
        match event {
          StoreEvent::Ingested => this.presentation.scan_completed(),
          StoreEvent::StagedChanged => this
            .presentation
            .staged_changed(store.read(cx).staged().len()),
          _ => {}
        }
        cx.notify();
      }),
    );
    Self {
      scan_store,
      explorer,
      candidates,
      dirty_git,
      collector,
      presentation: WorkbenchPresentation::default(),
      _subscriptions: subscriptions,
    }
  }

  pub fn focus_name(&self, cx: &App) -> String {
    self
      .explorer
      .read(cx)
      .focus_name()
      .unwrap_or("Space Lens")
      .to_string()
  }
  pub fn root_path(&self, cx: &App) -> Option<String> {
    self.explorer.read(cx).root_path(cx)
  }
  pub fn inspector_visible(&self) -> bool {
    self.presentation.inspector_visible
  }
  pub fn collector_visible(&self) -> bool {
    self.presentation.collector_visible
  }
  pub fn toggle_inspector(&mut self, cx: &mut Context<Self>) {
    self.presentation.inspector_visible = !self.presentation.inspector_visible;
    cx.notify();
  }
  pub fn toggle_collector(&mut self, cx: &mut Context<Self>) {
    self.presentation.collector_visible = !self.presentation.collector_visible;
    cx.notify();
  }
  pub fn show_inspector(&mut self, tab: InspectorTab, cx: &mut Context<Self>) {
    self.presentation.select_inspector(tab);
    cx.notify();
  }
  pub fn focus_root(&mut self, node_id: &str, cx: &mut Context<Self>) {
    self
      .explorer
      .update(cx, |view, cx| view.focus_root(node_id, cx));
    cx.notify();
  }

  fn render_inspector(&self, content: AnyElement, cx: &Context<Self>) -> Div {
    let mut tabs = h_flex()
      .gap(px(1.))
      .p(px(2.))
      .rounded(px(6.))
      .bg(segment_track(cx));
    for (tab, label) in [
      (InspectorTab::Contents, "Contents"),
      (InspectorTab::Cleanup, "Cleanup"),
      (InspectorTab::Git, "Git"),
    ] {
      let selected = self.presentation.inspector_tab == tab;
      tabs = tabs.child(
        Button::new(format!("inspector-{label}"))
          .ghost()
          .small()
          .label(label)
          .selected(selected)
          .flex_1()
          .rounded(px(4.))
          .bg(if selected {
            segment_selected(cx)
          } else {
            segment_track(cx)
          })
          .on_click(cx.listener(move |this, _, _, cx| this.show_inspector(tab, cx))),
      );
    }
    v_flex()
      .w(px(300.))
      .flex_shrink_0()
      .h_full()
      .min_h_0()
      .bg(inspector(cx))
      .border_l_1()
      .border_color(cx.theme().border)
      .p(px(12.))
      .gap(px(14.))
      .child(tabs)
      .child(div().flex_1().min_h_0().overflow_hidden().child(content))
  }

  fn render_status_bar(&self, cx: &Context<Self>) -> Div {
    let store = self.scan_store.read(cx);
    let roots = store.roots().len();
    let items = store.index().map(|index| index.len()).unwrap_or(0);
    h_flex()
      .w_full()
      .h(px(30.))
      .justify_center()
      .gap_2()
      .bg(toolbar(cx))
      .border_t_1()
      .border_color(cx.theme().border)
      .text_size(px(11.))
      .text_color(cx.theme().muted_foreground)
      .children(
        store
          .searching()
          .then(|| ProgressCircle::new("status-spinner").xsmall().loading(true)),
      )
      .child(format!(
        "{} folder{} · {} items",
        roots,
        if roots == 1 { "" } else { "s" },
        format_count(items)
      ))
  }
}

impl Render for WorkbenchView {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let inspector_content = if self.presentation.inspector_visible {
      Some(match self.presentation.inspector_tab {
        InspectorTab::Contents => self
          .explorer
          .update(cx, |view, cx| view.render_contents(cx))
          .into_any_element(),
        InspectorTab::Cleanup => self.candidates.clone().into_any_element(),
        InspectorTab::Git => self.dirty_git.clone().into_any_element(),
      })
    } else {
      None
    };
    v_flex()
      .size_full()
      .bg(cx.theme().background)
      .child(
        h_flex()
          .flex_1()
          .min_h_0()
          .w_full()
          .items_stretch()
          .child(
            div()
              .flex_1()
              .min_w_0()
              .h_full()
              .child(self.explorer.clone()),
          )
          .children(inspector_content.map(|content| self.render_inspector(content, cx))),
      )
      .children(self.presentation.collector_visible.then(|| {
        v_flex()
          .h(px(260.))
          .min_h(px(160.))
          .w_full()
          .flex_shrink_0()
          .border_t_1()
          .border_color(cx.theme().border)
          .bg(inspector(cx))
          .px(px(16.))
          .py(px(12.))
          .gap_2()
          .child(
            h_flex()
              .gap_2()
              .child(Icon::new(IconName::Inbox).small())
              .child(
                div()
                  .font_weight(FontWeight::MEDIUM)
                  .text_size(px(12.))
                  .child("Collector"),
              )
              .child(div().flex_1())
              .child(
                div()
                  .text_size(px(11.))
                  .text_color(cx.theme().muted_foreground)
                  .child("Review before cleanup"),
              )
              .child(
                Button::new("close-collector")
                  .ghost()
                  .xsmall()
                  .icon(IconName::Close)
                  .tooltip("Hide collector")
                  .on_click(cx.listener(|this, _, _, cx| this.toggle_collector(cx))),
              ),
          )
          .child(div().flex_1().min_h_0().child(self.collector.clone()))
      }))
      .child(self.render_status_bar(cx))
  }
}
