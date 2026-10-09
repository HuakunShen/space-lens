//! Circle-pack geometry over the scan index — the third family beside the
//! ring and tile charts: every direct child of the focused node becomes a
//! circle whose **area** is its size, and the depth-0 focus paints as a faint
//! backdrop disc so the pack reads as one body. Area is the honest encoding
//! here — a radius that tracked size would overstate a folder quadratically —
//! but circles never tile, so this chart is an overview of who is big, not a
//! measuring tool. The pack is built by hand: circles are placed largest
//! first, each one tangent to the pack at the position that keeps the
//! enclosing radius smallest, and the whole pack is scaled into the canvas at
//! the end (a uniform scale preserves every area ratio). Pure data and math
//! (`docs/chart-modes.md` is the shared contract); the view paints it and
//! hit-tests it from the same circles.

use std::f32::consts::TAU;

use crate::session::index::{ScanIndex, SortMode};

/// Angles tried around every already-placed circle when looking for a spot.
/// Coarse enough to stay cheap on a folder with hundreds of children, fine
/// enough that the chosen spot is visually tangent.
const ANGLE_STEPS: usize = 24;
/// The pack keeps at most this many of the largest children; the rest would
/// be sub-pixel at any canvas size this panel has, and the search cost grows
/// with the cube of the count.
pub const MAX_CIRCLES: usize = 96;
/// Breathing room between the pack and the canvas edge, so the outermost
/// circle never kisses the panel border.
const CANVAS_PAD: f32 = 10.0;

/// One painted circle. Coordinates are canvas-local pixels — the caller picks
/// the canvas size, so hit-testing and painting share one geometry without
/// scaling. `x`/`y` are the centre, `r` the radius.
#[derive(Clone, Debug)]
pub struct BubbleCircle {
  pub node_id: String,
  pub name: String,
  /// Full path, carried for the hover summary: circle labels are ellipsized
  /// to fit, and the summary is where the exact location shows.
  pub path: String,
  /// 0 for the focus's backdrop disc, 1 for the painted children.
  pub depth: u32,
  pub x: f32,
  pub y: f32,
  pub r: f32,
  pub size: u64,
  /// Child size ÷ the sized siblings' total — the number the labels show.
  /// The backdrop disc is the whole focus and reports 1.0.
  pub share: f32,
  pub ignored: bool,
  pub has_children: bool,
}

/// One circle before it is scaled into the canvas: centre in pack space and
/// radius in `sqrt(size)` units, so area ratios are exact by construction.
#[derive(Clone, Copy, Debug)]
struct Seed {
  x: f32,
  y: f32,
  r: f32,
}

impl Seed {
  /// True when a circle at `(x, y)` with radius `r` would eat into this one.
  /// The bounding-box reject keeps the O(n) scan cheap enough to run inside
  /// the placement search.
  fn hits(&self, x: f32, y: f32, r: f32) -> bool {
    let gap = self.r + r;
    if (self.x - x).abs() >= gap || (self.y - y).abs() >= gap {
      return false;
    }
    (self.x - x).hypot(self.y - y) < gap
  }
}

