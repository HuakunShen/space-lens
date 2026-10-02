//! The explorer panel — the web workbench's chart view (plan §3.1): a
//! painted sunburst of the focused directory beside its contents list, with
//! breadcrumb navigation, a collapsed-entries banner, and the same pastel
//! palette as the web (`session::sunburst`). Replaces the old children
//! DataTable: the web UI explores by chart + list, not by a raw table.

use std::{cell::Cell, f32::consts::TAU, rc::Rc};

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::component::plot::shape::{Arc, ArcData};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Disableable as _, Icon, Sizable as _};
use gpui_kit::*;

use crate::session::format::{format_bytes, format_count};
use crate::session::index::SortMode;
use crate::session::plan::StagedPath;
use crate::session::sunburst::{build_sunburst, muted_color, segment_color, SunburstSegment};
use crate::store::{ScanStore, StoreEvent};
use crate::views::theme::{inspector, stripe};

/// Ring depth of the painted sunburst, exactly the useful range of the web
/// chart: center, children, grandchildren, one more.
const MAX_DEPTH: u32 = 4;
/// Below ~0.6° a wedge is a hairline; prune it and its subtree.
const MIN_ANGLE: f32 = 0.01;

#[derive(Clone)]
struct FocusEntry {
  node_id: String,
  name: String,
}

#[derive(Clone)]
struct ChildRow {
  node_id: String,
  name: String,
  path: String,
  size: u64,
  items: usize,
  ignored: bool,
  collapsed: bool,
  has_children: bool,
}

pub struct ExplorerView {
  scan_store: Entity<ScanStore>,
  focus: Option<FocusEntry>,
  rows: Vec<ChildRow>,
  segments: Vec<SunburstSegment>,
  hovered: Option<String>,
  focus_handle: FocusHandle,
  _subscriptions: Vec<Subscription>,
}

impl ExplorerView {
  pub fn new(scan_store: Entity<ScanStore>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
    let subscription = cx.subscribe(&scan_store, |this, _, event, cx| {
      if matches!(event, StoreEvent::Ingested) {
        this.reset_focus(cx);
      }
    });

    let mut this = Self {
      scan_store,
      focus: None,
      rows: Vec::new(),
      segments: Vec::new(),
      hovered: None,
      focus_handle: cx.focus_handle(),
      _subscriptions: vec![subscription],
    };
    this.reset_focus(cx);
    this
  }

  fn reset_focus(&mut self, cx: &mut Context<Self>) {
    self.hovered = None;
    let store = self.scan_store.read(cx);
    self.focus = store.root_ids().first().and_then(|root_id| {
      store.index()?.get(root_id).map(|entry| FocusEntry {
        node_id: entry.id.clone(),
        name: entry.name.clone(),
      })
    });
    self.reload(cx);
  }

  fn navigate_to(&mut self, entry: FocusEntry, cx: &mut Context<Self>) {
    self.focus = Some(entry);
    self.hovered = None;
    self.reload(cx);
  }

  /// Rebuilds the row list and the sunburst segments for the focused node —
  /// both together, so the chart and the list can never disagree.
  fn reload(&mut self, cx: &mut Context<Self>) {
    let Some(focus) = self.focus.clone() else {
      self.rows = Vec::new();
      self.segments = Vec::new();
      cx.notify();
      return;
    };
    let store = self.scan_store.read(cx);
    self.rows = store
      .children(&focus.node_id, SortMode::Size)
      .map(|children| {
        children
          .iter()
          .map(|entry| ChildRow {
            node_id: entry.id.clone(),
            name: entry.name.clone(),
            path: entry.path.clone(),
            size: entry.size,
            items: entry.child_ids.len(),
            ignored: entry.ignored,
            collapsed: entry.collapsed,
            has_children: !entry.child_ids.is_empty(),
          })
          .collect()
      })
      .unwrap_or_default();
    self.segments = store
      .index()
      .map(|index| build_sunburst(index, &focus.node_id, MAX_DEPTH, MIN_ANGLE))
      .unwrap_or_default();
    cx.notify();
  }

