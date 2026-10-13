//! App Storage audit view — installed apps and their scattered storage.
//!
//! A copy-adaptation of the web `AppAuditList`/`AppAuditCard` hierarchy in
//! list form: each row carries the card's Z1 (avatar + name + total size
//! right-aligned), Z2 (App/CLI badge, Known marker, location count), and Z3
//! (purgeable dot + reclaimable sum). Selecting a row shows the entry's
//! detail in the second pane, including every catalog comment in full — the
//! comments are the point of this feature, so they wrap instead of
//! truncating. Read-only: no cleanup or deletion UI (trash-only flows are
//! desktop-web only for now).

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Disableable as _, Icon, Sizable as _};
use gpui_kit::*;
use smol::channel;
use spacelens_app_audit::{
  app_icon_png, audit_installed_apps, AppAuditEntry, AppAuditOptions, AppAuditReport, AppKind,
  AuditSource, Classification,
};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::session::format::{format_bytes, format_count};
use crate::views::theme::{inspector, selected, stripe};

/// Title-cased display label for each audit classification — the Rust twin of
/// the web `classificationLabel` (`packages/web-ui/src/lib/app-audit.ts`).
pub fn classification_label(classification: Classification) -> &'static str {
  match classification {
    Classification::Application => "Application",
    Classification::Cache => "Cache",
    Classification::Versions => "Old versions",
    Classification::Logs => "Logs",
    Classification::Redownloadable => "Re-downloadable",
    Classification::UserData => "User data",
    Classification::Support => "Support",
  }
}

/// A stable per-class accent used for the classification badge and its dot.
/// Fixed hues (not theme-derived) so the helper stays a pure function; the
/// labels stay readable on both appearances.
pub fn classification_color(classification: Classification) -> Hsla {
  Hsla::from(rgb(match classification {
    Classification::Application => 0x3b82f6,
    Classification::Cache => 0xf59e0b,
    Classification::Versions => 0x8b5cf6,
    Classification::Logs => 0x6b7280,
    Classification::Redownloadable => 0x22c55e,
    Classification::UserData => 0xf97316,
    Classification::Support => 0x64748b,
  }))
}

pub fn kind_label(kind: AppKind) -> &'static str {
  match kind {
    AppKind::App => "App",
    AppKind::Cli => "CLI",
  }
}

/// Sum of the sizes of an entry's purgeable locations — the Rust twin of the
/// web `purgeableTotal` (`packages/web-ui/src/lib/app-audit.ts`).
pub fn purgeable_total(entry: &AppAuditEntry) -> u64 {
  entry
    .locations
    .iter()
    .filter(|location| location.purgeable)
    .map(|location| location.size)
    .sum()
}

/// First-letter avatar text for an app name; tolerates blank names — the Rust
/// twin of the web `initialsFor`.
pub fn initials_for(name: &str) -> String {
  match name.trim().chars().next() {
    Some(first) => first.to_uppercase().to_string(),
    None => "?".to_string(),
  }
}

/// Decodes PNG bytes (as returned by [`app_icon_png`]) into a renderable GPUI
/// image. Pure and UI-thread-free: the background icon loader calls it, never
/// render. Returns `None` for undecodable bytes.
///
/// The RGBA→BGRA swap mirrors gpui-pre's own PNG path
/// (`decode_static_image_from_decoder`): `RenderImage` expects BGRA order.
pub fn decode_png_to_render_image(png: &[u8]) -> Option<Arc<RenderImage>> {
  let decoded = image::load_from_memory(png).ok()?;
  let mut rgba = decoded.to_rgba8();
  for pixel in rgba.chunks_exact_mut(4) {
    pixel.swap(0, 2);
  }
  let frame = image::Frame::new(rgba);
  // `Vec<Frame>` converts into the `SmallVec<[Frame; 1]>` `RenderImage::new`
  // takes, so no direct smallvec dependency is needed.
  Some(Arc::new(RenderImage::new(vec![frame])))
}

/// App bundle paths in report order that still need an icon load: entries
/// without an `app_path` (CLI tools) never need one, and paths already in the
/// cache — loaded or permanently failed (`None`) — are never re-requested.
pub fn missing_icon_paths(
  report: &AppAuditReport,
  icons: &HashMap<String, Option<Arc<RenderImage>>>,
) -> Vec<String> {
  report
    .apps
    .iter()
    .filter_map(|app| app.app_path.clone())
    .filter(|path| !icons.contains_key(path))
    .collect()
}