/// Builds the pack for `center_id`'s direct children across a `width` ×
/// `height` canvas. The returned vec leads with the focus's own backdrop disc
/// (`depth` 0, `share` 1.0) followed by the children, largest first. Radii
/// are `sqrt(size)` in the pack and are scaled uniformly to the canvas, so
/// circle areas keep the exact ratios the sizes have. Zero-size children are
/// skipped, only the [`MAX_CIRCLES`] largest are packed, and circles scaled
/// below `min_radius` are dropped after the fact — they are invisible, and
/// dropping them cannot move the others. A canvas too small for even
/// `min_radius`, or a focus with no sized children, paints nothing.
pub fn build_bubbles(
  index: &ScanIndex,
  center_id: &str,
  width: f32,
  height: f32,
  min_radius: f32,
) -> Vec<BubbleCircle> {
  let mut out = Vec::new();
  let Some(center) = index.get(center_id) else {
    return out;
  };
  let canvas_radius = (width.min(height) / 2.0 - CANVAS_PAD).max(0.0);
  if canvas_radius <= min_radius {
    return out;
  }
  let children = index
    .children(center_id, SortMode::Size)
    .unwrap_or_default();
  let sized: Vec<_> = children
    .iter()
    .filter(|child| child.size > 0)
    .copied()
    .collect();
  if sized.is_empty() {
    return out;
  }
  // Shares are honest about the whole sibling set even when only the largest
  // MAX_CIRCLES are placed; the dropped tail would be sub-pixel anyway.
  let total: u64 = sized.iter().map(|child| child.size).sum();
  let packed: Vec<_> = sized.iter().take(MAX_CIRCLES).copied().collect();
  let seeds: Vec<f32> = packed
    .iter()
    .map(|child| (child.size as f32).sqrt())
    .collect();
  let placed = pack(&seeds);
  // One uniform scale maps the pack's enclosing radius onto the canvas disc,
  // which is what keeps area ratios exact across canvas sizes.
  let enclosing = placed
    .iter()
    .map(|circle| circle.x.hypot(circle.y) + circle.r)
    .fold(0.0_f32, f32::max);
  let scale = canvas_radius / enclosing;
  let (cx, cy) = (width / 2.0, height / 2.0);

  out.push(BubbleCircle {
    node_id: center.id.clone(),
    name: center.name.clone(),
    path: center.path.clone(),
    depth: 0,
    x: cx,
    y: cy,
    r: canvas_radius,
    size: center.size,
    share: 1.0,
    ignored: center.ignored,
    has_children: true,
  });
  for (child, seed) in packed.iter().zip(&placed) {
    let r = seed.r * scale;
    if r < min_radius {
      continue;
    }
    out.push(BubbleCircle {
      node_id: child.id.clone(),
      name: child.name.clone(),
      path: child.path.clone(),
      depth: 1,
      x: cx + seed.x * scale,
      y: cy + seed.y * scale,
      r,
      size: child.size,
      share: child.size as f32 / total as f32,
      ignored: child.ignored,
      has_children: !child.child_ids.is_empty(),
    });
  }
  out
}

/// Places one circle per radius, in the order given (the caller sorts by size
/// descending so the big folders claim the middle). Each circle is tried
/// tangent to every circle already down, at [`ANGLE_STEPS`] positions around
/// each host, and the candidate that keeps the enclosing radius smallest
/// wins; ties go to the earliest candidate, so the pack is deterministic.
/// When no tangent spot is free, [`free_ring`] pushes the circle out to the
/// first free spot on an ever-wider ring around the pack.
fn pack(radii: &[f32]) -> Vec<Seed> {
  let mut placed: Vec<Seed> = Vec::with_capacity(radii.len());
  let mut enclosing = 0.0_f32;
  for &r in radii {
    if placed.is_empty() {
      placed.push(Seed { x: 0.0, y: 0.0, r });
      enclosing = r;
      continue;
    }
    let mut best: Option<Seed> = None;
    let mut best_enclosing = f32::INFINITY;
    for host in &placed {
      let tangent = host.r + r;
      for step in 0..ANGLE_STEPS {
        let angle = TAU * step as f32 / ANGLE_STEPS as f32;
        let x = host.x + tangent * angle.cos();
        let y = host.y + tangent * angle.sin();
        if placed.iter().any(|circle| circle.hits(x, y, r)) {
          continue;
        }
        let candidate = x.hypot(y) + r;
        if candidate < best_enclosing - 1e-4 {
          best_enclosing = candidate;
          best = Some(Seed { x, y, r });
        }
      }
    }
    let next = match best {
      Some(seed) => seed,
      None => free_ring(&placed, r, enclosing),
    };
    enclosing = enclosing.max(next.x.hypot(next.y) + r);
    placed.push(next);
  }
  placed
}