  fn descend(&mut self, node_id: &str, name: &str, cx: &mut Context<Self>) {
    self.navigate_to(
      FocusEntry {
        node_id: node_id.to_string(),
        name: name.to_string(),
      },
      cx,
    );
  }

  fn collect(&mut self, node_id: &str, path: &str, cx: &mut Context<Self>) {
    let entry = StagedPath {
      node_id: node_id.to_string(),
      path: path.to_string(),
    };
    self
      .scan_store
      .update(cx, |store, cx| store.stage(entry, cx));
  }

  /// The chart card: sunburst canvas with the total in the hole, hover/click
  /// hit-testing over the segments, and the summary line under it. The
  /// canvas's prepaint records its bounds into `bounds_cell`, and the mouse
  /// listeners read the same cell — one geometry, shared.
  fn render_chart(&self, focus: &FocusEntry, cx: &Context<Self>) -> Div {
    let total = self
      .scan_store
      .read(cx)
      .index()
      .and_then(|index| index.get(&focus.node_id))
      .map(|entry| entry.size)
      .unwrap_or(0);

    let bounds_cell: Rc<Cell<Bounds<Pixels>>> = Rc::new(Cell::new(Bounds::default()));
    let paint_segments = self.segments.clone();
    let paint_hover = self.hovered.clone();
    let move_segments = self.segments.clone();
    let move_cell = bounds_cell.clone();
    let click_segments = self.segments.clone();
    let click_cell = bounds_cell.clone();

    let foreground = cx.theme().foreground;
    let chart = div()
      .relative()
      .flex_1()
      .min_h_0()
      .id("explorer-chart")
      .child(
        canvas(
          {
            let cell = bounds_cell.clone();
            move |bounds: Bounds<Pixels>, _, _| cell.set(bounds)
          },
          move |bounds: Bounds<Pixels>, _, window: &mut Window, _: &mut App| {
            paint_sunburst(&bounds, &paint_segments, paint_hover.as_deref(), window);
          },
        )
        .size_full(),
      )
      .child(
        v_flex()
          .absolute()
          .inset_0()
          .items_center()
          .justify_center()
          .gap(px(4.))
          .child(
            div()
              .text_size(px(28.))
              .font_weight(FontWeight::SEMIBOLD)
              .child(format_bytes(total)),
          )
          .child(
            div()
              .max_w(px(130.))
              .truncate()
              .text_size(px(12.))
              .text_color(cx.theme().muted_foreground)
              .child(focus.name.clone()),
          ),
      )
      .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, window, cx| {
        let bounds = move_cell.get();
        if bounds.size.width.as_f32() <= 0.0 {
          return;
        }
        let hit = segment_at(&move_segments, &bounds, window.mouse_position());
        if hit != this.hovered {
          this.hovered = hit;
          cx.notify();
        }
      }))
      .on_hover(cx.listener(|this, hovering: &bool, _, cx| {
        if !hovering && this.hovered.take().is_some() {
          cx.notify();
        }
      }))
      .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
        let bounds = click_cell.get();
        if bounds.size.width.as_f32() <= 0.0 {
          return;
        }
        let hit =
          segment_at(&click_segments, &bounds, window.mouse_position()).and_then(|node_id| {
            click_segments
              .iter()
              .find(|segment| segment.node_id == node_id)
              .cloned()
          });
        if let Some(segment) = hit {
          this.descend(&segment.node_id, &segment.name, cx);
        }
      }));

    let summary = match self
      .hovered
      .as_deref()
      .and_then(|id| self.segments.iter().find(|segment| segment.node_id == id))
    {
      Some(segment) => {
        let share = if total > 0 {
          segment.size as f32 / total as f32 * 100.0
        } else {
          0.0
        };
        format!(
          "{} · {} · {:.1}%",
          segment.name,
          format_bytes(segment.size),
          share
        )
      }
      None => "Select a folder to explore its contents.".to_string(),
    };

    v_flex()
      .flex_1()
      .min_w_0()
      .min_h_0()
      .gap_2()
      .child(chart)
      .child(
        div()
          .text_xs()
          .text_color(foreground.opacity(0.55))
          .text_center()
          .h(px(20.))
          .child(summary),
      )
  }

  /// The right sidebar: every direct child as a colored row — dot in the
  /// row's ring color, name, item count, size, stage and descend buttons.
  pub fn render_contents(&self, cx: &Context<Self>) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut list = v_flex()
      .id("explorer-contents")
      .overflow_y_scroll()
      .flex_1()
      .min_h_0();
    for (ix, row) in self.rows.iter().enumerate() {
      let color = if row.ignored {
        muted_color(1)
      } else {
        segment_color(&row.node_id, 1)
      };
      let mut item = h_flex()
        .id(ix)
        .w_full()
        .px(px(8.))
        .py(px(9.))
        .gap(px(8.))
        .items_center()
        .rounded(px(4.))
        .bg(if ix % 2 == 1 {
          stripe(cx)
        } else {
          inspector(cx)
        })
        .hover(|style| style.bg(cx.theme().list_hover))
        .child(
          div()
            .size(px(6.))
            .flex_shrink_0()
            .rounded_full()
            .bg(rgb(color)),
        )
        .child(
          v_flex()
            .flex_1()
            .min_w_0()
            .child(div().text_size(px(13.)).truncate().child(row.name.clone()))
            .child(
              div()
                .text_size(px(11.))
                .text_color(muted)
                .child(if row.collapsed {
                  "Summarized".into()
                } else {
                  format!("{} items", format_count(row.items))
                }),
            ),
        )
        .child(
          div()
            .text_size(px(11.))
            .text_color(muted)
            .flex_shrink_0()
            .child(format_bytes(row.size)),
        )
        .child(
          Button::new(SharedString::from(format!("stage-{ix}")))
            .ghost()
            .xsmall()
            .accessibility_label(format!("Collect {}", row.name))
            .tooltip(format!("Collect {}", row.path))
            .icon(Icon::new(IconName::Plus).size(px(12.)))
            .on_click(cx.listener({
              let row = row.clone();
              move |this, _, _, cx| {
                cx.stop_propagation();
                this.collect(&row.node_id, &row.path, cx);
              }
            })),
        );
      if row.has_children {
        item = item.cursor_pointer().on_click(cx.listener({
          let row = row.clone();
          move |this, _, _, cx| this.descend(&row.node_id, &row.name, cx)
        }));
      }
      list = list.child(item);
    }
    v_flex()
      .size_full()
      .gap(px(6.))
      .child(
        h_flex()
          .px(px(8.))
          .py(px(5.))
          .justify_between()
          .text_size(px(11.))
          .text_color(muted)
          .child("Name")
          .child("Size ↓"),
      )
      .child(list)
      .child(
        div()
          .mt_2()
          .pt_3()
          .px_2()
          .border_t_1()
          .border_color(cx.theme().border)
          .text_size(px(11.))
          .text_color(muted)
          .child(format!(
            "{} items in this folder",
            format_count(self.rows.len())
          )),
      )
  }

  pub fn focus_name(&self) -> Option<&str> {
    self.focus.as_ref().map(|entry| entry.name.as_str())
  }

  pub fn root_path(&self, cx: &App) -> Option<String> {
    let focus = self.focus.as_ref()?;
    let store = self.scan_store.read(cx);
    let index = store.index()?;
    let ancestors = index.ancestors(&focus.node_id)?;
    let root = ancestors
      .first()
      .copied()
      .or_else(|| index.get(&focus.node_id))?;
    Some(root.path.clone())
  }

  pub fn focus_root(&mut self, node_id: &str, cx: &mut Context<Self>) {
    let entry = self
      .scan_store
      .read(cx)
      .index()
      .and_then(|index| index.get(node_id))
      .map(|entry| FocusEntry {
        node_id: entry.id.clone(),
        name: entry.name.clone(),
      });
    if let Some(entry) = entry {
      self.navigate_to(entry, cx);
    }
  }

  fn render_breadcrumb(&self, cx: &Context<Self>) -> Div {
    let (ancestors, current) = match &self.focus {
      Some(focus) => {
        let chain = self
          .scan_store
          .read(cx)
          .ancestors(&focus.node_id)
          .map(|entries| {
            entries
              .iter()
              .map(|entry| FocusEntry {
                node_id: entry.id.clone(),
                name: entry.name.clone(),
              })
              .collect::<Vec<_>>()
          })
          .unwrap_or_default();
        (chain, Some(focus.clone()))
      }
      None => (Vec::new(), None),
    };

    let back = cx.listener(|this, _, _, cx| {
      let Some(focus) = this.focus.clone() else {
        return;
      };
      let parent = this
        .scan_store
        .read(cx)
        .ancestors(&focus.node_id)
        .and_then(|chain| {
          chain.last().map(|entry| FocusEntry {
            node_id: entry.id.clone(),
            name: entry.name.clone(),
          })
        });
      if let Some(entry) = parent {
        this.navigate_to(entry, cx);
      }
    });

    let mut crumbs = h_flex().gap_1().flex_wrap().min_w_0();
    for (ix, entry) in ancestors.iter().enumerate() {
      crumbs = crumbs
        .child(
          Button::new(SharedString::from(format!("crumb-{ix}")))
            .ghost()
            .small()
            .label(entry.name.clone())
            .on_click(cx.listener({
              let entry = entry.clone();
              move |this, _, _, cx| this.navigate_to(entry.clone(), cx)
            })),
        )
        .child(Icon::new(IconName::ArrowRight).small());
    }
    if let Some(entry) = &current {
      crumbs = crumbs.child(
        h_flex()
          .gap_1()
          .px_2()
          .py_0p5()
          .rounded_full()
          .bg(stripe(cx))
          .child(Icon::new(IconName::House).small())
          .child(div().text_sm().truncate().child(entry.name.clone())),
      );
    }

    h_flex()
      .gap_2()
      .min_w_0()
      .child(
        Button::new("go-up")
          .ghost()
          .small()
          .icon(Icon::new(IconName::ArrowUp))
          .disabled(ancestors.is_empty())
          .on_click(back),
      )
      .child(crumbs)
  }

  fn render_placeholder(&self, cx: &Context<Self>) -> Div {
    div()
      .size_full()
      .flex()
      .items_center()
      .justify_center()
      .text_sm()
      .text_color(cx.theme().foreground.opacity(0.5))
      .child("Run a scan to explore it here.")
  }
}