/// The cached renderable icon for an app path, if it loaded successfully: a
/// path absent from the cache (not loaded yet) and a cached `None` (load
/// failed) both map to `None`, i.e. "show the letter avatar".
pub fn icon_for_path(
  icons: &HashMap<String, Option<Arc<RenderImage>>>,
  app_path: &Option<String>,
) -> Option<Arc<RenderImage>> {
  app_path
    .as_ref()
    .and_then(|path| icons.get(path).cloned().flatten())
}

/// The list-row projection of one audit entry: the web card's Z1-Z3 hierarchy
/// flattened into row data (the engine already sorts size-descending).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppRow {
  pub id: String,
  pub name: String,
  pub kind_label: &'static str,
  pub known: bool,
  pub total_size: u64,
  pub location_count: usize,
  pub reclaimable: u64,
}

pub fn app_row(entry: &AppAuditEntry) -> AppRow {
  AppRow {
    id: entry.id.clone(),
    name: entry.name.clone(),
    kind_label: kind_label(entry.kind),
    known: entry.source == AuditSource::Catalog,
    total_size: entry.total_size,
    location_count: entry.locations.len(),
    reclaimable: purgeable_total(entry),
  }
}

/// Messages from the background audit, tagged with the generation that
/// started them (plan §4.3): a refresh supersedes a running audit, and a late
/// result from the old generation is discarded.
enum AuditMsg {
  Done(u64, AppAuditReport),
  Failed(u64, String),
}

pub struct AppStorageView {
  report: Option<AppAuditReport>,
  loading: bool,
  generation: u64,
  error: Option<String>,
  selected: Option<String>,
  /// Per-view icon cache keyed by app bundle path: `Some` once the icon
  /// decoded, `None` permanently when the load failed (CLI entries have no
  /// path and never enter the map). Filled by the background icon loader.
  icons: HashMap<String, Option<Arc<RenderImage>>>,
  _audit_task: Option<Task<()>>,
  _icon_task: Option<Task<()>>,
}

impl AppStorageView {
  pub fn new() -> Self {
    Self {
      report: None,
      loading: false,
      generation: 0,
      error: None,
      selected: None,
      icons: HashMap::new(),
      _audit_task: None,
      _icon_task: None,
    }
  }

  /// Starts the audit on first open; a no-op once a report, a load, or an
  /// error is present (errors retry through the header button only).
  pub fn ensure_started(&mut self, cx: &mut Context<Self>) {
    if self.report.is_none() && !self.loading && self.error.is_none() {
      self.start_audit(cx);
    }
  }

  /// Runs `audit_installed_apps` with the default options (min_size 0,
  /// include_heuristic true, limit 500) on a background thread. A call while
  /// a load is in flight supersedes it via the generation bump.
  fn start_audit(&mut self, cx: &mut Context<Self>) {
    self.generation += 1;
    let generation = self.generation;
    self.loading = true;
    self.error = None;

    let (tx, rx) = channel::bounded::<AuditMsg>(1);
    cx.background_spawn(async move {
      // Only plain data crosses here: the future must stay Send.
      #[cfg(target_os = "macos")]
      {
        // The audit runs kuntu's rayon-parallel scan in-process and would
        // otherwise saturate every core while the UI thread tries to render
        // (hovering the sidebar entry lagged for the whole audit). The macOS
        // QoS class demotes ONLY this thread; the UI thread keeps its
        // user-interactive scheduling. This is the per-thread mechanism the
        // process-wide setpriority cannot provide.
        unsafe {
          libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
        }
      }
      #[cfg(all(unix, not(target_os = "macos")))]
      {
        // Best available on non-macOS unix: nice the process down.
        unsafe {
          libc::setpriority(libc::PRIO_PROCESS, 0, 10);
        }
      }
      let options = AppAuditOptions {
        min_size: 0,
        include_heuristic: true,
        limit: 500,
        home: None,
      };
      let msg = match audit_installed_apps(&options) {
        Ok(report) => AuditMsg::Done(generation, report),
        Err(error) => AuditMsg::Failed(generation, error.to_string()),
      };
      // A send error means the receiving view is gone; drop the result.
      let _ = tx.send(msg).await;
    })
    .detach();

    self._audit_task = Some(cx.spawn(async move |this, cx| {
      // Both recv arms reset the UI: a panicked background task closes the
      // channel, and swallowing that error would leave `loading` stuck
      // forever (plan §4.3).
      let outcome = match rx.recv().await {
        Ok(msg) => msg,
        Err(_) => AuditMsg::Failed(generation, "the background audit task aborted".into()),
      };
      this
        .update(cx, |this, cx| {
          match outcome {
            AuditMsg::Done(gen, report) if gen == this.generation => {
              this.loading = false;
              this.error = None;
              // A refresh can drop the selected app; fall back to the
              // placeholder rather than showing a stale detail.
              if !this
                .selected
                .as_deref()
                .is_some_and(|id| report.apps.iter().any(|app| app.id == id))
              {
                this.selected = None;
              }
              this.report = Some(report);
              // Icons load lazily after the report lands so the list paints
              // immediately; each icon notifies once as it arrives.
              this.start_icon_loading(cx);
            }
            AuditMsg::Failed(gen, message) if gen == this.generation => {
              this.loading = false;
              this.error = Some(message);
            }
            // Stale generation — a newer audit owns the view.
            _ => {}
          }
          cx.notify(); // every path notifies; the UI can never wedge
        })
        .ok(); // the view was closed; nothing left to reset
    }));

    cx.notify();
  }

