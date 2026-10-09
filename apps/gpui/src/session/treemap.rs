//! Squarified treemap geometry over the scan index — the Burrow-style
//! "square" counterpart to `session::sunburst`: the focused directory's
//! children tile the whole chart, each roomy tile shows its own children
//! inset by a fixed padding, and tile areas track subtree sizes. Pure data
//! and math (the Bruls–Huizing–van Wijk squarified layout); the view paints
//! it and hit-tests it from the same rects.

use crate::session::index::{ScanIndex, SortMode};

/// Horizontal inset reserved inside a tile before its children are laid
/// out — the nested-panels-inside-the-parent look of Burrow's treemap.
pub const LEVEL_PAD: f32 = 12.0;
/// Vertical inset above the children; taller than the sides so the parent's
/// name fits in the top padding strip (the webpack-analyzer frame look).
pub const LEVEL_PAD_TOP: f32 = 16.0;
/// A tile needs at least this much room beyond the padding on both axes
/// before its children are drawn inside; anything smaller stays a leaf.
const MIN_CONTENT_SIDE: f32 = 30.0;

/// One painted rectangle of the treemap. Coordinates are canvas-local
/// pixels — the caller picks the canvas size, so hit-testing and painting
/// share one geometry without scaling.
#[derive(Clone, Debug)]
pub struct TreemapRect {
  pub node_id: String,
  pub name: String,
  /// 1-based nesting level: 1 = direct children of the center node.
  pub depth: u32,
  pub x: f32,
  pub y: f32,
  pub w: f32,
  pub h: f32,
  pub size: u64,
  /// Share of this tile among its own tiling siblings (child size ÷ the
  /// sibling sum the layout normalized against) — the "who is biggest in
  /// this layer" number the flat view's labels show.
  pub share: f32,
  pub ignored: bool,
  pub has_children: bool,
}

#[derive(Clone, Copy, Debug)]
struct Rect {
  x: f32,
  y: f32,
  w: f32,
  h: f32,
}

impl Rect {
  fn area(&self) -> f32 {
    self.w * self.h
  }
}

/// Builds every tile for `center_id`'s subtree across a `width` × `height`
/// canvas. Depth 1 tiles the full rect among the center's children; a tile
/// with room for [`LEVEL_PAD`] + [`MIN_CONTENT_SIDE`] on both axes shows its
/// own children inside. Children are normalized against each other (their
/// sizes need not sum to the parent's — summarized entries make them
/// differ), so tiles always fill their parent exactly. Zero-size children
/// are skipped, and any tile narrower than `min_side` on an axis is dropped
/// together with its subtree — sub-pixel slivers cost render time for
/// nothing.
pub fn build_treemap(
  index: &ScanIndex,
  center_id: &str,
  width: f32,
  height: f32,
  max_depth: u32,
  min_side: f32,
) -> Vec<TreemapRect> {
  let mut out = Vec::new();
  if width < min_side || height < min_side {
    return out;
  }
  layout_children(
    index,
    center_id,
    Rect {
      x: 0.,
      y: 0.,
      w: width,
      h: height,
    },
    1,
    max_depth,
    min_side,
    &mut out,
  );
  out
}