/// Ring geometry for the chart `bounds`: `(hole, band)` — the single source
/// both the paint pass and hit-testing read, so a hover can never disagree
/// with what is on screen.
fn ring_geometry(bounds: &Bounds<Pixels>) -> (f32, f32) {
  let radius =
    (bounds.size.width.as_f32().min(bounds.size.height.as_f32()) / 2.0 - 18.0).clamp(0.0, 190.0);
  let hole = (radius * 0.34).max(58.0).min(radius * 0.7);
  let band = (radius - hole) / MAX_DEPTH as f32;
  (hole, band)
}

#[cfg(test)]
mod geometry_tests {
  use super::{ring_geometry, MAX_DEPTH};
  use gpui_kit::{point, px, size, Bounds};

  #[test]
  fn rings_fit_a_short_chart_above_the_collector() {
    let bounds = Bounds::new(point(px(0.), px(0.)), size(px(300.), px(100.)));
    let (hole, band) = ring_geometry(&bounds);
    assert!(band > 0.);
    assert!(hole + MAX_DEPTH as f32 * band <= 50.);
    assert!(hole < 50.);
  }
}

/// The wedge under `position` (window pixels), or None for the hole and the
/// area outside the outer ring.
fn segment_at(
  segments: &[SunburstSegment],
  bounds: &Bounds<Pixels>,
  position: Point<Pixels>,
) -> Option<String> {
  let (hole, band) = ring_geometry(bounds);
  let center = bounds.center();
  let dx = position.x.as_f32() - center.x.as_f32();
  let dy = position.y.as_f32() - center.y.as_f32();
  let radius = dx.hypot(dy);
  if radius < hole {
    return None;
  }
  // Screen angle -> 0 at 12 o'clock, clockwise (the plot Arc convention).
  let angle = (dy.atan2(dx) + std::f32::consts::FRAC_PI_2).rem_euclid(TAU);
  segments
    .iter()
    .find(|segment| {
      let r0 = hole + (segment.depth - 1) as f32 * band;
      r0 <= radius
        && radius <= r0 + band
        && segment.start_angle <= angle
        && angle < segment.end_angle
    })
    .map(|segment| segment.node_id.clone())
}