  /// Loads one icon per app with a bundle path, off the UI thread: the icon
  /// read (`app_icon_png`) and the PNG decode both run in `background_spawn`,
  /// and only the finished `RenderImage` crosses to the entity task, which
  /// inserts it and notifies once per icon. Entries already cached — decoded
  /// or permanently failed — are skipped, and icons for paths the new report
  /// no longer contains are pruned so a refresh never leaks stale entries.
  /// Messages carry the audit generation that started them: a refresh
  /// supersedes a running loader and its late icons are discarded.
  fn start_icon_loading(&mut self, cx: &mut Context<Self>) {
    let Some(report) = &self.report else {
      return;
    };
    let live: std::collections::HashSet<&str> = report
      .apps
      .iter()
      .filter_map(|app| app.app_path.as_deref())
      .collect();
    self.icons.retain(|path, _| live.contains(path.as_str()));
    let paths = missing_icon_paths(report, &self.icons);
    if paths.is_empty() {
      return;
    }
    let generation = self.generation;
    let (tx, rx) = channel::bounded::<(u64, String, Option<Arc<RenderImage>>)>(32);
    cx.background_spawn(async move {
      for path in paths {
        let rendered =
          app_icon_png(Path::new(&path)).and_then(|png| decode_png_to_render_image(&png));
        // A send error means the receiving view is gone; stop loading.
        if tx.send((generation, path, rendered)).await.is_err() {
          break;
        }
      }
    })
    .detach();

    self._icon_task = Some(cx.spawn(async move |this, cx| {
      while let Ok((gen, path, rendered)) = rx.recv().await {
        let current = this.update(cx, |this, cx| {
          if gen == this.generation {
            this.icons.insert(path, rendered);
            cx.notify();
            true
          } else {
            // Stale loader — a newer audit owns the view.
            false
          }
        });
        if !matches!(current, Ok(true)) {
          break;
        }
      }
    }));
  }

  fn select(&mut self, id: String, cx: &mut Context<Self>) {
    // Tapping the active row closes the detail, like the web drawer toggle.
    if self.selected.as_deref() == Some(id.as_str()) {
      self.selected = None;
    } else {
      self.selected = Some(id);
    }
    cx.notify();
  }

