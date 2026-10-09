//! Icicle geometry over the scan index — the partition counterpart to
//! `session::sunburst` and `session::treemap`: every depth level gets its own
//! equal-width column, columns advance left to right, and a node's vertical
//! span inside its column is its share of its parent's span. The payoff is
//! that a path down the hierarchy is a straight horizontal walk — parent,
//! child, and grandchild all start at the same y — where the sunburst makes
//! you follow angles and the treemap makes you shrink into insets. Pure data
//! and math (`docs/chart-modes.md` is the shared contract); the view paints
//! it and hit-tests it from the same rects.

use crate::session::index::{ScanIndex, SortMode};

/// Height of the per-level column header strip along the top of the canvas.
/// The plot proper starts below it, so a header never overlaps a row.
pub const HEADER_HEIGHT: f32 = 18.0;
/// Horizontal gap between two depth columns — without it, neighboring
/// columns read as one wide band instead of two levels.
pub const COLUMN_GAP: f32 = 3.0;
/// Vertical gap between two rows of one column. Rows tile their parent's
/// span exactly *before* this inset, so the gap is bookkeeping, not space
/// stolen from the children.
pub const ROW_GAP: f32 = 1.0;
/// A row slot thinner than this is invisible noise; it is pruned together
/// with its whole subtree. The painted box is [`ROW_GAP`] shorter than the
/// slot, so painted rows are never degenerate either.
const MIN_ROW_HEIGHT: f32 = 3.0;
/// A column narrower than this cannot be read at all; the canvas shows
/// fewer levels rather than slivers of every level.
const MIN_COLUMN_WIDTH: f32 = 8.0;

/// One painted row of the icicle. Coordinates are canvas-local pixels — the
/// caller picks the canvas size, so hit-testing and painting share one
/// geometry without scaling.
#[derive(Clone, Debug)]
pub struct IcicleRect {
  pub node_id: String,
  pub name: String,
  /// Full path, carried for the hover summary: a row is often too narrow
  /// for its name, and the summary is where the exact location shows.
  pub path: String,
  /// 1-based nesting level: 1 = direct children of the center node. All
  /// rects of one depth share the same `x` and `w`.
  pub depth: u32,
  pub x: f32,
  pub y: f32,
  pub w: f32,
  pub h: f32,
  pub size: u64,
  /// Share of the parent's tiled span the row fills — the sibling sum the
  /// layout normalized against (child size ÷ the sized siblings' total).
  /// The painted height is exactly `share` of the parent's span.
  pub share: f32,
  pub ignored: bool,
  pub has_children: bool,
}

/// One depth column's header strip, for the caption above the column. The
/// headers are derived from the painted rects, so a level that got pruned
/// (or never had room) carries no header and leaves no empty caption.
#[derive(Clone, Debug, PartialEq)]
pub struct IcicleHeader {
  pub depth: u32,
  pub x: f32,
  pub w: f32,
  /// How many rows the column holds.
  pub nodes: usize,
}

/// A placement slot in the recursion: the column's `x`/`w` plus the
/// parent's vertical span the children tile.
#[derive(Clone, Copy, Debug)]
struct Slot {
  x: f32,
  w: f32,
  y0: f32,
  y1: f32,
}