fn layout_children(
  index: &ScanIndex,
  node_id: &str,
  rect: Rect,
  depth: u32,
  max_depth: u32,
  min_side: f32,
  out: &mut Vec<TreemapRect>,
) {
  if depth > max_depth {
    return;
  }
  let Some(children) = index.children(node_id, SortMode::Size) else {
    return;
  };
  let total: u64 = children.iter().map(|child| child.size).sum();
  if total == 0 {
    return;
  }
  // Squarify wants areas: subtree size scaled so the children fill `rect`
  // exactly, zero-size entries dropped (they carry no area to place).
  let scale = rect.area() / total as f32;
  let sized: Vec<_> = children.iter().filter(|child| child.size > 0).collect();
  let areas: Vec<f32> = sized
    .iter()
    .map(|child| child.size as f32 * scale)
    .collect();
  for (ix, placed) in squarify(&areas, rect) {
    if placed.w < min_side || placed.h < min_side {
      continue;
    }
    let child = sized[ix];
    let has_children = !child.child_ids.is_empty();
    out.push(TreemapRect {
      node_id: child.id.clone(),
      name: child.name.clone(),
      depth,
      x: placed.x,
      y: placed.y,
      w: placed.w,
      h: placed.h,
      size: child.size,
      share: child.size as f32 / total as f32,
      ignored: child.ignored,
      has_children,
    });
    let roomy = placed.w >= 2.0 * LEVEL_PAD + MIN_CONTENT_SIDE
      && placed.h >= LEVEL_PAD_TOP + LEVEL_PAD + MIN_CONTENT_SIDE;
    if depth < max_depth && has_children && roomy {
      layout_children(
        index,
        &child.id,
        Rect {
          x: placed.x + LEVEL_PAD,
          y: placed.y + LEVEL_PAD_TOP,
          w: placed.w - 2.0 * LEVEL_PAD,
          h: placed.h - LEVEL_PAD_TOP - LEVEL_PAD,
        },
        depth + 1,
        max_depth,
        min_side,
        out,
      );
    }
  }
}

/// The squarified layout: rows of items are packed along the remaining
/// rect's shorter side, and an item joins the current row only while the
/// worst aspect ratio in the row keeps improving. Returns `(item index,
/// rect)` pairs covering `rect` exactly, in item order within each row.
fn squarify(areas: &[f32], rect: Rect) -> Vec<(usize, Rect)> {
  let mut out = Vec::with_capacity(areas.len());
  let mut remaining = rect;
  let mut start = 0;
  while start < areas.len() {
    if remaining.w <= 0.0 || remaining.h <= 0.0 {
      break;
    }
    let short = remaining.w.min(remaining.h);
    let mut end = start;
    let mut row_sum = 0.0;
    let mut best = f32::INFINITY;
    while end < areas.len() {
      let candidate_sum = row_sum + areas[end];
      let ratio = row_worst_ratio(candidate_sum, short, &areas[start..=end]);
      if ratio <= best {
        best = ratio;
        row_sum = candidate_sum;
        end += 1;
      } else {
        break;
      }
    }
    // A row laid along the shorter side is exactly `row_sum / short` thick
    // and always fits the longer side; the min only absorbs float drift (a
    // clamped row gives its last item the leftover via the fill below).
    let thickness = if remaining.w >= remaining.h {
      (row_sum / short).min(remaining.w)
    } else {
      (row_sum / short).min(remaining.h)
    };
    let row: Vec<f32> = areas[start..end].to_vec();
    if remaining.w >= remaining.h {
      // A vertical column hugging the left edge, items stacked downward.
      let mut y = remaining.y;
      for (k, &area) in row.iter().enumerate() {
        // The row's last item fills whatever float drift is left, so the
        // column ends exactly at the remaining rect's bottom edge.
        let h = if k == row.len() - 1 {
          (remaining.y + remaining.h - y).max(0.0)
        } else {
          area / thickness
        };
        out.push((
          start + k,
          Rect {
            x: remaining.x,
            y,
            w: thickness,
            h,
          },
        ));
        y += h;
      }
      remaining = Rect {
        x: remaining.x + thickness,
        y: remaining.y,
        w: (remaining.w - thickness).max(0.0),
        h: remaining.h,
      };
    } else {
      // A horizontal row hugging the top edge, items laid left to right.
      let mut x = remaining.x;
      for (k, &area) in row.iter().enumerate() {
        let w = if k == row.len() - 1 {
          (remaining.x + remaining.w - x).max(0.0)
        } else {
          area / thickness
        };
        out.push((
          start + k,
          Rect {
            x,
            y: remaining.y,
            w,
            h: thickness,
          },
        ));
        x += w;
      }
      remaining = Rect {
        x: remaining.x,
        y: remaining.y + thickness,
        w: remaining.w,
        h: (remaining.h - thickness).max(0.0),
      };
    }
    start = end;
  }
  out
}