  fn render_header(&self, cx: &Context<Self>) -> Div {
    let (count, grand_total) = match &self.report {
      Some(report) => (
        report.apps.len(),
        report.apps.iter().map(|app| app.total_size).sum::<u64>(),
      ),
      None => (0, 0),
    };
    h_flex()
      .w_full()
      .flex_shrink_0()
      .items_center()
      .gap(px(10.))
      .px(px(16.))
      .py(px(12.))
      .child(
        Icon::new(IconName::Package)
          .size(px(18.))
          .text_color(cx.theme().primary),
      )
      .child(
        v_flex()
          .min_w_0()
          .child(
            div()
              .text_size(px(15.))
              .font_weight(FontWeight::SEMIBOLD)
              .child("App Storage"),
          )
          .child(
            div()
              .text_size(px(11.))
              .text_color(cx.theme().muted_foreground)
              .child("What installed apps and CLI tools keep on this disk"),
          ),
      )
      .child(div().flex_1())
      .children(self.report.as_ref().map(|_| {
        div()
          .text_size(px(12.))
          .font_weight(FontWeight::SEMIBOLD)
          .child(format_bytes(grand_total))
      }))
      .children(self.report.as_ref().map(|_| {
        div()
          .text_size(px(11.))
          .text_color(cx.theme().muted_foreground)
          .child(format!(
            "{} app{} audited",
            format_count(count),
            if count == 1 { "" } else { "s" }
          ))
      }))
      .children(self.loading.then(|| {
        div()
          .text_size(px(11.))
          .text_color(cx.theme().muted_foreground)
          .child(if self.report.is_some() {
            "Refreshing…"
          } else {
            "Auditing…"
          })
      }))
      .child(
        Button::new("app-storage-refresh")
          .ghost()
          .small()
          .icon(IconName::RefreshCw)
          .label("Refresh")
          .disabled(self.loading)
          .tooltip("Re-run the installed-apps audit")
          .on_click(cx.listener(|this, _, _, cx| this.start_audit(cx))),
      )
  }

  fn render_body(&self, cx: &Context<Self>) -> Div {
    if self.loading && self.report.is_none() && self.error.is_none() {
      return v_flex()
        .flex_1()
        .min_h_0()
        .w_full()
        .items_center()
        .justify_center()
        .gap_2()
        .child(
          Progress::new("app-storage-progress")
            .loading(true)
            .w(px(280.)),
        )
        .child(
          div()
            .text_size(px(12.))
            .text_color(cx.theme().muted_foreground)
            .child("Auditing installed apps…"),
        );
    }
    if let Some(message) = &self.error {
      if self.report.is_none() {
        return v_flex()
          .flex_1()
          .min_h_0()
          .w_full()
          .items_center()
          .justify_center()
          .gap_2()
          .child(
            div()
              .text_size(px(12.))
              .text_color(cx.theme().danger)
              .child(message.clone()),
          )
          .child(
            Button::new("app-storage-retry")
              .small()
              .label("Try again")
              .on_click(cx.listener(|this, _, _, cx| this.start_audit(cx))),
          );
      }
    }
    let Some(report) = &self.report else {
      return v_flex()
        .flex_1()
        .min_h_0()
        .w_full()
        .items_center()
        .justify_center()
        .child(
          div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child("No audit yet — refresh to enumerate installed apps."),
        );
    };
    if report.apps.is_empty() {
      return v_flex()
        .flex_1()
        .min_h_0()
        .w_full()
        .items_center()
        .justify_center()
        .child(
          div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child("No apps found on this machine."),
        );
    }
    // Master-detail split: `h_flex` centers children, so the row stretches
    // them to the full height. Every ancestor of the two scroll containers is
    // bounded (`flex_1` + `min_h_0`, `min_w_0` on this horizontal split) so
    // the lists measure against the window instead of their content.
    h_flex()
      .flex_1()
      .min_h_0()
      .min_w_0()
      .w_full()
      .items_stretch()
      .child(self.render_list(report, cx))
      .child(self.render_detail(report, cx))
  }