/// The fallback spot: walk outward from the pack in [`ANGLE_STEPS`] steps per
/// ring, widening the ring until a position clears every placed circle. A
/// wide enough ring always has room, so this terminates.
fn free_ring(placed: &[Seed], r: f32, enclosing: f32) -> Seed {
  let mut distance = enclosing + r;
  for _ in 0..8 {
    for step in 0..ANGLE_STEPS {
      let angle = TAU * step as f32 / ANGLE_STEPS as f32;
      let x = distance * angle.cos();
      let y = distance * angle.sin();
      if !placed.iter().any(|circle| circle.hits(x, y, r)) {
        return Seed { x, y, r };
      }
    }
    distance *= 1.5;
  }
  Seed {
    x: distance,
    y: 0.0,
    r,
  }
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
        // A zero-size entry must be skipped, not sit at the centre as a dot.
        node("/proj/empty", 0, vec![]),
      ],
    )])
  }

  /// Twelve perfect-square sizes 400 … 81, so a correct pack has integer
  /// radius ratios: sqrt(400) = 20 down to sqrt(81) = 9.
  fn many() -> ScanIndex {
    let sizes = [400u64, 361, 324, 289, 256, 225, 196, 169, 144, 121, 100, 81];
    let kids = sizes
      .iter()
      .map(|size| node(&format!("/r/k{size}"), *size, vec![]))
      .collect();
    ScanIndex::new(&[node("/r", 5_000, kids)])
  }

  fn built(index: &ScanIndex, width: f32, height: f32) -> Vec<BubbleCircle> {
    let root_id = index.root_ids()[0].clone();
    build_bubbles(index, &root_id, width, height, 0.0)
  }

  fn painted(circles: &[BubbleCircle]) -> Vec<&BubbleCircle> {
    circles.iter().filter(|circle| circle.depth == 1).collect()
  }

  #[test]
  fn no_two_painted_circles_overlap() {
    let index = many();
    let circles = built(&index, 700.0, 500.0);
    let painted = painted(&circles);
    assert_eq!(painted.len(), 12);
    for (ix, a) in painted.iter().enumerate() {
      for b in painted.iter().skip(ix + 1) {
        let distance = (a.x - b.x).hypot(a.y - b.y);
        assert!(
          distance >= a.r + b.r - 0.01,
          "{} (r {}) overlaps {} (r {}): centres {} apart",
          a.name,
          a.r,
          b.name,
          b.r,
          distance
        );
      }
    }
  }

  #[test]
  fn circle_areas_track_size_ratios() {
    let index = many();
    let circles = built(&index, 700.0, 500.0);
    let by_name = |name: &str| {
      circles
        .iter()
        .find(|circle| circle.name == name)
        .unwrap_or_else(|| panic!("{name} circle"))
    };
    // 400 : 100 is 4× the area, so 2× the radius.
    let big = by_name("k400");
    let small = by_name("k100");
    assert!((big.r / small.r - 2.0).abs() < 0.02);
    assert!(((big.r / small.r).powi(2) - 4.0).abs() < 0.1);
    // The same identity through the size field: area ∝ size.
    let area_ratio = (big.r * big.r) / (small.r * small.r);
    let size_ratio = big.size as f32 / small.size as f32;
    assert!((area_ratio - size_ratio).abs() < 0.05);
    assert!((big.share - 400.0 / 2_666.0).abs() < 1e-3);
  }

  #[test]
  fn every_circle_fits_the_canvas_and_the_backdrop() {
    let (width, height) = (700.0, 500.0);
    let index = many();
    let circles = built(&index, width, height);
    let backdrop = circles.first().unwrap();
    assert_eq!(backdrop.depth, 0);
    assert!((backdrop.x - width / 2.0).abs() < 0.01);
    assert!((backdrop.y - height / 2.0).abs() < 0.01);
    for circle in painted(&circles) {
      assert!(
        circle.x - circle.r >= -0.01,
        "{circle:?} leaves the canvas left"
      );
      assert!(circle.x + circle.r <= width + 0.01);
      assert!(circle.y - circle.r >= -0.01);
      assert!(circle.y + circle.r <= height + 0.01);
      // The backdrop is the focus's own disc: the pack sits inside it.
      let distance = (circle.x - backdrop.x).hypot(circle.y - backdrop.y);
      assert!(distance + circle.r <= backdrop.r + 0.01);
    }
  }

  #[test]
  fn the_pack_is_deterministic_across_runs() {
    let index = many();
    let first = built(&index, 700.0, 500.0);
    let again = built(&index, 700.0, 500.0);
    assert_eq!(first.len(), again.len());
    for (a, b) in first.iter().zip(&again) {
      assert_eq!(a.node_id, b.node_id);
      assert_eq!(a.x.to_bits(), b.x.to_bits());
      assert_eq!(a.y.to_bits(), b.y.to_bits());
      assert_eq!(a.r.to_bits(), b.r.to_bits());
    }
  }

  #[test]
  fn the_backdrop_leads_the_pack_at_depth_zero() {
    let index = tree();
    let circles = built(&index, 600.0, 600.0);
    let backdrop = circles.first().unwrap();
    assert_eq!(backdrop.depth, 0);
    assert_eq!(backdrop.node_id, node_id_of("/proj"));
    assert_eq!(backdrop.name, "proj");
    assert_eq!(backdrop.path, "/proj");
    assert_eq!(backdrop.size, 100);
    assert!((backdrop.share - 1.0).abs() < 1e-4);
    assert!(backdrop.has_children);
    // 60 : 20 — the ratio, not the parent's absolute size.
    let big = circles.iter().find(|circle| circle.name == "big").unwrap();
    let small = circles
      .iter()
      .find(|circle| circle.name == "small")
      .unwrap();
    assert!((big.r / small.r - 3.0_f32.sqrt()).abs() < 0.01);
    // The backdrop's radius is the whole canvas disc the pack is scaled into.
    assert!((backdrop.r - (600.0 / 2.0 - CANVAS_PAD)).abs() < 0.01);
  }

  #[test]
  fn zero_size_children_and_a_single_child() {
    let index = tree();
    let circles = built(&index, 600.0, 600.0);
    assert!(circles.iter().all(|circle| circle.name != "empty"));
    assert_eq!(painted(&circles).len(), 2);

    let single = ScanIndex::new(&[node("/r", 50, vec![node("/r/only", 50, vec![])])]);
    let circles = built(&single, 400.0, 400.0);
    let only = circles.iter().find(|circle| circle.name == "only").unwrap();
    assert!((only.share - 1.0).abs() < 1e-4);
    // A one-circle pack is centred and scaled to the canvas disc.
    assert!((only.x - 200.0).abs() < 0.01 && (only.y - 200.0).abs() < 0.01);
    assert!((only.r - (200.0 - CANVAS_PAD)).abs() < 0.01);
  }

  #[test]
  fn the_pack_keeps_at_most_max_circles() {
    let kids: Vec<ScanNode> = (0..300)
      .map(|i| node(&format!("/r/k{i:03}"), 1_000 - i as u64, vec![]))
      .collect();
    let index = ScanIndex::new(&[node("/r", 300_000, kids)]);
    let circles = built(&index, 900.0, 700.0);
    assert_eq!(painted(&circles).len(), MAX_CIRCLES);
    // The kept ones are the largest, in size order.
    let sizes: Vec<u64> = painted(&circles).iter().map(|circle| circle.size).collect();
    assert!(sizes.windows(2).all(|pair| pair[0] >= pair[1]));
  }

  #[test]
  fn min_radius_drops_the_invisible_tail() {
    let index = ScanIndex::new(&[node(
      "/r",
      1_000_000,
      vec![
        node("/r/giant", 999_990, vec![]),
        node("/r/flea", 10, vec![]),
      ],
    )]);
    let root_id = index.root_ids()[0].clone();
    let all = build_bubbles(&index, &root_id, 600.0, 600.0, 0.0);
    assert!(all.iter().any(|circle| circle.name == "flea"));
    let pruned = build_bubbles(&index, &root_id, 600.0, 600.0, 12.0);
    assert!(pruned.iter().all(|circle| circle.name != "flea"));
    assert!(pruned.iter().any(|circle| circle.name == "giant"));
    assert!(pruned
      .iter()
      .filter(|circle| circle.depth == 1)
      .all(|circle| circle.r >= 12.0));
  }

  #[test]
  fn an_empty_or_tiny_canvas_paints_nothing() {
    let index = tree();
    let root_id = index.root_ids()[0].clone();
    assert!(build_bubbles(&index, &root_id, 0.0, 0.0, 0.0).is_empty());
    assert!(build_bubbles(&index, &root_id, 12.0, 600.0, 0.0).is_empty());
    assert!(build_bubbles(&index, "missing", 600.0, 600.0, 0.0).is_empty());
    // A leaf focus has no children to pack.
    let leaf = ScanIndex::new(&[node("/leaf", 10, vec![])]);
    assert!(build_bubbles(&leaf, &leaf.root_ids()[0], 600.0, 600.0, 0.0).is_empty());
    // Every child at zero size is the same as no children.
    let empty = ScanIndex::new(&[node("/r", 0, vec![node("/r/a", 0, vec![])])]);
    assert!(build_bubbles(&empty, &empty.root_ids()[0], 600.0, 600.0, 0.0).is_empty());
  }

  #[test]
  fn circles_carry_ids_paths_flags_and_sizes() {
    let index = tree();
    let circles = built(&index, 600.0, 600.0);
    let big = circles.iter().find(|circle| circle.name == "big").unwrap();
    assert_eq!(big.node_id, node_id_of("/proj/big"));
    assert_eq!(big.path, "/proj/big");
    assert_eq!(big.size, 60);
    assert_eq!(big.depth, 1);
    assert!(big.has_children);
    let small = circles
      .iter()
      .find(|circle| circle.name == "small")
      .unwrap();
    assert!(!small.has_children);
    assert_eq!(small.path, "/proj/small");

    let mut flagged = node("/proj/quiet", 20, vec![]);
    flagged.ignored = true;
    let index = ScanIndex::new(&[node(
      "/proj",
      120,
      vec![node("/proj/loud", 100, vec![]), flagged],
    )]);
    let circles = built(&index, 600.0, 600.0);
    let quiet = circles
      .iter()
      .find(|circle| circle.name == "quiet")
      .unwrap();
    assert!(quiet.ignored);
  }
}