/// The worst width/height ratio among the row's items if the row (total
/// `row_area`) is laid along a side of length `short`.
fn row_worst_ratio(row_area: f32, short: f32, items: &[f32]) -> f32 {
  if short <= 0.0 || row_area <= 0.0 {
    return f32::INFINITY;
  }
  let thickness = row_area / short;
  items
    .iter()
    .map(|&area| {
      let length = area / thickness;
      let lo = thickness.min(length);
      let hi = thickness.max(length);
      if lo <= 0.0 {
        f32::INFINITY
      } else {
        hi / lo
      }
    })
    .fold(0.0_f32, f32::max)
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
        // A zero-size entry must be skipped, not poison the layout.
        node("/proj/empty", 0, vec![]),
      ],
    )])
  }

  fn layout(index: &ScanIndex, width: f32, height: f32) -> Vec<TreemapRect> {
    let root_id = index.root_ids()[0].clone();
    build_treemap(index, &root_id, width, height, 4, 3.0)
  }

  fn overlap(a: &TreemapRect, b: &TreemapRect) -> f32 {
    let w = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
    let h = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
    w.max(0.0) * h.max(0.0)
  }

  #[test]
  fn depth_one_tiles_fill_the_canvas_without_overlap() {
    let index = tree();
    let (width, height) = (800.0, 600.0);
    let tiles = layout(&index, width, height);

    let top: Vec<_> = tiles.iter().filter(|t| t.depth == 1).collect();
    assert_eq!(top.len(), 2); // `empty` is zero-size and skipped
    let total_area: f32 = top.iter().map(|t| t.w * t.h).sum();
    assert!((total_area - width * height).abs() < 0.5);
    for (a, b) in top.iter().zip(top.iter().skip(1)) {
      assert!(overlap(a, b) < 0.01, "{a:?} overlaps {b:?}");
    }
    for tile in &top {
      assert!(tile.x >= -0.01 && tile.y >= -0.01);
      assert!(tile.x + tile.w <= width + 0.01);
      assert!(tile.y + tile.h <= height + 0.01);
    }
  }

  #[test]
  fn tile_areas_track_subtree_sizes() {
    let index = tree();
    let tiles = layout(&index, 800.0, 600.0);
    let big = tiles.iter().find(|t| t.name == "big").unwrap();
    let small = tiles.iter().find(|t| t.name == "small").unwrap();
    // 60 : 20 — the ratio, not the parent's absolute size.
    assert!((big.w * big.h) / (small.w * small.h) - 3.0 < 0.05);
  }

  #[test]
  fn equal_items_stay_near_square_while_a_slice_would_be_slivers() {
    // 16 equal children sliced vertically would each be 40 × 400 (ratio 10);
    // squarified keeps every tile at a readable ratio.
    let kids: Vec<ScanNode> = (0..16)
      .map(|i| node(&format!("/r/k{i:02}"), 100, vec![]))
      .collect();
    let index = ScanIndex::new(&[node("/r", 1600, kids)]);
    let tiles = layout(&index, 640.0, 400.0);
    assert_eq!(tiles.len(), 16);
    for tile in &tiles {
      let ratio = tile.w.max(tile.h) / tile.w.min(tile.h);
      assert!(ratio < 4.0, "tile {tile:?} is a sliver (ratio {ratio})");
    }
  }

  #[test]
  fn nested_children_sit_inset_inside_their_parent() {
    let index = tree();
    let tiles = layout(&index, 800.0, 600.0);
    let big = tiles.iter().find(|t| t.name == "big").unwrap();
    let inner = tiles.iter().find(|t| t.name == "inner").unwrap();
    assert_eq!(inner.depth, 2);
    assert!(inner.x >= big.x + LEVEL_PAD - 0.01);
    assert!(inner.y >= big.y + LEVEL_PAD - 0.01);
    assert!(inner.x + inner.w <= big.x + big.w - LEVEL_PAD + 0.01);
    assert!(inner.y + inner.h <= big.y + big.h - LEVEL_PAD + 0.01);
  }

  #[test]
  fn small_tiles_are_leaves_even_with_children() {
    // `big` (60 of 80) on a short canvas still fits a nested inset, but a
    // narrow canvas squeezes it below the padding + content threshold.
    let index = tree();
    let root_id = index.root_ids()[0].clone();
    let tiles = build_treemap(&index, &root_id, 200.0, 40.0, 4, 3.0);
    let inner = tiles.iter().find(|t| t.name == "inner");
    assert!(inner.is_none(), "no room to nest: {tiles:?}");
    // The top-level tiles still cover the canvas.
    let area: f32 = tiles.iter().map(|t| t.w * t.h).sum();
    assert!((area - 200.0 * 40.0).abs() < 0.5);
  }

  #[test]
  fn min_side_drops_slivers_and_their_subtrees() {
    let index = ScanIndex::new(&[node(
      "/proj",
      100_000,
      vec![
        node(
          "/proj/big",
          99_990,
          vec![node("/proj/big/inner", 10, vec![])],
        ),
        node("/proj/tiny", 10, vec![]),
      ],
    )]);
    let root_id = index.root_ids()[0].clone();
    let tiles = build_treemap(&index, &root_id, 800.0, 600.0, 4, 3.0);
    assert!(tiles.iter().all(|t| t.name != "tiny"));
    assert!(tiles.iter().all(|t| t.w >= 3.0 && t.h >= 3.0));
  }

  #[test]
  fn max_depth_caps_nesting() {
    let index = tree();
    let root_id = index.root_ids()[0].clone();
    let flat = build_treemap(&index, &root_id, 800.0, 600.0, 1, 3.0);
    assert!(flat.iter().all(|t| t.depth == 1));
    let nested = build_treemap(&index, &root_id, 800.0, 600.0, 2, 3.0);
    assert!(nested.iter().any(|t| t.name == "inner" && t.depth == 2));
  }

  #[test]
  fn tiles_carry_ids_flags_shares_and_sizes() {
    let index = tree();
    let tiles = layout(&index, 800.0, 600.0);
    let big = tiles.iter().find(|t| t.name == "big").unwrap();
    assert_eq!(big.node_id, node_id_of("/proj/big"));
    assert_eq!(big.size, 60);
    assert!(big.has_children);
    assert!((big.share - 0.75).abs() < 1e-4); // 60 of the 80 sized total
    let small = tiles.iter().find(|t| t.name == "small").unwrap();
    assert!(!small.has_children);
    assert!((small.share - 0.25).abs() < 1e-4);
    // `inner` is `big`'s only sized child — its own tiling gives it all.
    let inner = tiles.iter().find(|t| t.name == "inner").unwrap();
    assert!((inner.share - 1.0).abs() < 1e-4);

    let mut flagged = node("/proj/quiet", 20, vec![]);
    flagged.ignored = true;
    let index = ScanIndex::new(&[node(
      "/proj",
      120,
      vec![node("/proj/loud", 100, vec![]), flagged],
    )]);
    let root_id = index.root_ids()[0].clone();
    let tiles = build_treemap(&index, &root_id, 800.0, 600.0, 4, 3.0);
    let quiet = tiles.iter().find(|t| t.name == "quiet").unwrap();
    assert!(quiet.ignored);
  }

  #[test]
  fn a_canvas_below_min_side_is_empty() {
    let index = tree();
    let root_id = index.root_ids()[0].clone();
    assert!(build_treemap(&index, &root_id, 2.0, 600.0, 4, 3.0).is_empty());
    assert!(build_treemap(&index, &root_id, 800.0, 1.0, 4, 3.0).is_empty());
  }
}
