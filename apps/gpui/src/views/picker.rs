//! Scan picker view — the app's entry screen (plan §3.1, milestone 3).

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::progress::Progress;
// `Icon`/`IconName` live in a private `icon` module glob-re-exported from the
// component root (gpui-component 0.7.0 lib.rs `pub use icon::*;`).
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::*;
use serde::{Deserialize, Serialize};
use smol::channel;
use space_lens::{IgnoredMode, ScanNode, ScanOptions};

use crate::store::ScanStore;
use crate::views::theme::inspector;

/// Emitted when a scan of the current generation finishes successfully and
/// its tree was ingested into the shared `ScanStore`; the app shell switches
/// to the workbench on this signal (plan §4.1).
pub enum PickerEvent {
  ScanCompleted,
  Cancelled,
}

/// One remembered scan root, persisted as local JSON (plan §3.1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecentScan {
  pub path: String,
  pub scanned_at_ms: u64,
}

const RECENT_LIMIT: usize = 5;
/// macOS-only for the MVP: the desktop line targets macOS (plan §7 risk 8).
const RECENT_DIR: &str = "Library/Application Support/SpaceLens";
const RECENT_FILE: &str = "recent-scans.json";

/// Messages flowing from the background scan back to the UI, each tagged with
/// the generation that started it (plan §4.3).
enum ScanMsg {
  Done(u64, Vec<ScanNode>),
  Failed(u64, String),
}

pub struct PickerView {
  scan_store: Entity<ScanStore>,
  path_input: Entity<InputState>,
  second_path_input: Entity<InputState>,
  ignore_hidden: bool,
  respect_gitignore: bool,
  follow_symlinks: bool,
  exclude_ignored: bool,
  scanning: bool,
  generation: u64,
  error: Option<String>,
  recent: Vec<RecentScan>,
  _scan_task: Option<Task<()>>,
  _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PickerEvent> for PickerView {}

impl PickerView {
  pub fn recent_scans(&self) -> &[RecentScan] {
    &self.recent
  }

  pub fn select_path(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
    self.cancel_scan(cx);
    self
      .path_input
      .update(cx, |state, cx| state.set_value(path, window, cx));
    self.error = None;
    cx.notify();
  }

  pub fn forget_recent(&mut self, index: usize, cx: &mut Context<Self>) {
    if index < self.recent.len() {
      self.recent.remove(index);
      save_recent(&self.recent);
      cx.notify();
    }
  }
  pub fn new(scan_store: Entity<ScanStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
    let path_input = cx.new(|cx| InputState::new(window, cx).placeholder("/Users/you/Projects"));
    let second_path_input =
      cx.new(|cx| InputState::new(window, cx).placeholder("Optional second folder"));

    // Enter on either field starts the scan. The returned Subscriptions are
    // kept alive in `_subscriptions`; dropping one unsubscribes.
    let mut subscriptions = Vec::new();
    for input in [&path_input, &second_path_input] {
      subscriptions.push(cx.subscribe_in(input, window, |this, _, event, _, cx| {
        if matches!(event, InputEvent::PressEnter { .. }) {
          this.request_scan(cx);
        }
      }));
    }

    let mut this = Self {
      scan_store,
      path_input,
      second_path_input,
      // Defaults mirror the engine's ScanStartRequest defaults: gitignore
      // respected, hidden files scanned, symlinks not followed (symlinked
      // checkouts are measured as link nodes, plan §7 risk 7), ignored
      // entries summarized into their parents.
      ignore_hidden: false,
      respect_gitignore: true,
      follow_symlinks: false,
      exclude_ignored: false,
      scanning: false,
      generation: 0,
      error: None,
      recent: load_recent(),
      _scan_task: None,
      _subscriptions: subscriptions,
    };

    // Screenshot/demo hook: `SPACLENS_GPUI_AUTO_SCAN=<dir>` prefills the
    // primary path and starts the scan without any interaction, so automated
    // UI captures can reach the workbench on a headless-ish run.
    if let Some(target) = std::env::var_os("SPACLENS_GPUI_AUTO_SCAN") {
      let target = target.to_string_lossy().to_string();
      this.path_input.update(cx, |state, cx| {
        state.set_value(target.as_str(), window, cx);
      });
      this.request_scan(cx);
    }

    this
  }

