//! The explorer panel — the web workbench's chart view (plan §3.1): a
//! painted chart of the focused directory beside its contents list, with
//! breadcrumb navigation, a collapsed-entries banner, and the same pastel
//! palette as the web (`session::sunburst`). Five chart families share the
//! panel, matching the shared contract (`docs/chart-modes.md`): the sunburst
//! ring layout, the Burrow-style squarified treemap in its flat and nested
//! densities (`session::treemap`), the icicle partition (`session::icicle`),
//! the packed bubbles (`session::bubbles`), and the ranked strips
//! (`session::strips`). Replaces the old children DataTable: the web UI
//! explores by chart + list, not by a raw table.

use std::{cell::Cell, f32::consts::TAU, rc::Rc};

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::component::plot::shape::{Arc, ArcData};
use gpui_kit::component::{
  h_flex, v_flex, ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _,
};
use gpui_kit::*;

use crate::session::bubbles::{build_bubbles, BubbleCircle};
use crate::session::format::{format_bytes, format_count};
use crate::session::icicle::{
  build_icicle, column_headers, IcicleHeader, IcicleRect, HEADER_HEIGHT,
};
use crate::session::index::SortMode;
use crate::session::plan::StagedPath;
use crate::session::strips::{build_strips, StripRow, StripSegment};
use crate::session::sunburst::{build_sunburst, muted_color, segment_color, SunburstSegment};
use crate::session::treemap::{build_treemap, TreemapRect, LEVEL_PAD_TOP};
use crate::store::{ScanStore, StoreEvent};
use crate::views::theme::{inspector, segment_selected, segment_track, stripe};

/// Ring depth of the painted sunburst, exactly the useful range of the web
/// chart: center, children, grandchildren, one more.
const MAX_DEPTH: u32 = 4;
/// Below ~0.6° a wedge is a hairline; prune it and its subtree.
const MIN_ANGLE: f32 = 0.01;
/// Tiles narrower than this on either axis are invisible noise; prune them
/// and their subtrees with them.
const TREEMAP_MIN_SIDE: f32 = 3.0;
/// Half the painted gap between treemap tiles; each tile is inset by this
/// on every side so neighbors never touch.
const TILE_GAP: f32 = 1.5;
/// Label ink for painted shapes: near-black works on every pastel of the
/// shared palette in both appearances (the sunburst never paints text on its
/// segments; every other family does).
const TILE_LABEL: u32 = 0x23252b;
/// Label ink for ignored entries, which paint over a dark muted grey.
const TILE_LABEL_MUTED: u32 = 0xd6d6dc;
/// Icicle columns run to the same depth as the sunburst rings.
const ICICLE_MAX_DEPTH: u32 = 4;
/// A bubble below this radius is an unreadable dot; the pack drops it.
const BUBBLE_MIN_RADIUS: f32 = 3.0;
/// Half the painted gap between bubbles, so neighbors never touch.
const BUBBLE_GAP: f32 = 1.0;
/// A bubble shows its name from this radius up, and its size too from
/// [`BUBBLE_FULL_RADIUS`] — text only where it cannot spill out of the disc.
const BUBBLE_NAME_RADIUS: f32 = 17.0;
const BUBBLE_FULL_RADIUS: f32 = 34.0;
/// One strip row's height and the gap under it, in canvas pixels.
const STRIP_ROW_HEIGHT: f32 = 24.0;
const STRIP_ROW_GAP: f32 = 4.0;
/// The label gutter at a row's left: the folder name and size live there and
/// the bar fills the rest, so text never sits on top of the composition.
const STRIP_GUTTER_MIN: f32 = 96.0;
const STRIP_GUTTER_MAX: f32 = 180.0;
/// Space between the gutter text and the bar it labels.
const STRIP_GUTTER_GAP: f32 = 10.0;
/// A strip segment shows its name from this painted width up.
const STRIP_SEGMENT_MIN: f32 = 72.0;
/// Icicle rows show their name from this painted width up, and the share too
/// from [`ICICLE_FULL_MIN_W`].
const ICICLE_NAME_MIN_W: f32 = 46.0;
const ICICLE_FULL_MIN_W: f32 = 92.0;
/// Half the painted gap between icicle rows and columns.
const ICICLE_GAP: f32 = 1.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChartMode {
  #[default]
  Sunburst,
  /// Burrow's flat tile view: one layer — the focused folder's direct
  /// children, areas comparable at a glance.
  Flat,
  /// The webpack-bundle-analyzer form: roomy tiles nest their own children
  /// one inside the other, so many layers show at once.
  Nested,
  /// The partition chart: one equal-width column per depth level, a node's
  /// vertical span its share of its parent — a path reads straight across.
  Icicle,
  /// Packed circles, area ∝ size: an overview of who is big, with the focus
  /// itself as a faint backdrop disc.
  Bubbles,
  /// One ranked full-width row per child, each split into its own children:
  /// folder compositions compared without rescaling bars.
  Strips,
}

/// The treemap geometry for the focused node, cached between frames: the
/// layout is rebuilt only when the focus, the flat/nested variant, or the
/// canvas size changed, so painting, hit-testing, and the label overlays
/// all read one geometry.
struct TreemapLayout {
  center_id: String,
  nested: bool,
  width: f32,
  height: f32,
  rects: Vec<TreemapRect>,
}

/// The icicle geometry for the focused node, cached with the column headers
/// derived at build time — the header strips are a function of the painted
/// rects, so recomputing them per frame would be work with one answer.
struct IcicleLayout {
  center_id: String,
  width: f32,
  height: f32,
  rects: Vec<IcicleRect>,
  headers: Vec<IcicleHeader>,
}

/// The bubble pack for the focused node, cached like the treemap: the pack
/// is the expensive layout in the panel (an O(n²) placement search), and a
/// focus, canvas, or radius change is the only thing that invalidates it.
struct BubbleLayout {
  center_id: String,
  width: f32,
  height: f32,
  circles: Vec<BubbleCircle>,
}