/// Builds every row for `center_id`'s subtree across a `width` × `height`
/// canvas. Depth 1 tiles the plot area (the canvas minus the header strip)
/// among the center's children; each child's span becomes the slot its own
/// children tile in the next column, and so on to `max_depth`. Children are
/// normalized against each other — their sizes need not sum to the parent's,
/// because summarized entries make them differ — so rows tile their parent
/// exactly and never leave gaps or overflow. Zero-size entries are skipped,
/// and any row slot shorter than [`MIN_ROW_HEIGHT`] is pruned together with
/// its subtree: sub-pixel rows cost render time for nothing.
pub fn build_icicle(
  index: &ScanIndex,
  center_id: &str,
  width: f32,
  height: f32,
  max_depth: u32,
) -> Vec<IcicleRect> {
  let mut out = Vec::new();
  let plot_height = height - HEADER_HEIGHT;
  if width < MIN_COLUMN_WIDTH || plot_height < MIN_ROW_HEIGHT || max_depth == 0 {
    return out;
  }
  // Equal-width columns mean the canvas can only carry as many levels as it
  // can give MIN_COLUMN_WIDTH each; a narrow panel shows its top levels
  // instead of unreadable slivers of all of them.
  let levels = structural_depth(index, center_id, max_depth).min(max_columns_that_fit(width));
  if levels == 0 {
    return out;
  }
  let column_width = (width - COLUMN_GAP * (levels - 1) as f32) / levels as f32;
  layout_children(
    index,
    center_id,
    1,
    Slot {
      x: 0.0,
      w: column_width,
      y0: HEADER_HEIGHT,
      y1: height,
    },
    levels,
    &mut out,
  );
  out
}

/// The per-level header strips for a built icicle, in depth order. One header
/// per depth that actually painted rows: `x`/`w` are that column's extent and
/// `nodes` its row count.
pub fn column_headers(rects: &[IcicleRect]) -> Vec<IcicleHeader> {
  let mut headers: Vec<IcicleHeader> = Vec::new();
  for rect in rects {
    match headers.iter_mut().find(|header| header.depth == rect.depth) {
      Some(header) => {
        let right = header.x + header.w;
        header.x = header.x.min(rect.x);
        header.w = right.max(rect.x + rect.w) - header.x;
        header.nodes += 1;
      }
      None => headers.push(IcicleHeader {
        depth: rect.depth,
        x: rect.x,
        w: rect.w,
        nodes: 1,
      }),
    }
  }
  headers.sort_by_key(|header| header.depth);
  headers
}

fn layout_children(
  index: &ScanIndex,
  node_id: &str,
  depth: u32,
  slot: Slot,
  max_depth: u32,
  out: &mut Vec<IcicleRect>,
) {
  if depth > max_depth {
    return;
  }
  let Some(children) = index.children(node_id, SortMode::Size) else {
    return;
  };
  let total: u64 = children.iter().map(|child| child.size).sum();
  if total == 0 || slot.y1 <= slot.y0 {
    return;
  }
  let span = slot.y1 - slot.y0;
  let mut cursor = slot.y0;
  for child in children {
    let extent = child.size as f32 / total as f32 * span;
    // The cursor advances for every child, painted or not: dropping a sliver
    // must not shift its siblings off the parent's tile grid.
    if child.size > 0 && extent >= MIN_ROW_HEIGHT {
      let size = child.size;
      let share = size as f32 / total as f32;
      out.push(IcicleRect {
        node_id: child.id.clone(),
        name: child.name.clone(),
        path: child.path.clone(),
        depth,
        x: slot.x,
        y: cursor + ROW_GAP * 0.5,
        w: slot.w,
        h: extent - ROW_GAP,
        size,
        share,
        ignored: child.ignored,
        has_children: !child.child_ids.is_empty(),
      });
      if depth < max_depth && !child.child_ids.is_empty() {
        layout_children(
          index,
          &child.id,
          depth + 1,
          Slot {
            x: slot.x + slot.w + COLUMN_GAP,
            w: slot.w,
            y0: cursor + ROW_GAP * 0.5,
            y1: cursor + extent - ROW_GAP * 0.5,
          },
          max_depth,
          out,
        );
      }
    }
    cursor += extent;
  }
}

/// How many depth levels below `node_id` carry a sized node, capped at
/// `limit`. This is the structural depth, not the painted one — pruning can
/// still cut a level short when its rows end up thinner than the minimum,
/// and the extra column then stays empty.
fn structural_depth(index: &ScanIndex, node_id: &str, limit: u32) -> u32 {
  if limit == 0 {
    return 0;
  }
  let Some(children) = index.children(node_id, SortMode::Size) else {
    return 0;
  };
  let mut deepest = u32::from(children.iter().any(|child| child.size > 0));
  if limit > 1 {
    for child in children {
      if child.size == 0 || child.child_ids.is_empty() {
        continue;
      }
      deepest = deepest.max(1 + structural_depth(index, &child.id, limit - 1));
    }
  }
  deepest
}