  fn request_scan(&mut self, cx: &mut Context<Self>) {
    if self.scanning {
      return;
    }
    self.generation += 1;
    let generation = self.generation;

    // Path validation runs synchronously on the main thread — canonicalize is
    // cheap, and a bad path enters the error state without spawning a task
    // (plan §4.3).
    let options = match self.build_scan_options(cx) {
      Ok(options) => options,
      Err(message) => {
        self.error = Some(message);
        cx.notify();
        return;
      }
    };

    self.scanning = true;
    self.error = None;
    let (tx, rx) = channel::bounded::<ScanMsg>(1);
    cx.background_spawn(async move {
      // Only plain data crosses here: the future must stay Send.
      let tree = space_lens::scan_directory(options);
      // The engine never errors (Vec, not Result): an empty tree means no
      // readable root, normalized into a failure by the app layer (plan §4.3).
      let msg = if tree.is_empty() {
        ScanMsg::Failed(
          generation,
          "the scan produced no tree: the folders are missing or unreadable".into(),
        )
      } else {
        ScanMsg::Done(generation, tree)
      };
      // A send error means the receiving view is gone; drop the result.
      let _ = tx.send(msg).await;
    })
    .detach();

    self._scan_task = Some(cx.spawn(async move |this, cx| {
      // Both Ok and Err paths reset the UI: a panicked background task closes
      // the channel, and swallowing that error would leave `scanning` stuck
      // and the button unclickable forever (plan §4.3).
      let outcome = match rx.recv().await {
        Ok(msg) => msg,
        Err(_) => ScanMsg::Failed(generation, "the background scan task aborted".into()),
      };
      this
        .update(cx, |this, cx| {
          match outcome {
            ScanMsg::Done(gen, tree) if gen == this.generation => {
              this.scanning = false;
              this.error = None;
              this.remember_recent(&tree);
              // Ingest directly (plan §4.3) — the tree moves into the store
              // without a clone, and the store's `Ingested` event refreshes
              // the workbench panels.
              this
                .scan_store
                .update(cx, |store, cx| store.ingest(tree, cx));
              cx.emit(PickerEvent::ScanCompleted);
            }
            ScanMsg::Failed(gen, message) if gen == this.generation => {
              // Retryable: the button is clickable again, the message shows.
              this.scanning = false;
              this.error = Some(message);
            }
            // Stale generation — a newer scan (or a cancel) owns the UI.
            _ => {}
          }
          cx.notify(); // every path notifies; the UI can never wedge
        })
        .ok(); // the view was closed; nothing left to reset
    }));

    cx.notify();
  }

  pub fn cancel_scan(&mut self, cx: &mut Context<Self>) {
    if !self.scanning {
      return;
    }
    // Dropping the task does not stop the running rayon scan: bumping the
    // generation voids it instead, so its late result is discarded and the
    // UI is interactive immediately (plan §4.3).
    self.generation += 1;
    self.scanning = false;
    self._scan_task = None;
    cx.notify();
  }

  fn browse(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
    cx.spawn_in(window, async move |this, cx| {
      // rfd's macOS open panel must run on the main thread; spawn_in polls
      // this future on the main-thread executor (gpui-pre Context::spawn_in).
      let picked = rfd::AsyncFileDialog::new().pick_folder().await;
      let Some(handle) = picked else { return };
      let path = handle.path().to_string_lossy().to_string();
      this
        .update_in(cx, |this, window, cx| {
          this.path_input.update(cx, |state, cx| {
            state.set_value(path.as_str(), window, cx);
          });
          cx.notify();
        })
        .ok(); // the view was closed
    })
    .detach();
  }

  fn build_scan_options(&self, cx: &App) -> Result<ScanOptions, String> {
    let primary = self.path_input.read(cx).value().trim().to_string();
    if primary.is_empty() {
      return Err("enter a folder to scan".into());
    }
    let mut directories = vec![canonicalize_checked(&primary)?];
    let second = self.second_path_input.read(cx).value().trim().to_string();
    if !second.is_empty() {
      directories.push(canonicalize_checked(&second)?);
    }
    Ok(ScanOptions {
      directories,
      // full_path matches the desktop engine's sessions (engine.rs:360).
      full_path: true,
      ignore_hidden: self.ignore_hidden,
      respect_gitignore: self.respect_gitignore,
      ignored_mode: if self.exclude_ignored {
        IgnoredMode::Exclude
      } else {
        IgnoredMode::Summarize
      },
      follow_symlinks: self.follow_symlinks,
    })
  }