  /// The size-descending app list: one row per entry with the web card's
  /// Z1-Z3 hierarchy (name + total, badges + count, reclaimable dot).
  ///
  /// The outer wrapper is a `v_flex` (column) so the inner scroll container's
  /// `flex_1` resolves vertically against the bounded split height: inside a
  /// plain `div` (row direction) `flex_1` would grow horizontally and the
  /// list would size to its content, never scrolling. Row `.id(ix)` keys
  /// match workbench.rs's scrolling lists (`cleanup-candidates`,
  /// `git-repositories`, `explorer-contents`), which scroll fine with them —
  /// plain keyed rows without `on_scroll` do not swallow wheel events.
  fn render_list(&self, report: &AppAuditReport, cx: &Context<Self>) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut list = v_flex()
      .id("app-storage-list")
      .flex_1()
      .min_w_0()
      .min_h_0()
      .overflow_y_scroll();
    for (ix, app) in report.apps.iter().enumerate() {
      let row = app_row(app);
      let active = self.selected.as_deref() == Some(app.id.as_str());
      let id = app.id.clone();
      list = list.child(
        h_flex()
          .id(ix)
          .w_full()
          .px(px(12.))
          .py(px(10.))
          .gap(px(10.))
          .items_center()
          .rounded(px(4.))
          .bg(if active {
            selected(cx)
          } else if ix % 2 == 1 {
            stripe(cx)
          } else {
            inspector(cx)
          })
          .hover(|style| style.bg(cx.theme().list_hover))
          .cursor_pointer()
          .child(app_avatar(
            &self.icons,
            &app.app_path,
            &app.name,
            px(32.),
            px(8.),
            px(13.),
            cx,
          ))
          .child(
            v_flex()
              .flex_1()
              .min_w_0()
              .gap(px(2.))
              .child(div().truncate().text_size(px(13.)).child(app.name.clone()))
              .child(
                h_flex()
                  .gap(px(6.))
                  .items_center()
                  .min_w_0()
                  .child(badge(row.kind_label, muted, cx))
                  .children(row.known.then(|| badge("Known", cx.theme().primary, cx)))
                  .child(
                    div()
                      .truncate()
                      .text_size(px(11.))
                      .text_color(muted)
                      .child(format!(
                        "· {} location{}",
                        format_count(row.location_count),
                        if row.location_count == 1 { "" } else { "s" }
                      )),
                  ),
              )
              .children((row.reclaimable > 0).then(|| {
                h_flex()
                  .gap(px(6.))
                  .items_center()
                  .child(
                    div()
                      .size(px(6.))
                      .flex_shrink_0()
                      .rounded_full()
                      .bg(cx.theme().primary),
                  )
                  .child(
                    div()
                      .truncate()
                      .text_size(px(11.))
                      .text_color(muted)
                      .child(format!("{} reclaimable", format_bytes(row.reclaimable))),
                  )
              })),
          )
          .child(
            div()
              .flex_shrink_0()
              .text_size(px(13.))
              .font_weight(FontWeight::SEMIBOLD)
              .child(format_bytes(row.total_size)),
          )
          .on_click(cx.listener(move |this, _, _, cx| this.select(id.clone(), cx))),
      );
    }
    v_flex()
      .flex_1()
      .min_w_0()
      .h_full()
      .min_h_0()
      .px(px(8.))
      .py(px(8.))
      .child(list)
  }

  fn render_detail(&self, report: &AppAuditReport, cx: &Context<Self>) -> Div {
    let muted = cx.theme().muted_foreground;
    let selected_entry = self
      .selected
      .as_deref()
      .and_then(|id| report.apps.iter().find(|app| app.id == id));
    let Some(app) = selected_entry else {
      return v_flex()
        .w(px(380.))
        .flex_shrink_0()
        .h_full()
        .min_h_0()
        .bg(inspector(cx))
        .border_l_1()
        .border_color(cx.theme().border)
        .items_center()
        .justify_center()
        .px(px(24.))
        .child(
          div()
            .text_sm()
            .text_color(muted)
            .text_center()
            .child("Select an app to see where its storage lives."),
        );
    };
    let reclaimable = purgeable_total(app);
    // The scroll container owns the `.id()` (overflow needs one); it wraps
    // the detail content so this function keeps its `Div` return.
    let content = v_flex()
      .id("app-storage-detail")
      .flex_1()
      .min_h_0()
      .w_full()
      .overflow_y_scroll()
      .px(px(16.))
      .py(px(14.))
      .gap(px(10.))
      .child(
        h_flex()
          .gap(px(10.))
          .items_center()
          .min_w_0()
          .child(app_avatar(
            &self.icons,
            &app.app_path,
            &app.name,
            px(40.),
            px(10.),
            px(15.),
            cx,
          ))
          .child(
            v_flex()
              .flex_1()
              .min_w_0()
              .child(
                div()
                  .truncate()
                  .text_size(px(15.))
                  .font_weight(FontWeight::SEMIBOLD)
                  .child(app.name.clone()),
              )
              .child(
                div()
                  .text_size(px(12.))
                  .font_weight(FontWeight::SEMIBOLD)
                  .child(format_bytes(app.total_size)),
              ),
          ),
      )
      .child(
        h_flex()
          .gap(px(6.))
          .items_center()
          .child(badge(kind_label(app.kind), muted, cx))
          .children(
            (app.source == AuditSource::Catalog).then(|| badge("Known", cx.theme().primary, cx)),
          ),
      )
      .children(app.bundle_id.clone().map(|bundle_id| {
        div()
          .truncate()
          .text_size(px(11.))
          .text_color(muted)
          .child(bundle_id)
      }))
      .children(app.app_path.clone().map(|app_path| {
        div()
          .truncate()
          .text_size(px(11.))
          .text_color(muted)
          .child(app_path)
      }))
      .children((app.installed_size > 0).then(|| {
        div().text_size(px(11.)).text_color(muted).child(format!(
          "includes {} app bundle",
          format_bytes(app.installed_size)
        ))
      }))
      .children((reclaimable > 0).then(|| {
        h_flex()
          .gap(px(6.))
          .items_center()
          .child(
            div()
              .size(px(6.))
              .flex_shrink_0()
              .rounded_full()
              .bg(cx.theme().primary),
          )
          .child(
            div()
              .text_size(px(11.))
              .text_color(muted)
              .child(format!("{} reclaimable", format_bytes(reclaimable))),
          )
      }))
      .child(
        div()
          .w_full()
          .h(px(1.))
          .flex_shrink_0()
          .bg(cx.theme().border),
      )
      .children(
        app
          .locations
          .iter()
          .enumerate()
          .map(|(ix, location)| self.render_location(ix, location, cx)),
      );
    v_flex()
      .w(px(380.))
      .flex_shrink_0()
      .h_full()
      .min_h_0()
      .bg(inspector(cx))
      .border_l_1()
      .border_color(cx.theme().border)
      .child(content)
  }

  fn render_location(
    &self,
    ix: usize,
    location: &spacelens_app_audit::AppAuditLocation,
    cx: &Context<Self>,
  ) -> Stateful<Div> {
    let muted = cx.theme().muted_foreground;
    let class_color = classification_color(location.classification);
    v_flex()
      .id(ix)
      .w_full()
      .gap(px(3.))
      .py(px(6.))
      .child(
        h_flex()
          .gap(px(6.))
          .items_center()
          .min_w_0()
          .children(location.purgeable.then(|| {
            div()
              .size(px(6.))
              .flex_shrink_0()
              .rounded_full()
              .bg(cx.theme().primary)
          }))
          .child(
            div()
              .flex_shrink_0()
              .text_size(px(12.))
              .font_weight(FontWeight::SEMIBOLD)
              .child(format_bytes(location.size)),
          )
          .child(badge(
            classification_label(location.classification),
            class_color,
            cx,
          ))
          .children(location.label.clone().map(|label| {
            div()
              .truncate()
              .text_size(px(12.))
              .font_weight(FontWeight::SEMIBOLD)
              .child(label)
          })),
      )
      .child(
        div()
          .truncate()
          .text_size(px(11.))
          .text_color(muted)
          .child(location.path.clone()),
      )
      // The catalog comments are the point of this feature: always shown in
      // full, wrapped — never truncated away.
      .children(location.comment.clone().map(|comment| {
        div()
          .w_full()
          .text_size(px(12.))
          .text_color(cx.theme().foreground)
          .child(comment)
      }))
  }

  fn render_footnote(&self, cx: &Context<Self>) -> Option<Div> {
    let report = self.report.as_ref()?;
    let reclaimable: u64 = report.apps.iter().map(purgeable_total).sum();
    Some(
      div()
        .w_full()
        .flex_shrink_0()
        .px(px(16.))
        .py(px(8.))
        .border_t_1()
        .border_color(cx.theme().border)
        .text_size(px(11.))
        .text_color(cx.theme().muted_foreground)
        .child(format!(
          "Reclaimable classes: {} · Classification, not a recommendation — nothing is selected or deleted automatically.",
          format_bytes(reclaimable)
        )),
    )
  }
}

