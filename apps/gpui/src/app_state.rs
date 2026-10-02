//! One native window shell around the picker and the live workbench.

use std::collections::BTreeSet;
use std::path::Path;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Disableable as _, Icon, Sizable as _};
use gpui_kit::*;

use crate::session::index::node_id_of;
use crate::store::{ScanStore, StoreEvent};
use crate::views::picker::{PickerEvent, PickerView};
use crate::views::presentation::InspectorTab;
use crate::views::theme::{
  selected, sidebar, sidebar_button, theme_toggle_button, toolbar, SIDEBAR_WIDTH, TOOLBAR_HEIGHT,
};
use crate::views::workbench::WorkbenchView;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
  Picker,
  Workbench,
}

pub struct AppState {
  phase: Phase,
  scan_store: Entity<ScanStore>,
  picker: Entity<PickerView>,
  workbench: Entity<WorkbenchView>,
  sidebar_visible: bool,
  _subscriptions: Vec<Subscription>,
}

impl AppState {
  pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
    let scan_store = cx.new(|_| ScanStore::new());
    let picker = cx.new(|cx| PickerView::new(scan_store.clone(), window, cx));
    let workbench = cx.new(|cx| WorkbenchView::new(scan_store.clone(), window, cx));
    let mut subscriptions = vec![cx.subscribe(&picker, |this, _, event: &PickerEvent, cx| {
      match event {
        PickerEvent::ScanCompleted => this.phase = Phase::Workbench,
        PickerEvent::Cancelled if this.scan_store.read(cx).index().is_some() => {
          this.phase = Phase::Workbench
        }
        _ => {}
      }
      cx.notify();
    })];
    subscriptions.push(cx.observe(&workbench, |_, _, cx| cx.notify()));
    subscriptions.push(cx.observe(&picker, |_, _, cx| cx.notify()));
    subscriptions.push(cx.subscribe(&scan_store, |_, _, _: &StoreEvent, cx| cx.notify()));
    Self {
      phase: Phase::Picker,
      scan_store,
      picker,
      workbench,
      sidebar_visible: true,
      _subscriptions: subscriptions,
    }
  }

  fn select_path(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
    let scanned = self
      .scan_store
      .read(cx)
      .roots()
      .iter()
      .any(|root| root.to_string_lossy() == path);
    if scanned {
      self.show_workbench(cx);
      self
        .workbench
        .update(cx, |view, cx| view.focus_root(&node_id_of(&path), cx));
    } else {
      self.phase = Phase::Picker;
      self
        .picker
        .update(cx, |view, cx| view.select_path(&path, window, cx));
    }
    cx.notify();
  }

  fn show_workbench(&mut self, cx: &mut Context<Self>) {
    // Returning to an existing result supersedes any scan in the picker.
    self.picker.update(cx, |view, cx| view.cancel_scan(cx));
    self.phase = Phase::Workbench;
  }

  fn render_toolbar(&self, cx: &Context<Self>) -> Div {
    let title = if self.phase == Phase::Picker {
      "New Scan".to_string()
    } else {
      self.workbench.read(cx).focus_name(cx)
    };
    let scanned = self.scan_store.read(cx).index().is_some();
    let staged = self.scan_store.read(cx).staged().len();
    h_flex()
      .w_full()
      .h(px(TOOLBAR_HEIGHT))
      .flex_shrink_0()
      .bg(toolbar(cx))
      .border_b_1()
      .border_color(cx.theme().border)
      .child(
        h_flex()
          .w(px(if self.sidebar_visible {
            SIDEBAR_WIDTH
          } else {
            136.
          }))
          .h_full()
          .flex_shrink_0()
          .bg(sidebar(cx).opacity(0.94))
          .pl(px(99.))
          .child(
            Button::new("toggle-sidebar")
              .ghost()
              .small()
              .accessibility_label("Toggle sidebar")
              .icon(IconName::PanelLeft)
              .tooltip("Toggle sidebar")
              .on_click(cx.listener(|this, _, _, cx| {
                this.sidebar_visible = !this.sidebar_visible;
                cx.notify();
              })),
          ),
      )
      .child(
        h_flex()
          .flex_1()
          .min_w_0()
          .px(px(16.))
          .gap(px(8.))
          .child(
            v_flex()
              .id("window-title-drag")
              .gap(px(1.))
              .min_w_0()
              .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
              .child(
                div()
                  .truncate()
                  .text_size(px(14.))
                  .font_weight(FontWeight::SEMIBOLD)
                  .child(title),
              )
              .child(
                div()
                  .text_size(px(11.))
                  .text_color(cx.theme().muted_foreground)
                  .child("Space Lens"),
              ),
          )
          .child(
            div()
              .id("toolbar-drag")
              .flex_1()
              .h_full()
              .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move()),
          )
          .child(
            Button::new("new-scan")
              .ghost()
              .small()
              .icon(IconName::FolderPlus)
              .label("New Scan")
              .on_click(cx.listener(|this, _, _, cx| {
                this.phase = Phase::Picker;
                cx.notify();
              })),
          )
          .child(div().w(px(1.)).h(px(21.)).mx_1().bg(cx.theme().border))
          .child(
            Button::new("toggle-collector")
              .ghost()
              .small()
              .accessibility_label("Toggle collector")
              .bg(if self.workbench.read(cx).collector_visible() {
                selected(cx)
              } else {
                toolbar(cx)
              })
              .icon(IconName::Inbox)
              .tooltip(if staged == 0 {
                "Show collector".to_string()
              } else {
                format!("Collector · {} paths", staged)
              })
              .disabled(!scanned)
              .on_click(cx.listener(|this, _, _, cx| {
                this.show_workbench(cx);
                this
                  .workbench
                  .update(cx, |view, cx| view.toggle_collector(cx));
                cx.notify();
              })),
          )
          .child(
            Button::new("toggle-inspector")
              .ghost()
              .small()
              .accessibility_label("Toggle inspector")
              .bg(if self.workbench.read(cx).inspector_visible() {
                selected(cx)
              } else {
                toolbar(cx)
              })
              .icon(IconName::PanelRight)
              .tooltip("Toggle inspector")
              .disabled(!scanned)
              .on_click(cx.listener(|this, _, _, cx| {
                this
                  .workbench
                  .update(cx, |view, cx| view.toggle_inspector(cx))
              })),
          )
          .child(theme_toggle_button(cx)),
      )
  }

  fn sidebar_heading(&self, title: &'static str, cx: &Context<Self>) -> Div {
    div()
      .px(px(9.))
      .pt(px(15.))
      .pb(px(5.))
      .text_size(px(11.))
      .font_weight(FontWeight::SEMIBOLD)
      .text_color(cx.theme().muted_foreground)
      .child(title)
  }

  fn render_sidebar(&self, cx: &Context<Self>) -> Div {
    let recent = self.picker.read(cx).recent_scans().to_vec();
    let scanned = self.scan_store.read(cx).index().is_some();
    let active_root = if self.phase == Phase::Workbench {
      self.workbench.read(cx).root_path(cx)
    } else {
      None
    };
    let mut locations: Vec<(String, String, IconName)> = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
      locations.push((
        "Home".into(),
        home.to_string_lossy().to_string(),
        IconName::House,
      ));
    }
    let mut volumes = BTreeSet::new();
    for entry in &recent {
      if let Some(path) = entry.path.strip_prefix("/Volumes/") {
        if let Some(volume) = path.split('/').next() {
          volumes.insert(volume.to_string());
        }
      }
    }
    for volume in volumes {
      locations.push((
        volume.clone(),
        format!("/Volumes/{volume}"),
        IconName::HardDrive,
      ));
    }
    let mut content = v_flex()
      .id("native-sidebar")
      .flex_1()
      .min_h_0()
      .overflow_y_scroll()
      .px(px(10.))
      .pb(px(12.))
      .child(self.sidebar_heading("Locations", cx));
    for (ix, (name, path, icon)) in locations.into_iter().enumerate() {
      content = content.child(
        sidebar_button(format!("location-{ix}"), name, icon, cx)
          .tooltip(path.clone())
          .on_click(cx.listener(move |this, _, window, cx| {
            this.phase = Phase::Picker;
            this
              .picker
              .update(cx, |view, cx| view.select_path(&path, window, cx));
            cx.notify();
          })),
      );
    }
    if !recent.is_empty() {
      content = content.child(self.sidebar_heading("Recent Scans", cx));
      for (ix, entry) in recent.into_iter().enumerate() {
        let active = active_root.as_deref() == Some(entry.path.as_str());
        let name = Path::new(&entry.path)
          .file_name()
          .map(|value| value.to_string_lossy().to_string())
          .unwrap_or_else(|| entry.path.clone());
        content = content.child(
          h_flex()
            .w_full()
            .rounded(px(5.))
            .bg(if active {
              selected(cx)
            } else {
              sidebar(cx).opacity(0.0)
            })
            .child(
              sidebar_button(format!("recent-{ix}"), name, IconName::Folder, cx)
                .flex_1()
                .min_w_0()
                .tooltip(entry.path.clone())
                .on_click(cx.listener(move |this, _, window, cx| {
                  this.select_path(entry.path.clone(), window, cx)
                })),
            )
            .child(
              Button::new(format!("forget-{ix}"))
                .ghost()
                .xsmall()
                .icon(Icon::new(IconName::Close).size(px(10.)))
                .tooltip("Remove from recent scans")
                .on_click(cx.listener(move |this, _, _, cx| {
                  this
                    .picker
                    .update(cx, |picker, cx| picker.forget_recent(ix, cx))
                })),
            ),
        );
      }
    }
    content = content.child(self.sidebar_heading("Tools", cx));
    for (tab, label, icon) in [
      (InspectorTab::Cleanup, "Cleanup", IconName::BrushCleaning),
      (InspectorTab::Git, "Git Repositories", IconName::GitBranch),
    ] {
      content = content.child(
        sidebar_button(format!("tool-{label}"), label, icon, cx)
          .disabled(!scanned)
          .on_click(cx.listener(move |this, _, _, cx| {
            this.show_workbench(cx);
            this
              .workbench
              .update(cx, |view, cx| view.show_inspector(tab, cx));
            cx.notify();
          })),
      );
    }
    v_flex()
      .w(px(SIDEBAR_WIDTH))
      .flex_shrink_0()
      .h_full()
      .min_h_0()
      .bg(sidebar(cx).opacity(0.94))
      .border_r_1()
      .border_color(cx.theme().border)
      .child(content)
      .child(
        div()
          .px(px(19.))
          .py(px(16.))
          .text_size(px(11.))
          .text_color(cx.theme().muted_foreground)
          .child("Space Lens"),
      )
  }
}

impl Render for AppState {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let screen = match self.phase {
      Phase::Picker => self.picker.clone().into_any_element(),
      Phase::Workbench => self.workbench.clone().into_any_element(),
    };
    v_flex()
      .size_full()
      .text_color(cx.theme().foreground)
      .text_size(px(13.))
      .child(self.render_toolbar(cx))
      .child(
        h_flex()
          .flex_1()
          .min_h_0()
          .w_full()
          .items_stretch()
          .children(self.sidebar_visible.then(|| self.render_sidebar(cx)))
          .child(div().flex_1().min_w_0().h_full().child(screen)),
      )
  }
}