  fn remember_recent(&mut self, tree: &[ScanNode]) {
    // One entry per scanned root: the web ScanPicker lists recent targets
    // individually, so re-running any single root stays possible after a
    // scan that used the optional second folder.
    let now = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map(|duration| duration.as_millis() as u64)
      .unwrap_or(0);
    for root in tree {
      let entry = RecentScan {
        path: root.path.to_string_lossy().to_string(),
        scanned_at_ms: now,
      };
      self.recent = push_recent(std::mem::take(&mut self.recent), entry);
    }
    save_recent(&self.recent);
  }
}

fn canonicalize_checked(path: &str) -> Result<PathBuf, String> {
  std::fs::canonicalize(path).map_err(|_| format!("path does not exist: {path}"))
}

/// Prepends `entry`, dropping prior entries with the same path and capping at
/// [`RECENT_LIMIT`] — the shape of the web ScanPicker's recent list.
fn push_recent(recent: Vec<RecentScan>, entry: RecentScan) -> Vec<RecentScan> {
  let mut next = Vec::with_capacity(RECENT_LIMIT);
  let path = entry.path.clone();
  next.push(entry);
  for existing in recent {
    if next.len() == RECENT_LIMIT {
      break;
    }
    if existing.path != path {
      next.push(existing);
    }
  }
  next
}

fn recent_file_path() -> Option<PathBuf> {
  let home = std::env::var_os("HOME")?;
  Some(PathBuf::from(home).join(RECENT_DIR).join(RECENT_FILE))
}

/// Best-effort load: first launch or a corrupt file both yield an empty list.
fn load_recent() -> Vec<RecentScan> {
  let Some(path) = recent_file_path() else {
    return Vec::new();
  };
  std::fs::read_to_string(path)
    .ok()
    .and_then(|json| serde_json::from_str(&json).ok())
    .unwrap_or_default()
}

/// Best-effort save: a full disk or missing HOME must never fail the scan.
fn save_recent(recent: &[RecentScan]) {
  let Some(path) = recent_file_path() else {
    return;
  };
  if let Some(dir) = path.parent() {
    let _ = std::fs::create_dir_all(dir);
  }
  if let Ok(json) = serde_json::to_string_pretty(recent) {
    let _ = std::fs::write(path, json);
  }
}

impl Render for PickerView {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    let form = v_flex()
      .w(px(520.))
      .max_w_full()
      .gap(px(14.))
      .child(
        h_flex()
          .gap(px(10.))
          .child(
            div()
              .w(px(92.))
              .flex_shrink_0()
              .text_right()
              .text_size(px(12.))
              .child("Folder:"),
          )
          .child(
            Input::new(&self.path_input)
              .small()
              .cleanable(true)
              .flex_1()
              .min_w_0(),
          )
          .child(
            Button::new("browse")
              .small()
              .label("Choose…")
              .on_click(cx.listener(Self::browse)),
          ),
      )
      .child(
        h_flex()
          .gap(px(10.))
          .child(
            div()
              .w(px(92.))
              .flex_shrink_0()
              .text_right()
              .text_size(px(12.))
              .child("Second folder:"),
          )
          .child(
            Input::new(&self.second_path_input)
              .small()
              .cleanable(true)
              .flex_1()
              .min_w_0(),
          ),
      )
      .child(
        v_flex()
          .pl(px(102.))
          .pt(px(12.))
          .gap(px(14.))
          .child(
            v_flex()
              .gap(px(3.))
              .child(
                Checkbox::new("respect-gitignore")
                  .small()
                  .label("Respect .gitignore")
                  .checked(self.respect_gitignore)
                  .on_click(cx.listener(|this, checked, _, cx| {
                    this.respect_gitignore = *checked;
                    cx.notify();
                  })),
              )
              .child(
                div()
                  .pl(px(22.))
                  .text_size(px(11.))
                  .text_color(muted)
                  .child("Summarize ignored files in their parent folders."),
              ),
          )
          .child(
            Checkbox::new("ignore-hidden")
              .small()
              .label("Ignore hidden files")
              .checked(self.ignore_hidden)
              .on_click(cx.listener(|this, checked, _, cx| {
                this.ignore_hidden = *checked;
                cx.notify();
              })),
          )
          .child(
            v_flex()
              .gap(px(3.))
              .child(
                Checkbox::new("follow-symlinks")
                  .small()
                  .label("Follow symbolic links")
                  .checked(self.follow_symlinks)
                  .on_click(cx.listener(|this, checked, _, cx| {
                    this.follow_symlinks = *checked;
                    cx.notify();
                  })),
              )
              .child(
                div()
                  .pl(px(22.))
                  .text_size(px(11.))
                  .text_color(muted)
                  .child("Include the folders they point to."),
              ),
          )
          .child(
            Checkbox::new("exclude-ignored")
              .small()
              .label("Exclude ignored entries from sizes")
              .checked(self.exclude_ignored)
              .on_click(cx.listener(|this, checked, _, cx| {
                this.exclude_ignored = *checked;
                cx.notify();
              })),
          ),
      )
      .children(self.error.clone().map(|message| {
        div()
          .pl(px(102.))
          .text_size(px(12.))
          .text_color(cx.theme().danger)
          .child(message)
      }))
      .children(self.scanning.then(|| {
        v_flex()
          .pt_3()
          .gap_2()
          .child(Progress::new("scan-progress").loading(true).w_full())
          .child(
            div()
              .text_size(px(11.))
              .text_color(muted)
              .child("Reading folder sizes…"),
          )
      }))
      .child(
        h_flex()
          .w_full()
          .justify_end()
          .gap_2()
          .mt(px(16.))
          .pt(px(16.))
          .border_t_1()
          .border_color(cx.theme().border)
          .child(
            Button::new("cancel")
              .small()
              .label("Cancel")
              .on_click(cx.listener(|this, _, _, cx| {
                if this.scanning {
                  this.cancel_scan(cx);
                } else {
                  cx.emit(PickerEvent::Cancelled);
                }
              })),
          )
          .child(
            Button::new("scan")
              .primary()
              .small()
              .label(if self.scanning { "Scanning…" } else { "Scan" })
              .loading(self.scanning)
              .on_click(cx.listener(|this, _, _, cx| this.request_scan(cx))),
          ),
      );