impl Render for AppStorageView {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    // Belt and braces with the sidebar entry's `ensure_started`: opening the
    // phase always ends up with a load in flight. Guarded, so the notify
    // below never loops.
    self.ensure_started(cx);
    // `size_full` fills the shell's screen slot; `min_h_0`/`min_w_0` keep the
    // flex chain shrinkable so the scroll containers below stay bounded.
    v_flex()
      .id("app-storage")
      .size_full()
      .min_h_0()
      .min_w_0()
      .bg(cx.theme().background)
      .text_color(cx.theme().foreground)
      .child(self.render_header(cx))
      .child(
        div()
          .w_full()
          .h(px(1.))
          .flex_shrink_0()
          .bg(cx.theme().border),
      )
      .child(self.render_body(cx))
      .children(self.render_footnote(cx))
  }
}

/// An app's icon when its bundle icon loaded, else the existing letter
/// avatar: the icon path goes through `ImageSource::Render`, the decoded
/// `RenderImage` held in the view's cache. Sized for list rows (32px) and the
/// detail header (40px) by the caller.
fn app_avatar(
  icons: &std::collections::HashMap<String, Option<Arc<RenderImage>>>,
  app_path: &Option<String>,
  name: &str,
  size: Pixels,
  radius: Pixels,
  font: Pixels,
  cx: &App,
) -> AnyElement {
  if let Some(rendered) = icon_for_path(icons, app_path) {
    return img(ImageSource::Render(rendered))
      .size(size)
      .flex_shrink_0()
      .rounded(radius)
      .into_any_element();
  }
  let muted = cx.theme().muted_foreground;
  div()
    .size(size)
    .flex_shrink_0()
    .rounded(radius)
    .bg(stripe(cx))
    .flex()
    .items_center()
    .justify_center()
    .child(
      div()
        .text_size(font)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(muted)
        .child(initials_for(name)),
    )
    .into_any_element()
}