/// Paints every segment: ring bands with a small radial gap and pad angle,
/// the web palette (muted for ignored), and non-hovered segments dimmed
/// while the cursor is over one.
fn paint_sunburst(
  bounds: &Bounds<Pixels>,
  segments: &[SunburstSegment],
  hovered: Option<&str>,
  window: &mut Window,
) {
  let (hole, band) = ring_geometry(bounds);
  let gap = (band * 0.12).min(1.5);
  for segment in segments {
    let r0 = hole + (segment.depth - 1) as f32 * band + gap;
    let r1 = hole + segment.depth as f32 * band - gap;
    let color_u32 = if segment.ignored {
      muted_color(segment.depth)
    } else {
      segment_color(&segment.node_id, segment.depth)
    };
    let mut color = Hsla::from(rgb(color_u32));
    if hovered.is_some_and(|id| id != segment.node_id) {
      color = color.opacity(0.45);
    }
    let mut arc_data = ArcData::new(
      segment,
      0,
      segment.size as f32,
      segment.start_angle,
      segment.end_angle,
    );
    arc_data.pad_angle = 0.008;
    Arc::new()
      .inner_radius(r0.max(0.0))
      .outer_radius(r1.max(r0))
      .paint(&arc_data, color, bounds, window);
  }
}

impl Focusable for ExplorerView {
  fn focus_handle(&self, _: &App) -> FocusHandle {
    self.focus_handle.clone()
  }
}