    v_flex()
      .id("scan-picker")
      .size_full()
      .overflow_y_scroll()
      .bg(inspector(cx))
      .items_center()
      .px(px(32.))
      .py(px(48.))
      .gap(px(28.))
      .child(
        v_flex()
          .items_center()
          .gap(px(8.))
          .child(
            Icon::new(IconName::FolderSearch)
              .size(px(36.))
              .text_color(cx.theme().primary),
          )
          .child(
            div()
              .text_size(px(21.))
              .font_weight(FontWeight::SEMIBOLD)
              .child("Choose a folder to scan"),
          )
          .child(
            div()
              .text_size(px(12.))
              .text_color(muted)
              .child("Explore disk usage and find space to reclaim."),
          ),
      )
      .child(form)
  }
}

#[cfg(test)]
mod tests {
  // Explicit imports, not `use super::*`: the file's top-level
  // `use gpui_kit::*` glob would pull the whole kit prelude into the test
  // module and blow the `#[test]` expansion over the recursion limit.
  use super::{push_recent, RecentScan, RECENT_LIMIT};

  fn recent(path: &str) -> RecentScan {
    RecentScan {
      path: path.into(),
      scanned_at_ms: 0,
    }
  }

  #[test]
  fn recent_round_trips_through_json() {
    let entries = vec![recent("/a"), recent("/b")];
    let json = serde_json::to_string(&entries).unwrap();
    let back: Vec<RecentScan> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, entries);
  }

  #[test]
  fn recent_push_moves_a_duplicate_path_to_front() {
    let pushed = push_recent(vec![recent("/a"), recent("/b")], recent("/a"));
    let paths: Vec<&str> = pushed.iter().map(|entry| entry.path.as_str()).collect();
    assert_eq!(paths, ["/a", "/b"]);
  }

  #[test]
  fn recent_push_keeps_at_most_five_entries() {
    // push_recent's contract: the input is newest-first, the new entry goes
    // to the front, and the oldest tail falls off past the limit.
    let entries: Vec<RecentScan> = (0..5).rev().map(|i| recent(&format!("/p{i}"))).collect();
    let pushed = push_recent(entries, recent("/p5"));
    assert_eq!(pushed.len(), RECENT_LIMIT);
    assert_eq!(pushed.first().unwrap().path, "/p5");
    // The oldest entry (/p0) fell off the end.
    assert_eq!(pushed.last().unwrap().path, "/p1");
  }
}