/// A small bordered pill for kind / Known / classification markers.
fn badge(label: impl Into<SharedString>, color: Hsla, _cx: &App) -> Div {
  div()
    .flex_shrink_0()
    .rounded_full()
    .border_1()
    .border_color(color.opacity(0.45))
    .px(px(7.))
    .py(px(1.))
    .text_size(px(10.))
    .font_weight(FontWeight::MEDIUM)
    .text_color(color)
    .child(label.into())
}

#[cfg(test)]
mod tests {
  // Explicit imports, not `use super::*`: the file's top-level
  // `use gpui_kit::*` glob would pull the whole kit prelude into the test
  // module and blow the `#[test]` expansion over the recursion limit.
  use super::{
    app_row, classification_color, classification_label, decode_png_to_render_image, icon_for_path,
    initials_for, missing_icon_paths, purgeable_total,
  };
  use gpui_kit::RenderImage;
  use spacelens_app_audit::{
    AppAuditEntry, AppAuditLocation, AppAuditReport, AppKind, AuditSource, Classification,
  };
  use std::collections::{BTreeSet, HashMap};
  use std::sync::Arc;

  fn location(path: &str, size: u64, purgeable: bool) -> AppAuditLocation {
    AppAuditLocation {
      path: path.into(),
      label: None,
      classification: Classification::Cache,
      purgeable,
      comment: None,
      size,
      source: AuditSource::Heuristic,
    }
  }

  fn entry(id: &str) -> AppAuditEntry {
    AppAuditEntry {
      id: id.into(),
      name: "Raycast".into(),
      kind: AppKind::App,
      source: AuditSource::Catalog,
      bundle_id: Some("com.raycast.macos".into()),
      app_path: Some("/Applications/Raycast.app".into()),
      installed_size: 1_000,
      total_size: 2_000,
      locations: vec![
        location("/Users/hk/Library/Caches/com.raycast.macos", 100, true),
        location(
          "/Users/hk/Library/Application Support/com.raycast.macos",
          900,
          false,
        ),
      ],
    }
  }

  #[test]
  fn every_classification_has_a_label_and_a_distinct_color() {
    let cases = [
      (Classification::Application, "Application"),
      (Classification::Cache, "Cache"),
      (Classification::Versions, "Old versions"),
      (Classification::Logs, "Logs"),
      (Classification::Redownloadable, "Re-downloadable"),
      (Classification::UserData, "User data"),
      (Classification::Support, "Support"),
    ];
    for (classification, label) in cases {
      assert_eq!(classification_label(classification), label);
    }
    // `Hsla` carries no `PartialEq`, so uniqueness goes through its debug
    // rendering — one stable color per class.
    let colors: BTreeSet<String> = cases
      .iter()
      .map(|(classification, _)| format!("{:?}", classification_color(*classification)))
      .collect();
    assert_eq!(colors.len(), cases.len());
  }