impl EventEmitter<PanelEvent> for ExplorerView {}

// The component `Panel` inherits the base trait, so the required
// `panel_name` lives on the base impl and the component impl stays empty
// (the pattern gpui-component's own panels use). The name stays "children":
// persisted dock layouts reference it.
impl BasePanel for ExplorerView {
  fn panel_name(&self) -> &'static str {
    "children"
  }
}

impl Panel for ExplorerView {}

impl Render for ExplorerView {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let Some(focus) = self.focus.clone() else {
      return self.render_placeholder(cx);
    };
    let store = self.scan_store.read(cx);
    let total = store
      .index()
      .and_then(|index| index.get(&focus.node_id))
      .map(|entry| entry.size)
      .unwrap_or(0);
    let has_collapsed = self.rows.iter().any(|row| row.collapsed);
    let muted = cx.theme().muted_foreground;
    v_flex()
      .size_full()
      .px(px(28.))
      .pt(px(26.))
      .pb(px(16.))
      .gap(px(14.))
      .child(
        h_flex()
          .gap(px(10.))
          .child(
            Icon::new(IconName::Folder)
              .size(px(26.))
              .text_color(cx.theme().primary),
          )
          .child(
            v_flex()
              .gap(px(2.))
              .child(
                div()
                  .text_size(px(20.))
                  .font_weight(FontWeight::SEMIBOLD)
                  .child(focus.name.clone()),
              )
              .child(div().text_size(px(12.)).text_color(muted).child(format!(
                "{} items · {}",
                format_count(self.rows.len()),
                format_bytes(total)
              ))),
          ),
      )
      .children(has_collapsed.then(|| {
        h_flex()
          .gap(px(6.))
          .text_size(px(11.))
          .text_color(muted)
          .child(Icon::new(IconName::Info).size(px(13.)))
          .child("Some entries are summarized. Open a folder to explore further.")
      }))
      .child(self.render_chart(&focus, cx))
      .child(
        div()
          .border_t_1()
          .border_color(cx.theme().border)
          .pt_2()
          .child(self.render_breadcrumb(cx)),
      )
  }
}