/// The most equal-width columns of at least [`MIN_COLUMN_WIDTH`] that fit
/// `width`, always at least one.
fn max_columns_that_fit(width: f32) -> u32 {
  let fits = ((width + COLUMN_GAP) / (MIN_COLUMN_WIDTH + COLUMN_GAP)).floor();
  (fits as u32).max(1)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::session::index::node_id_of;
  use space_lens::ScanNode;

  fn node(path: &str, size: u64, children: Vec<ScanNode>) -> ScanNode {
    ScanNode {
      name: String::new(),
      path: path.into(),
      size,
      children,
      depth: 0,
      ignored: false,
      collapsed: false,
    }
  }

  fn tree() -> ScanIndex {
    ScanIndex::new(&[node(
      "/proj",
      100,
      vec![
        node("/proj/big", 60, vec![node("/proj/big/inner", 10, vec![])]),
        node("/proj/small", 20, vec![]),
        // A zero-size entry must be skipped, not poison the column.
        node("/proj/empty", 0, vec![]),
      ],
    )])
  }

  fn layout(index: &ScanIndex, width: f32, height: f32) -> Vec<IcicleRect> {
    let root_id = index.root_ids()[0].clone();
    build_icicle(index, &root_id, width, height, 4)
  }

  fn overlap(a: &IcicleRect, b: &IcicleRect) -> f32 {
    let w = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
    let h = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
    w.max(0.0) * h.max(0.0)
  }

  #[test]
  fn columns_are_equal_width_and_advance_by_one_column() {
    let index = tree();
    let rects = layout(&index, 800.0, 600.0);
    let column = |depth: u32| -> Vec<&IcicleRect> {
      rects.iter().filter(|rect| rect.depth == depth).collect()
    };
    let first = column(1);
    let second = column(2);
    assert_eq!(first.len(), 2); // `empty` is zero-size and skipped
    assert_eq!(second.len(), 1);
    for rect in first.iter().chain(second.iter()) {
      assert_eq!(rect.w, first[0].w);
    }
    // Two levels share the canvas equally, separated by exactly one gap.
    assert!((2.0 * first[0].w + COLUMN_GAP - 800.0).abs() < 0.01);
    assert!((second[0].x - (first[0].x + first[0].w + COLUMN_GAP)).abs() < 0.01);
    // Every row of a column starts at the same x.
    assert!(first.iter().all(|rect| rect.x == first[0].x));
  }

  #[test]
  fn a_nodes_span_is_its_share_of_its_parent() {
    let index = tree();
    let rects = layout(&index, 800.0, 600.0);
    let plot = 600.0 - HEADER_HEIGHT;
    let big = rects.iter().find(|rect| rect.name == "big").unwrap();
    let small = rects.iter().find(|rect| rect.name == "small").unwrap();
    // 60 : 20 of the 80 sized siblings — 75% / 25% of the plot's span. The
    // painted box plus its gap is the slot the share describes.
    assert!((big.h + ROW_GAP - 0.75 * plot).abs() < 0.01);
    assert!((small.h + ROW_GAP - 0.25 * plot).abs() < 0.01);
    assert!((big.share - 0.75).abs() < 1e-4);
    assert!((small.share - 0.25).abs() < 1e-4);
  }

  #[test]
  fn children_stay_inside_their_parents_span_in_the_next_column() {
    let index = tree();
    let rects = layout(&index, 800.0, 600.0);
    let big = rects.iter().find(|rect| rect.name == "big").unwrap();
    let inner = rects.iter().find(|rect| rect.name == "inner").unwrap();
    assert_eq!(inner.depth, 2);
    assert!(inner.x >= big.x + big.w + COLUMN_GAP - 0.01);
    // The child column is exactly one column over and just as wide.
    assert!((inner.x - (big.x + big.w + COLUMN_GAP)).abs() < 0.01);
    assert!((inner.w - big.w).abs() < 0.01);
    assert!(inner.y >= big.y - 0.01);
    assert!(inner.y + inner.h <= big.y + big.h + 0.01);
  }

  #[test]
  fn siblings_tile_their_parent_without_overlap_or_drift() {
    let index = ScanIndex::new(&[node(
      "/r",
      1000,
      vec![
        node(
          "/r/a",
          500,
          vec![node("/r/a/x", 300, vec![]), node("/r/a/y", 200, vec![])],
        ),
        node("/r/b", 300, vec![]),
        node("/r/c", 200, vec![]),
      ],
    )]);
    let rects = layout(&index, 900.0, 500.0);
    let top: Vec<_> = rects.iter().filter(|rect| rect.depth == 1).collect();
    assert_eq!(top.len(), 3);
    // Sorted by size, so the slots run a, b, c top to bottom.
    assert_eq!(top[0].name, "a");
    for (a, b) in top.iter().zip(top.iter().skip(1)) {
      assert!(overlap(a, b) < 0.01, "{a:?} overlaps {b:?}");
      assert!((b.y - (a.y + a.h) - ROW_GAP).abs() < 0.01);
    }
    let plot = 500.0 - HEADER_HEIGHT;
    let spanned: f32 = top.iter().map(|rect| rect.h + ROW_GAP).sum();
    assert!((spanned - plot).abs() < 0.05);
    // The children of `a` tile exactly its painted span.
    let inner: Vec<_> = rects.iter().filter(|rect| rect.depth == 2).collect();
    assert_eq!(inner.len(), 2);
    let sum: f32 = inner.iter().map(|rect| rect.h + ROW_GAP).sum();
    assert!((sum - top[0].h).abs() < 0.05);
    assert!(inner[0].y >= top[0].y - 0.01);
    assert!(inner[1].y + inner[1].h <= top[0].y + top[0].h + 0.01);
  }

  #[test]
  fn zero_size_entries_are_skipped_and_never_degenerate() {
    let index = tree();
    let rects = layout(&index, 800.0, 600.0);
    assert!(rects.iter().all(|rect| rect.name != "empty"));
    for rect in &rects {
      assert!(rect.size > 0);
      assert!(rect.w > 0.0 && rect.h >= MIN_ROW_HEIGHT - ROW_GAP - 0.01);
      assert!(rect.x >= 0.0 && rect.y >= HEADER_HEIGHT - 0.01);
      assert!(rect.x + rect.w <= 800.01 && rect.y + rect.h <= 600.01);
    }
  }

  #[test]
  fn a_single_child_takes_the_whole_span() {
    let index = ScanIndex::new(&[node(
      "/r",
      40,
      vec![node("/r/only", 40, vec![node("/r/only/leaf", 40, vec![])])],
    )]);
    let rects = layout(&index, 600.0, 300.0);
    let only = rects.iter().find(|rect| rect.name == "only").unwrap();
    let leaf = rects.iter().find(|rect| rect.name == "leaf").unwrap();
    assert!((only.share - 1.0).abs() < 1e-4);
    assert!((only.h - (300.0 - HEADER_HEIGHT - ROW_GAP)).abs() < 0.01);
    assert!((leaf.share - 1.0).abs() < 1e-4);
    assert!(leaf.y >= only.y - 0.01 && leaf.y + leaf.h <= only.y + only.h + 0.01);
  }

  #[test]
  fn a_deep_chain_keeps_every_painted_row_above_the_minimum() {
    // Sixty levels of one child each: the canvas can only give the top ones
    // a readable column, and whatever is painted must still be a real row.
    let mut current = node("/deep/l60", 1_000, vec![]);
    for level in (0..60).rev() {
      current = node(&format!("/deep/l{level}"), 1_000, vec![current]);
    }
    let index = ScanIndex::new(&[current]);
    let root_id = index.root_ids()[0].clone();
    let rects = build_icicle(&index, &root_id, 1000.0, 700.0, 60);
    assert!(!rects.is_empty());
    assert!(rects
      .iter()
      .all(|rect| rect.h >= MIN_ROW_HEIGHT - ROW_GAP - 0.01));
    // Columns are equal width, so only what fits was painted.
    let depths: Vec<u32> = rects.iter().map(|rect| rect.depth).collect();
    assert!(depths.len() <= max_columns_that_fit(1000.0) as usize);
  }

  #[test]
  fn max_depth_caps_the_columns() {
    let index = tree();
    let root_id = index.root_ids()[0].clone();
    let flat = build_icicle(&index, &root_id, 800.0, 600.0, 1);
    assert!(flat.iter().all(|rect| rect.depth == 1));
    assert!(!flat.iter().any(|rect| rect.name == "inner"));
    let two = build_icicle(&index, &root_id, 800.0, 600.0, 2);
    assert!(two
      .iter()
      .any(|rect| rect.name == "inner" && rect.depth == 2));
  }

  #[test]
  fn a_canvas_with_no_room_paints_nothing() {
    let index = tree();
    let root_id = index.root_ids()[0].clone();
    assert!(build_icicle(&index, &root_id, 0.0, 0.0, 4).is_empty());
    assert!(build_icicle(&index, &root_id, 4.0, 600.0, 4).is_empty());
    assert!(build_icicle(&index, &root_id, 800.0, HEADER_HEIGHT, 4).is_empty());
    assert!(build_icicle(&index, &root_id, 800.0, 600.0, 0).is_empty());
    // A leaf focus has no children to paint.
    let leaf = ScanIndex::new(&[node("/leaf", 10, vec![])]);
    assert!(build_icicle(&leaf, &leaf.root_ids()[0], 800.0, 600.0, 4).is_empty());
    // An unknown focus is not a panic either.
    assert!(build_icicle(&index, "missing", 800.0, 600.0, 4).is_empty());
  }

  #[test]
  fn headers_describe_each_painted_column() {
    let index = tree();
    let rects = layout(&index, 800.0, 600.0);
    let headers = column_headers(&rects);
    assert_eq!(headers.len(), 2);
    assert_eq!(headers[0].depth, 1);
    assert_eq!(headers[0].nodes, 2);
    assert_eq!(headers[1].depth, 2);
    assert_eq!(headers[1].nodes, 1);
    for header in &headers {
      assert!(header.x >= 0.0 && header.x + header.w <= 800.01);
      assert!(header.w > 0.0);
    }
    assert!(headers[1].x > headers[0].x);
    assert!(column_headers(&[]).is_empty());
  }

  #[test]
  fn rects_carry_ids_paths_flags_and_shares() {
    let index = tree();
    let rects = layout(&index, 800.0, 600.0);
    let big = rects.iter().find(|rect| rect.name == "big").unwrap();
    assert_eq!(big.node_id, node_id_of("/proj/big"));
    assert_eq!(big.path, "/proj/big");
    assert_eq!(big.size, 60);
    assert!(big.has_children);
    let small = rects.iter().find(|rect| rect.name == "small").unwrap();
    assert!(!small.has_children);
    assert_eq!(small.path, "/proj/small");

    let mut flagged = node("/proj/quiet", 20, vec![]);
    flagged.ignored = true;
    let index = ScanIndex::new(&[node(
      "/proj",
      120,
      vec![node("/proj/loud", 100, vec![]), flagged],
    )]);
    let rects = layout(&index, 800.0, 600.0);
    let quiet = rects.iter().find(|rect| rect.name == "quiet").unwrap();
    assert!(quiet.ignored);
  }
}