  #[test]
  fn report_entries_project_to_list_rows() {
    let app = entry("raycast");
    assert_eq!(purgeable_total(&app), 100);
    assert_eq!(initials_for("Raycast"), "R");
    assert_eq!(initials_for("   "), "?");

    let row = app_row(&app);
    assert_eq!(row.id, "raycast");
    assert_eq!(row.name, "Raycast");
    assert_eq!(row.kind_label, "App");
    assert!(row.known);
    assert_eq!(row.total_size, 2_000);
    assert_eq!(row.location_count, 2);
    assert_eq!(row.reclaimable, 100);

    let cli = AppAuditEntry {
      kind: AppKind::Cli,
      source: AuditSource::Heuristic,
      bundle_id: None,
      app_path: None,
      installed_size: 0,
      ..entry("codex-cli")
    };
    let cli_row = app_row(&cli);
    assert_eq!(cli_row.kind_label, "CLI");
    assert!(!cli_row.known);
  }

  fn test_render_image() -> Arc<RenderImage> {
    let frame = image::Frame::new(image::ImageBuffer::from_pixel(
      1,
      1,
      image::Rgba([0, 0, 0, 255]),
    ));
    Arc::new(RenderImage::new(vec![frame]))
  }

  fn icon_report() -> AppAuditReport {
    let mut first = entry("raycast");
    first.app_path = Some("/Applications/Raycast.app".into());
    let mut cli = entry("codex-cli");
    cli.app_path = None; // CLI entries have no bundle and never need icons
    let mut third = entry("zoom");
    third.app_path = Some("/Applications/Zoom.app".into());
    AppAuditReport {
      platform: "macos".into(),
      generated_at: "2026-10-13T00:00:00Z".into(),
      apps: vec![first, cli, third],
      total_apps: 3,
      truncated: false,
    }
  }

  #[test]
  fn icon_loader_requests_only_uncached_bundle_paths_in_order() {
    let report = icon_report();
    let icons: HashMap<String, Option<Arc<RenderImage>>> = HashMap::new();
    // Report order, CLI entries skipped.
    assert_eq!(
      missing_icon_paths(&report, &icons),
      vec![
        "/Applications/Raycast.app".to_string(),
        "/Applications/Zoom.app".to_string(),
      ]
    );
    // A decoded icon and a permanent failure are both cached: neither is
    // re-requested.
    let icons: HashMap<String, Option<Arc<RenderImage>>> = HashMap::from([
      (
        "/Applications/Raycast.app".to_string(),
        Some(test_render_image()),
      ),
      ("/Applications/Zoom.app".to_string(), None),
    ]);
    assert!(missing_icon_paths(&report, &icons).is_empty());
  }

  #[test]
  fn icon_cache_maps_missing_and_failed_paths_to_the_letter_avatar() {
    let icons: HashMap<String, Option<Arc<RenderImage>>> = HashMap::from([
      (
        "/Applications/Ok.app".to_string(),
        Some(test_render_image()),
      ),
      ("/Applications/Broken.app".to_string(), None),
    ]);
    assert!(icon_for_path(&icons, &Some("/Applications/Ok.app".into())).is_some());
    assert!(icon_for_path(&icons, &Some("/Applications/Broken.app".into())).is_none());
    assert!(icon_for_path(&icons, &Some("/Applications/Unknown.app".into())).is_none());
    assert!(icon_for_path(&icons, &None).is_none());
  }

  #[test]
  fn png_bytes_decode_to_a_single_rgba_frame() {
    let buffer = image::ImageBuffer::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]));
    let dynamic = image::DynamicImage::ImageRgba8(buffer);
    let mut cursor = std::io::Cursor::new(Vec::new());
    dynamic
      .write_to(&mut cursor, image::ImageFormat::Png)
      .expect("test PNG encodes");
    let png = cursor.into_inner();
    let rendered = decode_png_to_render_image(&png).expect("test PNG decodes");
    assert_eq!(rendered.frame_count(), 1);
    assert_eq!(
      rendered.as_bytes(0).map(|bytes| bytes.len()),
      Some(2 * 2 * 4)
    );
    assert!(decode_png_to_render_image(b"not a png").is_none());
  }
}