/// The ranked strips for the focused node. Rows carry fractions, not pixels,
/// so the cache is keyed on the focus alone: a resize changes how many rows
/// fit on screen, never the layout itself.
struct StripLayout {
  center_id: String,
  rows: Vec<StripRow>,
}

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
  treemap: Option<TreemapLayout>,
  icicle: Option<IcicleLayout>,
  bubbles: Option<BubbleLayout>,
  strips: Option<StripLayout>,
  mode: ChartMode,
  hovered: Option<String>,
  /// The chart canvas's last laid-out bounds, recorded by the canvas's
  /// layout pass every frame; the layout caches and the mouse listeners
  /// read the same cell, so a hover can never disagree with the screen.
  bounds_cell: Rc<Cell<Bounds<Pixels>>>,
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

    // Screenshot/demo hook (like the picker's `SPACLENS_GPUI_AUTO_SCAN`):
    // seed the initial chart mode without any interaction. `treemap` is the
    // legacy spelling of the nested view that hook originally launched.
    let mode = match std::env::var("SPACLENS_GPUI_CHART").as_deref() {
      Ok("flat") => ChartMode::Flat,
      Ok("nested") | Ok("treemap") => ChartMode::Nested,
      Ok("icicle") => ChartMode::Icicle,
      Ok("bubbles") => ChartMode::Bubbles,
      Ok("strips") => ChartMode::Strips,
      _ => ChartMode::default(),
    };

    let mut this = Self {
      scan_store,
      focus: None,
      rows: Vec::new(),
      segments: Vec::new(),
      treemap: None,
      icicle: None,
      bubbles: None,
      strips: None,
      mode,
      hovered: None,
      bounds_cell: Rc::new(Cell::new(Bounds::default())),
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

  /// Rebuilds the row list and the chart geometry for the focused node —
  /// both together, so the chart and the list can never disagree. The layout
  /// caches are dropped here; [`Self::sync_chart_layout`] rebuilds the active
  /// mode's against the canvas size during the next render.
  fn reload(&mut self, cx: &mut Context<Self>) {
    let Some(focus) = self.focus.clone() else {
      self.rows = Vec::new();
      self.segments = Vec::new();
      self.clear_layouts();
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
    self.clear_layouts();
    cx.notify();
  }

  /// Drops every cached layout — the four chart families share the focus and
  /// the canvas, so they all go stale together.
  fn clear_layouts(&mut self) {
    self.treemap = None;
    self.icicle = None;
    self.bubbles = None;
    self.strips = None;
  }

  /// Rebuilds the active mode's cached layout when it is stale — a different
  /// focus, a flat↔nested switch, or a canvas that changed size. Runs during
  /// render against the canvas's previous-frame bounds (the same frame-lag
  /// the sunburst's `bounds_cell` lives with); the surrounding notify cascade
  /// on focus changes and the per-frame resize renders refill it immediately.
  fn sync_chart_layout(&mut self, cx: &Context<Self>) {
    let Some(focus) = self.focus.clone() else {
      self.clear_layouts();
      return;
    };
    let bounds = self.bounds_cell.get();
    let (width, height) = (bounds.size.width.as_f32(), bounds.size.height.as_f32());
    if width <= 0.0 || height <= 0.0 {
      return;
    }
    match self.mode {
      // The sunburst is built in `reload`: it needs no canvas size.
      ChartMode::Sunburst => {}
      ChartMode::Flat | ChartMode::Nested => {
        let nested = self.mode == ChartMode::Nested;
        if matches!(&self.treemap, Some(layout)
          if layout.center_id == focus.node_id
            && layout.nested == nested
            && same_canvas(layout.width, width)
            && same_canvas(layout.height, height))
        {
          return;
        }
        // Flat is the Burrow layer view: depth 1, the focused folder's direct
        // children only. Nested keeps the analyzer-style multi-level inset.
        let depth = if nested { MAX_DEPTH } else { 1 };
        self.treemap = self.scan_store.read(cx).index().map(|index| TreemapLayout {
          center_id: focus.node_id.clone(),
          nested,
          width,
          height,
          rects: build_treemap(
            index,
            &focus.node_id,
            width,
            height,
            depth,
            TREEMAP_MIN_SIDE,
          ),
        });
      }
      ChartMode::Icicle => {
        if matches!(&self.icicle, Some(layout)
          if layout.center_id == focus.node_id
            && same_canvas(layout.width, width)
            && same_canvas(layout.height, height))
        {
          return;
        }
        self.icicle = self.scan_store.read(cx).index().map(|index| {
          let rects = build_icicle(index, &focus.node_id, width, height, ICICLE_MAX_DEPTH);
          IcicleLayout {
            center_id: focus.node_id.clone(),
            width,
            height,
            headers: column_headers(&rects),
            rects,
          }
        });
      }
      ChartMode::Bubbles => {
        if matches!(&self.bubbles, Some(layout)
          if layout.center_id == focus.node_id
            && same_canvas(layout.width, width)
            && same_canvas(layout.height, height))
        {
          return;
        }
        self.bubbles = self.scan_store.read(cx).index().map(|index| BubbleLayout {
          center_id: focus.node_id.clone(),
          width,
          height,
          circles: build_bubbles(index, &focus.node_id, width, height, BUBBLE_MIN_RADIUS),
        });
      }
      ChartMode::Strips => {
        if matches!(&self.strips, Some(layout) if layout.center_id == focus.node_id) {
          return;
        }
        self.strips = self.scan_store.read(cx).index().map(|index| StripLayout {
          center_id: focus.node_id.clone(),
          rows: build_strips(index, &focus.node_id),
        });
      }
    }
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

  /// The chart card: the current mode's canvas with hover/click hit-testing
  /// over a shared geometry, and the summary line under it. The canvas's
  /// layout pass records its bounds into `bounds_cell`, and the mouse
  /// listeners read the same cell — one geometry, shared.
  fn render_chart(&self, focus: &FocusEntry, cx: &Context<Self>) -> Div {
    let total = self
      .scan_store
      .read(cx)
      .index()
      .and_then(|index| index.get(&focus.node_id))
      .map(|entry| entry.size)
      .unwrap_or(0);

    let (chart, summary) = match self.mode {
      ChartMode::Sunburst => (
        self.render_sunburst(focus, total, cx),
        self.sunburst_summary(total),
      ),
      ChartMode::Flat | ChartMode::Nested => (self.render_treemap(cx), self.treemap_summary(total)),
      ChartMode::Icicle => (self.render_icicle(cx), self.icicle_summary(total)),
      ChartMode::Bubbles => (self.render_bubbles(cx), self.bubbles_summary(total)),
      ChartMode::Strips => (self.render_strips(cx), self.strips_summary(total)),
    };
    let foreground = cx.theme().foreground;
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
          .max_w_full()
          .truncate()
          .child(summary),
      )
  }

  fn sunburst_summary(&self, total: u64) -> String {
    match self
      .hovered
      .as_deref()
      .and_then(|id| self.segments.iter().find(|segment| segment.node_id == id))
    {
      Some(segment) => summarize(segment.name.clone(), segment.size, total),
      None => NO_SELECTION_SUMMARY.into(),
    }
  }

  fn treemap_summary(&self, total: u64) -> String {
    match self.hovered.as_deref().and_then(|id| {
      self
        .treemap
        .iter()
        .flat_map(|layout| &layout.rects)
        .find(|rect| rect.node_id == id)
    }) {
      Some(rect) => summarize(rect.name.clone(), rect.size, total),
      None => NO_SELECTION_SUMMARY.into(),
    }
  }

  fn icicle_summary(&self, total: u64) -> String {
    match self.hovered.as_deref().and_then(|id| {
      self
        .icicle
        .iter()
        .flat_map(|layout| &layout.rects)
        .find(|rect| rect.node_id == id)
    }) {
      Some(rect) => summarize_at(&rect.path, rect.size, total),
      None => NO_SELECTION_SUMMARY.into(),
    }
  }

  fn bubbles_summary(&self, total: u64) -> String {
    match self.hovered.as_deref().and_then(|id| {
      self
        .bubbles
        .iter()
        .flat_map(|layout| &layout.circles)
        .find(|circle| circle.node_id == id)
    }) {
      Some(circle) => summarize_at(&circle.path, circle.size, total),
      None => NO_SELECTION_SUMMARY.into(),
    }
  }

  /// The strips summary doubles as the canvas's overflow notice: rows are a
  /// fixed height, so a folder with more children than fit shows the first
  /// rows and says how many stayed below the fold.
  fn strips_summary(&self, total: u64) -> String {
    let Some(layout) = &self.strips else {
      return NO_SELECTION_SUMMARY.into();
    };
    if let Some(found) = self.hovered.as_deref().and_then(|id| {
      layout.rows.iter().find_map(|row| {
        row
          .segments
          .iter()
          .find(|segment| segment.node_id == id)
          .map(|segment| (segment.path.as_str(), segment.size))
          .or_else(|| (row.node_id == id).then_some((row.path.as_str(), row.size)))
      })
    }) {
      return summarize_at(found.0, found.1, total);
    }
    let visible = visible_rows(
      self.bounds_cell.get().size.height.as_f32(),
      layout.rows.len(),
    );
    if visible < layout.rows.len() {
      return format!(
        "{} of {} rows fit — resize the panel to see the rest.",
        visible,
        layout.rows.len()
      );
    }
    NO_SELECTION_SUMMARY.into()
  }

  /// The sunburst variant: ring canvas with the total in the hole, click a
  /// wedge to descend.
  fn render_sunburst(&self, focus: &FocusEntry, total: u64, cx: &Context<Self>) -> Stateful<Div> {
    let paint_segments = self.segments.clone();
    let paint_hover = self.hovered.clone();
    let move_segments = self.segments.clone();
    let move_cell = self.bounds_cell.clone();
    let click_segments = self.segments.clone();
    let click_cell = self.bounds_cell.clone();

    div()
      .relative()
      .flex_1()
      .min_h_0()
      .id("explorer-chart")
      .child(
        canvas(
          {
            let cell = self.bounds_cell.clone();
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
      }))
  }

  /// The treemap variant: Burrow-style squarified tiles — children fill the
  /// whole canvas, roomy tiles show their own children inset inside, hover
  /// dims the other tiles, click a folder tile to descend. Labels are real
  /// overlaid elements (free truncation and the shared font stack), built
  /// from the same cached rects the paint pass and hit-testing read.
  fn render_treemap(&self, cx: &Context<Self>) -> Stateful<Div> {
    let rects = self
      .treemap
      .as_ref()
      .map(|layout| layout.rects.clone())
      .unwrap_or_default();
    let paint_rects = rects.clone();
    let paint_hover = self.hovered.clone();
    let move_rects = rects.clone();
    let move_cell = self.bounds_cell.clone();
    let click_rects = rects;
    let click_cell = self.bounds_cell.clone();

    div()
      .relative()
      .flex_1()
      .min_h_0()
      .id("explorer-chart")
      .child(
        canvas(
          {
            let cell = self.bounds_cell.clone();
            move |bounds: Bounds<Pixels>, _, _| cell.set(bounds)
          },
          move |bounds: Bounds<Pixels>, _, window: &mut Window, _: &mut App| {
            paint_treemap(&paint_rects, paint_hover.as_deref(), &bounds, window);
          },
        )
        .size_full(),
      )
      .children(self.treemap_labels())
      .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, window, cx| {
        let bounds = move_cell.get();
        if bounds.size.width.as_f32() <= 0.0 {
          return;
        }
        let hit = rect_at(&move_rects, &bounds, window.mouse_position()).map(|rect| rect.node_id);
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
        if let Some(rect) = rect_at(&click_rects, &bounds, window.mouse_position()) {
          if rect.has_children {
            this.descend(&rect.node_id, &rect.name, cx);
          }
        }
      }))
  }

  /// The icicle variant: equal-width columns per depth, a node's span its
  /// share of its parent, one caption per column. Hover dims the other rows,
  /// click a row with children to descend into it.
  fn render_icicle(&self, cx: &Context<Self>) -> Stateful<Div> {
    let rects = self
      .icicle
      .as_ref()
      .map(|layout| layout.rects.clone())
      .unwrap_or_default();
    let paint_rects = rects.clone();
    let paint_hover = self.hovered.clone();
    let move_rects = rects.clone();
    let move_cell = self.bounds_cell.clone();
    let click_rects = rects;
    let click_cell = self.bounds_cell.clone();

    div()
      .relative()
      .flex_1()
      .min_h_0()
      .id("explorer-chart")
      .child(
        canvas(
          {
            let cell = self.bounds_cell.clone();
            move |bounds: Bounds<Pixels>, _, _| cell.set(bounds)
          },
          move |bounds: Bounds<Pixels>, _, window: &mut Window, _: &mut App| {
            paint_icicle(&paint_rects, paint_hover.as_deref(), &bounds, window);
          },
        )
        .size_full(),
      )
      .children(self.icicle_labels(cx))
      .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, window, cx| {
        let bounds = move_cell.get();
        if bounds.size.width.as_f32() <= 0.0 {
          return;
        }
        let hit = icicle_at(&move_rects, &bounds, window.mouse_position()).map(|rect| rect.node_id);
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
        if let Some(rect) = icicle_at(&click_rects, &bounds, window.mouse_position()) {
          if rect.has_children {
            this.descend(&rect.node_id, &rect.name, cx);
          }
        }
      }))
  }

  /// The bubbles variant: packed circles with area ∝ size, the focused node
  /// itself a faint backdrop disc behind them. Hover dims the other circles,
  /// click a folder bubble to descend.
  fn render_bubbles(&self, cx: &Context<Self>) -> Stateful<Div> {
    let circles = self
      .bubbles
      .as_ref()
      .map(|layout| layout.circles.clone())
      .unwrap_or_default();
    let paint_circles = circles.clone();
    let paint_hover = self.hovered.clone();
    let move_circles = circles.clone();
    let move_cell = self.bounds_cell.clone();
    let click_circles = circles;
    let click_cell = self.bounds_cell.clone();

    div()
      .relative()
      .flex_1()
      .min_h_0()
      .id("explorer-chart")
      .child(
        canvas(
          {
            let cell = self.bounds_cell.clone();
            move |bounds: Bounds<Pixels>, _, _| cell.set(bounds)
          },
          move |bounds: Bounds<Pixels>, _, window: &mut Window, _: &mut App| {
            paint_bubbles(&paint_circles, paint_hover.as_deref(), &bounds, window);
          },
        )
        .size_full(),
      )
      .children(self.bubble_labels())
      .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, window, cx| {
        let bounds = move_cell.get();
        if bounds.size.width.as_f32() <= 0.0 {
          return;
        }
        let hit =
          bubble_at(&move_circles, &bounds, window.mouse_position()).map(|circle| circle.node_id);
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
        if let Some(circle) = bubble_at(&click_circles, &bounds, window.mouse_position()) {
          if circle.depth > 0 && circle.has_children {
            this.descend(&circle.node_id, &circle.name, cx);
          }
        }
      }))
  }

  /// The strips variant: one ranked row per child, its own children tiling
  /// the bar to the right of the label gutter. Hover highlights whatever is
  /// under the cursor (a row or one segment), click descends into it.
  fn render_strips(&self, cx: &Context<Self>) -> Stateful<Div> {
    let rows = self
      .strips
      .as_ref()
      .map(|layout| layout.rows.clone())
      .unwrap_or_default();
    let paint_rows = rows.clone();
    let paint_hover = self.hovered.clone();
    let move_rows = rows.clone();
    let move_cell = self.bounds_cell.clone();
    let click_rows = rows;
    let click_cell = self.bounds_cell.clone();

    div()
      .relative()
      .flex_1()
      .min_h_0()
      .id("explorer-chart")
      .child(
        canvas(
          {
            let cell = self.bounds_cell.clone();
            move |bounds: Bounds<Pixels>, _, _| cell.set(bounds)
          },
          move |bounds: Bounds<Pixels>, _, window: &mut Window, _: &mut App| {
            paint_strips(&paint_rows, paint_hover.as_deref(), &bounds, window);
          },
        )
        .size_full(),
      )
      .children(self.strip_labels(cx))
      .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, window, cx| {
        let bounds = move_cell.get();
        if bounds.size.width.as_f32() <= 0.0 {
          return;
        }
        let hit = strip_at(&move_rows, &bounds, window.mouse_position()).map(|(row, segment)| {
          segment.map_or_else(|| row.node_id.clone(), |segment| segment.node_id.clone())
        });
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
        if let Some((row, segment)) = strip_at(&click_rows, &bounds, window.mouse_position()) {
          let hit = match segment {
            Some(segment) => (
              segment.node_id.clone(),
              segment.name.clone(),
              segment.has_children,
            ),
            None => (row.node_id.clone(), row.name.clone(), row.has_children),
          };
          if hit.2 {
            this.descend(&hit.0, &hit.1, cx);
          }
        }
      }))
  }

  /// Captions for the icicle: one per column header strip, plus a label for
  /// every row with room for text. The thresholds are the same conservative
  /// ones the treemap uses — a label is only painted where it cannot spill.
  fn icicle_labels(&self, cx: &Context<Self>) -> Vec<Div> {
    let Some(layout) = &self.icicle else {
      return Vec::new();
    };
    let mut labels = Vec::new();
    let caption = cx.theme().muted_foreground;
    for header in &layout.headers {
      let text = if header.w >= 84.0 {
        format!("Level {} · {}", header.depth, header.nodes)
      } else if header.w >= 40.0 {
        format!("L{}", header.depth)
      } else {
        continue;
      };
      labels.push(
        div()
          .absolute()
          .left(px(header.x + 4.))
          .top(px(2.))
          .w(px((header.w - 8.).max(0.)))
          .h(px(HEADER_HEIGHT - 4.))
          .min_w_0()
          .overflow_hidden()
          .child(
            div()
              .truncate()
              .text_size(px(10.))
              .font_weight(FontWeight::MEDIUM)
              .text_color(caption)
              .child(text),
          ),
      );
    }
    let hovered = self.hovered.as_deref();
    for rect in &layout.rects {
      let mut ink = label_ink(rect.ignored);
      if hovered.is_some_and(|id| id != rect.node_id) {
        ink = ink.opacity(0.45);
      }
      let text = if rect.w >= ICICLE_FULL_MIN_W && rect.h >= 15. {
        format!("{} · {:.0}%", rect.name, rect.share * 100.0)
      } else if rect.w >= ICICLE_NAME_MIN_W && rect.h >= 13. {
        rect.name.clone()
      } else {
        continue;
      };
      labels.push(
        div()
          .absolute()
          .left(px(rect.x + 4.))
          .top(px(rect.y + 1.))
          .w(px((rect.w - 8.).max(0.)))
          .h(px((rect.h - 2.).max(0.)))
          .min_w_0()
          .overflow_hidden()
          .child(
            div()
              .truncate()
              .text_size(px(10.))
              .font_weight(FontWeight::MEDIUM)
              .text_color(ink)
              .child(text),
          ),
      );
    }
    labels
  }

  /// Labels for the bubbles: a disc wide enough gets its name, a roomier one
  /// the size and share underneath. The box is the circle's bounding square,
  /// which is exactly as wide as the circle at the line where the text sits,
  /// then truncation keeps it there.
  fn bubble_labels(&self) -> Vec<Div> {
    let Some(layout) = &self.bubbles else {
      return Vec::new();
    };
    let hovered = self.hovered.as_deref();
    // A hover on the backdrop is a hover on the focus, not on a sibling.
    let focus_id = layout.circles.first().map(|circle| circle.node_id.as_str());
    let mut labels = Vec::new();
    for circle in &layout.circles {
      if circle.depth == 0 || circle.r < BUBBLE_NAME_RADIUS {
        continue;
      }
      let mut ink = label_ink(circle.ignored);
      if hovered.is_some_and(|id| id != circle.node_id && Some(id) != focus_id) {
        ink = ink.opacity(0.45);
      }
      let side = 2.0 * circle.r;
      let mut label = v_flex()
        .absolute()
        .left(px(circle.x - circle.r))
        .top(px(circle.y - circle.r))
        .w(px(side))
        .h(px(side))
        .min_w_0()
        .min_h_0()
        .overflow_hidden()
        .items_center()
        .justify_center()
        .gap(px(1.))
        .child(
          div()
            .max_w_full()
            .truncate()
            .text_size(px(11.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(ink)
            .child(circle.name.clone()),
        );
      if circle.r >= BUBBLE_FULL_RADIUS {
        label = label.child(
          div()
            .max_w_full()
            .truncate()
            .text_size(px(10.))
            .text_color(ink.opacity(0.75))
            .child(format!(
              "{} · {:.0}%",
              format_bytes(circle.size),
              circle.share * 100.0
            )),
        );
      }
      labels.push(label);
    }
    labels
  }

  /// Labels for the strip rows that fit: the gutter carries the row's name
  /// and size in theme ink (it sits on the faint track, not on a pastel fill),
  /// and a roomy segment carries its own name in the shared label ink. Same
  /// conservative threshold as the other families — text only where it cannot
  /// spill.
  fn strip_labels(&self, cx: &Context<Self>) -> Vec<Div> {
    let Some(layout) = &self.strips else {
      return Vec::new();
    };
    let bounds = self.bounds_cell.get();
    let width = bounds.size.width.as_f32();
    let gutter = strip_gutter(width);
    let bar_w = (width - gutter).max(0.0);
    let visible = visible_rows(bounds.size.height.as_f32(), layout.rows.len());
    let hovered = self.hovered.as_deref();
    let mut labels = Vec::new();
    for (rank, row) in layout.rows.iter().take(visible).enumerate() {
      let y = rank as f32 * (STRIP_ROW_HEIGHT + STRIP_ROW_GAP);
      let mut row_ink = if row.ignored {
        cx.theme().muted_foreground
      } else {
        cx.theme().foreground
      };
      if hovered.is_some_and(|id| id != row.node_id) {
        row_ink = row_ink.opacity(0.55);
      }
      labels.push(
        v_flex()
          .absolute()
          .left(px(6.))
          .top(px(y + 3.))
          .w(px((gutter - 6. - STRIP_GUTTER_GAP).max(0.)))
          .h(px(STRIP_ROW_HEIGHT - 6.))
          .min_w_0()
          .justify_center()
          .gap(px(1.))
          .child(
            div()
              .max_w_full()
              .truncate()
              .text_size(px(12.))
              .font_weight(FontWeight::MEDIUM)
              .text_color(row_ink)
              .child(row.name.clone()),
          )
          .child(
            div()
              .max_w_full()
              .truncate()
              .text_size(px(10.))
              .text_color(row_ink.opacity(0.72))
              .child(format!(
                "{} · {:.0}%",
                format_bytes(row.size),
                row.share * 100.0
              )),
          ),
      );
      let mut x = gutter;
      for segment in &row.segments {
        let w = segment.width_fraction * bar_w;
        if w >= STRIP_SEGMENT_MIN {
          let mut ink = label_ink(segment.ignored);
          if hovered.is_some_and(|id| id != segment.node_id) {
            ink = ink.opacity(0.45);
          }
          labels.push(
            div()
              .absolute()
              .left(px(x + 5.))
              .top(px(y + 3.))
              .w(px((w - 10.).max(0.)))
              .h(px(STRIP_ROW_HEIGHT - 6.))
              .min_w_0()
              .overflow_hidden()
              .flex()
              .items_center()
              .child(
                div()
                  .max_w_full()
                  .truncate()
                  .text_size(px(11.))
                  .text_color(ink)
                  .child(segment.name.clone()),
              ),
          );
        }
        x += w;
      }
    }
    labels
  }

  /// Labels for the treemap tiles, in one of two shapes. A tile with laid-
  /// out children (the next rect in the pre-order list is one level deeper)
  /// is a frame: its name sits alone in the top padding strip, above the
  /// children. A leaf gets the centered Burrow card — icon, name, size when
  /// the tile fits all three, a bare name when it fits less. Ignored tiles
  /// take light ink; the pastel palette takes near-black. Labels live
  /// inside the chart's listener div, so they never block the hover/click
  /// hit-testing underneath.
  fn treemap_labels(&self) -> Vec<Div> {
    let Some(layout) = &self.treemap else {
      return Vec::new();
    };
    let rects = &layout.rects;
    let hovered_elsewhere = self.hovered.as_deref();
    let mut labels = Vec::new();
    for (ix, rect) in rects.iter().enumerate() {
      let has_painted_children = rects
        .get(ix + 1)
        .is_some_and(|next| next.depth == rect.depth + 1);
      let mut ink = label_ink(rect.ignored);
      if hovered_elsewhere.is_some_and(|id| id != rect.node_id) {
        ink = ink.opacity(0.45);
      }
      if has_painted_children {
        if rect.w >= 64. && rect.h >= 34. {
          labels.push(
            div()
              .absolute()
              .left(px(rect.x + 5.))
              .top(px(rect.y + 2.))
              .w(px((rect.w - 10.).max(0.)))
              .h(px(LEVEL_PAD_TOP - 4.))
              .min_w_0()
              .overflow_hidden()
              .child(
                div()
                  .truncate()
                  .text_size(px(10.))
                  .font_weight(FontWeight::MEDIUM)
                  .text_color(ink)
                  .child(format!("{} · {:.0}%", rect.name, rect.share * 100.0)),
              ),
          );
        }
        continue;
      }
      if rect.w >= 88. && rect.h >= 54. {
        labels.push(
          v_flex()
            .absolute()
            .left(px(rect.x + 6.))
            .top(px(rect.y + 6.))
            .w(px((rect.w - 12.).max(0.)))
            .h(px((rect.h - 12.).max(0.)))
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .items_center()
            .justify_center()
            .gap(px(2.))
            .child(Icon::new(IconName::Folder).size(px(13.)).text_color(ink))
            .child(
              div()
                .max_w_full()
                .truncate()
                .text_size(px(11.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(ink)
                .child(rect.name.clone()),
            )
            .child(
              div()
                .max_w_full()
                .truncate()
                .text_size(px(10.))
                .text_color(ink.opacity(0.75))
                .child(format!(
                  "{} · {:.0}%",
                  format_bytes(rect.size),
                  rect.share * 100.0
                )),
            ),
        );
      } else if rect.w >= 48. && rect.h >= 22. {
        labels.push(
          div()
            .absolute()
            .left(px(rect.x + 6.))
            .top(px(rect.y + 6.))
            .w(px((rect.w - 12.).max(0.)))
            .h(px((rect.h - 12.).max(0.)))
            .min_w_0()
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .child(
              div()
                .max_w_full()
                .truncate()
                .text_size(px(11.))
                .text_color(ink)
                .child(rect.name.clone()),
            ),
        );
      }
    }
    labels
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

  /// The chart-mode segmented control in the panel header — the same
  /// track/selected styling as the inspector tabs, text labels like them
  /// (icon+label buttons would not fit the header row).
  fn render_mode_toggle(&self, cx: &Context<Self>) -> Div {
    let mut toggle = h_flex()
      .gap(px(1.))
      .p(px(2.))
      .rounded(px(6.))
      .bg(segment_track(cx));
    for (mode, label) in [
      (ChartMode::Sunburst, "Sunburst"),
      (ChartMode::Flat, "Flat"),
      (ChartMode::Nested, "Nested"),
      (ChartMode::Icicle, "Icicle"),
      (ChartMode::Bubbles, "Bubbles"),
      (ChartMode::Strips, "Strips"),
    ] {
      let selected = self.mode == mode;
      toggle = toggle.child(
        Button::new(SharedString::from(format!("chart-mode-{label}")))
          .ghost()
          .small()
          .label(label)
          .selected(selected)
          .rounded(px(4.))
          .bg(if selected {
            segment_selected(cx)
          } else {
            segment_track(cx)
          })
          .accessibility_label(format!("Show {label} chart"))
          .on_click(cx.listener(move |this, _, _, cx| {
            if this.mode != mode {
              this.mode = mode;
              this.hovered = None;
              cx.notify();
            }
          })),
      );
    }
    toggle
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

/// The summary line under an idle chart.
const NO_SELECTION_SUMMARY: &str = "Select a folder to explore its contents.";

/// The hover summary shared by the sunburst and treemap: name, size, share of
/// the focused total.
fn summarize(name: String, size: u64, total: u64) -> String {
  let share = if total > 0 {
    size as f32 / total as f32 * 100.0
  } else {
    0.0
  };
  format!("{} · {} · {:.1}%", name, format_bytes(size), share)
}

/// The hover summary for the newer families, which show the full path
/// instead of the bare name: their labels are ellipsized inside tight shapes
/// (a narrow icicle column, a small disc, a short strip segment), so the
/// summary line is where the exact location shows. The path ends in the name.
fn summarize_at(path: &str, size: u64, total: u64) -> String {
  let share = if total > 0 {
    size as f32 / total as f32 * 100.0
  } else {
    0.0
  };
  format!("{} · {} · {:.1}%", path, format_bytes(size), share)
}

/// Label ink for one painted primitive: near-black on the pastel palette,
/// light grey on an ignored entry's muted fill.
fn label_ink(ignored: bool) -> Hsla {
  Hsla::from(rgb(if ignored {
    TILE_LABEL_MUTED
  } else {
    TILE_LABEL
  }))
}

/// The shared pastel fill for one painted primitive — the web `nodeColor`
/// with the muted grey for ignored entries.
fn chart_color(node_id: &str, depth: u32, ignored: bool) -> Hsla {
  Hsla::from(rgb(if ignored {
    muted_color(depth)
  } else {
    segment_color(node_id, depth)
  }))
}

/// Whether a cached layout's canvas matches the current one. Half a pixel is
/// the same canvas: a fractional layout pass must not rebuild the geometry.
fn same_canvas(cached: f32, current: f32) -> bool {
  (cached - current).abs() < 0.5
}

/// The label gutter at a strip row's left. Proportional so a wide panel does
/// not waste space on labels, clamped so a narrow one still shows a name, and
/// shared by the paint pass and hit-testing.
fn strip_gutter(width: f32) -> f32 {
  (width * 0.26).clamp(STRIP_GUTTER_MIN, STRIP_GUTTER_MAX)
}

/// How many strip rows fit a canvas `height`: rows are a fixed height, so the
/// tail stays below the fold rather than shrinking into slivers (the summary
/// line says how many are hidden).
fn visible_rows(height: f32, total: usize) -> usize {
  let fits = ((height + STRIP_ROW_GAP) / (STRIP_ROW_HEIGHT + STRIP_ROW_GAP)).floor();
  (fits.max(0.0) as usize).min(total)
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

/// The tile under `position` (window pixels): the deepest containing rect —
/// nested tiles sit inside their parents, and parents come first in the
/// pre-order rect list. None outside the canvas.
fn rect_at(
  rects: &[TreemapRect],
  bounds: &Bounds<Pixels>,
  position: Point<Pixels>,
) -> Option<TreemapRect> {
  let x = position.x.as_f32() - bounds.origin.x.as_f32();
  let y = position.y.as_f32() - bounds.origin.y.as_f32();
  let mut best: Option<&TreemapRect> = None;
  for rect in rects {
    let inside = rect.x <= x && x < rect.x + rect.w && rect.y <= y && y < rect.y + rect.h;
    if inside && best.is_none_or(|top| rect.depth >= top.depth) {
      best = Some(rect);
    }
  }
  best.cloned()
}

/// Paints every treemap tile: rounded quads inset by a small gap, the web
/// palette (muted for ignored), and non-hovered tiles dimmed while the
/// cursor is over one.
fn paint_treemap(
  rects: &[TreemapRect],
  hovered: Option<&str>,
  bounds: &Bounds<Pixels>,
  window: &mut Window,
) {
  for rect in rects {
    let mut color = chart_color(&rect.node_id, rect.depth, rect.ignored);
    if hovered.is_some_and(|id| id != rect.node_id) {
      color = color.opacity(0.45);
    }
    let tile = Bounds::new(
      point(
        bounds.origin.x + px(rect.x + TILE_GAP),
        bounds.origin.y + px(rect.y + TILE_GAP),
      ),
      size(
        px((rect.w - 2.0 * TILE_GAP).max(0.5)),
        px((rect.h - 2.0 * TILE_GAP).max(0.5)),
      ),
    );
    window.paint_quad(fill(tile, color).corner_radii(px(3.)));
  }
}

/// The icicle row under `position` (window pixels): the deepest containing
/// rect, mirroring [`rect_at`]'s convention so a hover reads the same in
/// every family. Rows of one column do not overlap, so this is mostly a
/// formality — but a pruned sliver can leave a row beside a deeper one, and
/// the deeper one should win.
fn icicle_at(
  rects: &[IcicleRect],
  bounds: &Bounds<Pixels>,
  position: Point<Pixels>,
) -> Option<IcicleRect> {
  let x = position.x.as_f32() - bounds.origin.x.as_f32();
  let y = position.y.as_f32() - bounds.origin.y.as_f32();
  let mut best: Option<&IcicleRect> = None;
  for rect in rects {
    let inside = rect.x <= x && x < rect.x + rect.w && rect.y <= y && y < rect.y + rect.h;
    if inside && best.is_none_or(|top| rect.depth >= top.depth) {
      best = Some(rect);
    }
  }
  best.cloned()
}

/// The circle under `position` (window pixels). Every painted circle lies
/// inside the depth-0 backdrop, so the deepest containing circle wins; the
/// depth-1 circles never overlap each other, so at most one does.
fn bubble_at(
  circles: &[BubbleCircle],
  bounds: &Bounds<Pixels>,
  position: Point<Pixels>,
) -> Option<BubbleCircle> {
  let x = position.x.as_f32() - bounds.origin.x.as_f32();
  let y = position.y.as_f32() - bounds.origin.y.as_f32();
  let mut best: Option<&BubbleCircle> = None;
  for circle in circles {
    let distance = (circle.x - x).hypot(circle.y - y);
    if distance >= circle.r {
      continue;
    }
    let wins = best.is_none_or(|top| {
      circle.depth > top.depth || (circle.depth == top.depth && circle.r < top.r)
    });
    if wins {
      best = Some(circle);
    }
  }
  best.cloned()
}

/// The strip element under `position` (window pixels): the row it belongs to
/// plus, when the cursor is over the bar rather than the label gutter, the
/// segment under it. Rows stack in rank order at [`STRIP_ROW_HEIGHT`] plus
/// [`STRIP_ROW_GAP`], and the bar starts after [`strip_gutter`] — the same
/// geometry the paint pass used.
fn strip_at<'a>(
  rows: &'a [StripRow],
  bounds: &Bounds<Pixels>,
  position: Point<Pixels>,
) -> Option<(&'a StripRow, Option<&'a StripSegment>)> {
  let x = position.x.as_f32() - bounds.origin.x.as_f32();
  let y = position.y.as_f32() - bounds.origin.y.as_f32();
  let width = bounds.size.width.as_f32();
  if x < 0.0 || x >= width || y < 0.0 {
    return None;
  }
  let rank = (y / (STRIP_ROW_HEIGHT + STRIP_ROW_GAP)).floor() as usize;
  let row = rows.get(rank)?;
  let row_y = rank as f32 * (STRIP_ROW_HEIGHT + STRIP_ROW_GAP);
  if y >= row_y + STRIP_ROW_HEIGHT {
    return None; // the gap between two rows belongs to neither
  }
  let bar_x = strip_gutter(width);
  let bar_w = (width - bar_x).max(0.0);
  if x < bar_x || row.segments.is_empty() {
    return Some((row, None));
  }
  let mut cursor = bar_x;
  for segment in &row.segments {
    let w = segment.width_fraction * bar_w;
    if x >= cursor && x < cursor + w {
      return Some((row, Some(segment)));
    }
    cursor += w;
  }
  Some((row, None))
}

/// Paints every icicle row: rounded quads inset by a small gap from the
/// column geometry, the web palette (muted for ignored), and non-hovered
/// rows dimmed while the cursor is over one. The headers are text only, so
/// they are painted by the label overlay, not here.
fn paint_icicle(
  rects: &[IcicleRect],
  hovered: Option<&str>,
  bounds: &Bounds<Pixels>,
  window: &mut Window,
) {
  for rect in rects {
    let mut color = chart_color(&rect.node_id, rect.depth, rect.ignored);
    if hovered.is_some_and(|id| id != rect.node_id) {
      color = color.opacity(0.45);
    }
    let tile = Bounds::new(
      point(
        bounds.origin.x + px(rect.x + ICICLE_GAP),
        bounds.origin.y + px(rect.y + ICICLE_GAP),
      ),
      size(
        px((rect.w - 2.0 * ICICLE_GAP).max(0.5)),
        px((rect.h - 2.0 * ICICLE_GAP).max(0.5)),
      ),
    );
    window.paint_quad(fill(tile, color).corner_radii(px(2.)));
  }
}

/// Paints the bubble pack: the depth-0 backdrop first, faint so it reads as
/// a body rather than a disc competing with its children, then every child
/// circle shrunk by a hair so neighbors keep a visible seam. Non-hovered
/// circles dim while the cursor is over one.
fn paint_bubbles(
  circles: &[BubbleCircle],
  hovered: Option<&str>,
  bounds: &Bounds<Pixels>,
  window: &mut Window,
) {
  // Hovering the backdrop is hovering the focus itself, not a sibling, so it
  // must not gray out the children the backdrop contains.
  let focus_id = circles.first().map(|circle| circle.node_id.as_str());
  for circle in circles {
    let mut color = chart_color(&circle.node_id, circle.depth, circle.ignored);
    let radius = if circle.depth == 0 {
      color = color.opacity(0.12);
      circle.r
    } else {
      if hovered.is_some_and(|id| id != circle.node_id && Some(id) != focus_id) {
        color = color.opacity(0.45);
      }
      (circle.r - BUBBLE_GAP).max(0.5)
    };
    // A square with half-side corner radii is a disc in the quad shader.
    let disc = Bounds::new(
      point(
        bounds.origin.x + px(circle.x - radius),
        bounds.origin.y + px(circle.y - radius),
      ),
      size(px(2.0 * radius), px(2.0 * radius)),
    );
    window.paint_quad(fill(disc, color).corner_radii(px(radius)));
  }
}

/// Paints the strip rows: each row's own color as a faint track across the
/// full width (so the ranking and the row labels have an anchor even where a
/// row has one child), then the row's children as solid segments over the
/// bar. Non-hovered primitives dim while the cursor is over one.
fn paint_strips(
  rows: &[StripRow],
  hovered: Option<&str>,
  bounds: &Bounds<Pixels>,
  window: &mut Window,
) {
  let width = bounds.size.width.as_f32();
  let bar_x = strip_gutter(width);
  let bar_w = (width - bar_x).max(0.0);
  let visible = visible_rows(bounds.size.height.as_f32(), rows.len());
  for (rank, row) in rows.iter().take(visible).enumerate() {
    let y = rank as f32 * (STRIP_ROW_HEIGHT + STRIP_ROW_GAP);
    let mut track = chart_color(&row.node_id, row.depth, row.ignored).opacity(0.16);
    if hovered.is_some_and(|id| id != row.node_id) {
      track = track.opacity(0.06);
    }
    let row_bounds = Bounds::new(
      point(bounds.origin.x, bounds.origin.y + px(y)),
      size(px(width), px(STRIP_ROW_HEIGHT)),
    );
    window.paint_quad(fill(row_bounds, track).corner_radii(px(4.)));
    if row.segments.is_empty() {
      // A leaf (or an all-zero-size parent): one solid bar in its own color.
      let bar = Bounds::new(
        point(bounds.origin.x + px(bar_x), bounds.origin.y + px(y + 3.)),
        size(px(bar_w), px(STRIP_ROW_HEIGHT - 6.)),
      );
      window.paint_quad(
        fill(bar, chart_color(&row.node_id, row.depth, row.ignored)).corner_radii(px(3.)),
      );
      continue;
    }
    let mut x = bar_x;
    for segment in &row.segments {
      let w = segment.width_fraction * bar_w;
      let mut color = chart_color(&segment.node_id, segment.depth, segment.ignored);
      if hovered.is_some_and(|id| id != segment.node_id) {
        color = color.opacity(0.45);
      }
      let tile = Bounds::new(
        point(bounds.origin.x + px(x + 0.5), bounds.origin.y + px(y + 3.)),
        size(px((w - 1.0).max(0.5)), px(STRIP_ROW_HEIGHT - 6.)),
      );
      window.paint_quad(fill(tile, color).corner_radii(px(3.)));
      x += w;
    }
  }
}

#[cfg(test)]
mod treemap_hit_tests {
  use super::rect_at;
  use crate::session::treemap::TreemapRect;
  use gpui_kit::{point, px, size, Bounds};

  fn rect(name: &str, depth: u32, x: f32, y: f32, w: f32, h: f32) -> TreemapRect {
    TreemapRect {
      node_id: name.into(),
      name: name.into(),
      depth,
      x,
      y,
      w,
      h,
      size: 1,
      share: 0.5,
      ignored: false,
      has_children: false,
    }
  }

  #[test]
  fn the_deepest_tile_wins_and_canvas_edges_are_exclusive() {
    let tiles = vec![
      rect("parent", 1, 0., 0., 100., 100.),
      rect("child", 2, 12., 12., 76., 76.),
    ];
    let bounds = Bounds::new(point(px(10.), px(10.)), size(px(100.), px(100.)));
    let at = |x: f32, y: f32| rect_at(&tiles, &bounds, point(px(x), px(y))).map(|tile| tile.name);
    // Window pixels: the canvas sits at (10, 10), so (20, 20) is local (10, 10).
    assert_eq!(at(20., 20.).as_deref(), Some("parent"));
    assert_eq!(at(40., 40.).as_deref(), Some("child"));
    assert_eq!(at(9., 40.), None); // left of the canvas
    assert_eq!(at(40., 110.), None); // below the canvas

    // The top-right corner strip belongs to the parent, not the child.
    assert_eq!(at(105., 15.).as_deref(), Some("parent"));
  }
}

#[cfg(test)]
mod icicle_hit_tests {
  use super::{icicle_at, ICICLE_MAX_DEPTH};
  use crate::session::icicle::IcicleRect;
  use gpui_kit::{point, px, size, Bounds};

  fn rect(name: &str, depth: u32, x: f32, y: f32, w: f32, h: f32) -> IcicleRect {
    IcicleRect {
      node_id: name.into(),
      name: name.into(),
      path: format!("/{name}"),
      depth,
      x,
      y,
      w,
      h,
      size: 1,
      share: 0.5,
      ignored: false,
      has_children: false,
    }
  }

  #[test]
  fn rows_hit_test_by_column_and_canvas_edges_are_exclusive() {
    let rows = vec![
      rect("big", 1, 0., 18., 100., 80.),
      rect("small", 1, 0., 98., 100., 20.),
      rect("inner", 2, 103., 18., 100., 80.),
    ];
    let bounds = Bounds::new(point(px(10.), px(10.)), size(px(300.), px(300.)));
    let at = |x: f32, y: f32| icicle_at(&rows, &bounds, point(px(x), px(y))).map(|r| r.name);
    // Window pixels: the canvas sits at (10, 10), so (20, 30) is local (10, 20).
    assert_eq!(at(20., 30.).as_deref(), Some("big"));
    assert_eq!(at(20., 118.).as_deref(), Some("small")); // local (10, 108)
    assert_eq!(at(150., 30.).as_deref(), Some("inner"));
    assert_eq!(at(9., 30.), None); // left of the canvas
    assert_eq!(at(20., 320.), None); // below the canvas
    assert_eq!(at(150., 305.), None); // the column gap belongs to no row
    assert!(ICICLE_MAX_DEPTH > 1);
  }
}

#[cfg(test)]
mod bubble_hit_tests {
  use super::bubble_at;
  use crate::session::bubbles::BubbleCircle;
  use gpui_kit::{point, px, size, Bounds};

  fn circle(name: &str, depth: u32, x: f32, y: f32, r: f32) -> BubbleCircle {
    BubbleCircle {
      node_id: name.into(),
      name: name.into(),
      path: format!("/{name}"),
      depth,
      x,
      y,
      r,
      size: 1,
      share: 0.5,
      ignored: false,
      has_children: false,
    }
  }

  #[test]
  fn the_deepest_containing_circle_wins_and_edges_are_exclusive() {
    let circles = vec![
      circle("focus", 0, 50., 50., 50.),
      circle("big", 1, 40., 50., 20.),
      circle("small", 1, 80., 20., 10.),
    ];
    let bounds = Bounds::new(point(px(10.), px(10.)), size(px(100.), px(100.)));
    let at = |x: f32, y: f32| bubble_at(&circles, &bounds, point(px(x), px(y))).map(|c| c.name);
    // Local (40, 50) is inside both the backdrop and `big`: the child wins.
    assert_eq!(at(50., 60.).as_deref(), Some("big"));
    // Local (15, 15) is only inside the backdrop.
    assert_eq!(at(25., 25.).as_deref(), Some("focus"));
    assert_eq!(at(9., 60.), None); // left of the canvas
    assert_eq!(at(50., 111.), None); // below the canvas
                                     // Exactly on the rim is outside (the hit test is exclusive).
    assert_eq!(at(110., 60.), None);
  }
}

#[cfg(test)]
mod strip_hit_tests {
  use super::{strip_at, strip_gutter, STRIP_ROW_GAP, STRIP_ROW_HEIGHT};
  use crate::session::strips::{StripRow, StripSegment};
  use gpui_kit::{point, px, size, Bounds};

  fn segment(name: &str, width_fraction: f32) -> StripSegment {
    StripSegment {
      node_id: name.into(),
      name: name.into(),
      path: format!("/{name}"),
      depth: 2,
      size: 1,
      width_fraction,
      ignored: false,
      has_children: false,
    }
  }

  fn row(name: &str, segments: Vec<StripSegment>) -> StripRow {
    StripRow {
      node_id: name.into(),
      name: name.into(),
      path: format!("/{name}"),
      depth: 1,
      size: 1,
      share: 0.5,
      ignored: false,
      has_children: !segments.is_empty(),
      segments,
    }
  }

  #[test]
  fn rows_split_into_segments_and_the_gutter_belongs_to_the_row() {
    let rows = vec![
      row("first", vec![segment("a", 0.25), segment("b", 0.75)]),
      row("second", vec![]),
    ];
    let width = 400.0;
    let bounds = Bounds::new(point(px(0.), px(0.)), size(px(width), px(200.)));
    let gutter = strip_gutter(width);
    assert!(gutter > 0.0 && gutter < width);
    let bar = width - gutter;
    let at = |x: f32, y: f32| {
      strip_at(&rows, &bounds, point(px(x), px(y)))
        .map(|(row, segment)| (row.name.clone(), segment.map(|s| s.name.clone())))
    };
    // The gutter belongs to the row, not to whichever segment sits first.
    assert_eq!(at(gutter - 4.0, 6.0), Some(("first".into(), None)));
    // 0.25 of the bar is `a`, the rest `b`.
    assert_eq!(
      at(gutter + bar * 0.1, 6.0),
      Some(("first".into(), Some("a".into())))
    );
    assert_eq!(
      at(gutter + bar * 0.5, 6.0),
      Some(("first".into(), Some("b".into())))
    );
    // The second row has no segments: the whole bar is the row's own bar.
    let second_y = STRIP_ROW_HEIGHT + STRIP_ROW_GAP + 6.0;
    assert_eq!(at(gutter + 30.0, second_y), Some(("second".into(), None)));
    // The gap between the two rows belongs to neither.
    assert_eq!(at(gutter + 30.0, STRIP_ROW_HEIGHT + 1.0), None);
    // Below the last row there is nothing to hit.
    assert_eq!(at(gutter + 30.0, 400.0), None);
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
    if matches!(
      self.mode,
      ChartMode::Flat
        | ChartMode::Nested
        | ChartMode::Icicle
        | ChartMode::Bubbles
        | ChartMode::Strips
    ) {
      self.sync_chart_layout(cx);
    }
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
          )
          .child(div().flex_1())
          .child(self.render_mode_toggle(cx)),
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
