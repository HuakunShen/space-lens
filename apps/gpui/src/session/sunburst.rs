//! Sunburst geometry over the scan index — the GPUI port of the web UI's
//! d3 partition (web-ui `lib/sunburst.ts`): one ring per depth level around
//! the focused directory, each parent's arc split among its children in
//! proportion to subtree size. Pure data and math; the view paints it.

use std::f32::consts::TAU;

use crate::session::index::{ScanIndex, SortMode};

/// The web sunburst's pastel palette (`web-ui/src/lib/colors.ts`), 0xRRGGBB.
pub const PALETTE: [u32; 9] = [
  0xf8d66d, 0xd8f96a, 0xa7f06b, 0x6ee7a8, 0x5eead4, 0x67e8f9, 0x93c5fd, 0xc4b5fd, 0xf0abfc,
];

/// Muted grays for ignored entries, alternating by ring parity — the web
/// `nodeMutedColor`.
pub fn muted_color(depth: u32) -> u32 {
  if depth % 2 == 0 {
    0x3a3f49
  } else {
    0x484d57
  }
}

/// The web `nodeColor`: a Java-style string hash plus the ring depth picks
/// one of the nine pastels, stable for a given directory across renders.
pub fn segment_color(id: &str, depth: u32) -> u32 {
  let seed = hash(id).wrapping_add(depth.wrapping_mul(17) as i32);
  PALETTE[seed.unsigned_abs() as usize % PALETTE.len()]
}

fn hash(input: &str) -> i32 {
  let mut value: i32 = 0;
  for byte in input.as_bytes() {
    value = value
      .wrapping_shl(5)
      .wrapping_sub(value)
      .wrapping_add(i32::from(*byte));
  }
  value
}

/// One painted wedge of the sunburst.
#[derive(Clone)]
pub struct SunburstSegment {
  pub node_id: String,
  pub name: String,
  /// 1-based ring: 1 = direct children of the center node.
  pub depth: u32,
  /// Plot `Arc` convention: radians, 0 at 12 o'clock, clockwise.
  pub start_angle: f32,
  pub end_angle: f32,
  pub size: u64,
  pub ignored: bool,
  pub has_children: bool,
}

/// Builds every ring for `center_id`'s subtree. Ring 1 splits the full
/// circle among the center's children; ring 2 splits each ring-1 arc among
/// that child's children; and so on to `max_depth`. Children are normalized
/// against each other (their sizes need not sum to the parent's — summarized
/// entries make them differ), so rings never leave gaps or overflow their
/// parent's arc. A child narrower than `min_angle` radians is pruned together
/// with its subtree — invisible slivers cost render time for nothing.
pub fn build_sunburst(
  index: &ScanIndex,
  center_id: &str,
  max_depth: u32,
  min_angle: f32,
) -> Vec<SunburstSegment> {
  let mut segments = Vec::new();
  push_ring(
    index,
    center_id,
    1,
    0.0,
    TAU,
    max_depth,
    min_angle,
    &mut segments,
  );
  segments
}

#[allow(clippy::too_many_arguments)]
fn push_ring(
  index: &ScanIndex,
  node_id: &str,
  depth: u32,
  start: f32,
  end: f32,
  max_depth: u32,
  min_angle: f32,
  out: &mut Vec<SunburstSegment>,
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
  let span = end - start;
  let mut cursor = start;
  for child in children {
    let child_end = cursor + (child.size as f32 / total as f32) * span;
    if child.size > 0 && child_end - cursor >= min_angle {
      out.push(SunburstSegment {
        node_id: child.id.clone(),
        name: child.name.clone(),
        depth,
        start_angle: cursor,
        end_angle: child_end,
        size: child.size,
        ignored: child.ignored,
        has_children: !child.child_ids.is_empty(),
      });
      push_ring(
        index,
        &child.id,
        depth + 1,
        cursor,
        child_end,
        max_depth,
        min_angle,
        out,
      );
    }
    cursor = child_end;
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
        // A zero-size entry must be skipped, not poison the ring.
        node("/proj/empty", 0, vec![]),
      ],
    )])
  }

  #[test]
  fn ring_one_covers_the_full_circle_in_size_order() {
    let index = tree();
    let root_id = index.root_ids()[0].clone();
    let segments = build_sunburst(&index, &root_id, 3, 0.0);

    let ring_one: Vec<_> = segments.iter().filter(|s| s.depth == 1).collect();
    assert_eq!(ring_one.len(), 2); // `empty` is zero-size and skipped
    assert_eq!(ring_one[0].name, "big");
    assert!((ring_one[0].start_angle - 0.0).abs() < 1e-5);
    // 60/(60+20) of the circle.
    assert!((ring_one[0].end_angle - TAU * 0.75).abs() < 1e-4);
    assert_eq!(ring_one[1].name, "small");
    assert!((ring_one[1].end_angle - TAU).abs() < 1e-4);
  }

  #[test]
  fn children_are_normalized_against_each_other_not_the_parent() {
    // `/proj` is 100 but its children sum to 80; ring 1 still fills 2π, and
    // a child's ring-2 arc is its share among ITS siblings — `inner` is
    // `big`'s only sized child, so it takes all of `big`'s arc.
    let index = tree();
    let root_id = index.root_ids()[0].clone();
    let segments = build_sunburst(&index, &root_id, 3, 0.0);

    let big = segments
      .iter()
      .find(|s| s.name == "big")
      .expect("big segment");
    let inner: Vec<_> = segments.iter().filter(|s| s.name == "inner").collect();
    assert_eq!(inner.len(), 1);
    assert!((inner[0].start_angle - big.start_angle).abs() < 1e-5);
    assert!((inner[0].end_angle - big.end_angle).abs() < 1e-4);
  }

  #[test]
  fn min_angle_prunes_a_child_and_its_subtree() {
    let index = tree();
    let root_id = index.root_ids()[0].clone();
    // A threshold above `small`'s 90° keeps only `big` at ring 1 — and
    // nothing for `small`'s (absent) children either way.
    let segments = build_sunburst(&index, &root_id, 3, 2.0);
    assert!(segments.iter().all(|s| s.name != "small"));
    assert!(segments.iter().any(|s| s.name == "big"));
    assert!(!segments.iter().any(|s| s.name == "inner" && s.depth > 2));
  }

  #[test]
  fn max_depth_caps_the_rings() {
    let index = tree();
    let root_id = index.root_ids()[0].clone();
    // depth 1 keeps only the direct children; depth 2 adds `inner`.
    let flat = build_sunburst(&index, &root_id, 1, 0.0);
    assert!(flat.iter().all(|s| s.depth == 1));
    assert!(!flat.iter().any(|s| s.name == "inner"));
    let two_rings = build_sunburst(&index, &root_id, 2, 0.0);
    assert!(two_rings.iter().all(|s| s.depth <= 2));
    assert!(two_rings.iter().any(|s| s.name == "inner"));
  }

  #[test]
  fn colors_match_the_web_palette_and_mute_ignored() {
    // The port must agree with the web `nodeColor(id, depth)` exactly;
    // these three are the JS function's own outputs.
    assert_eq!(segment_color("apps", 1), 0x6ee7a8);
    assert_eq!(segment_color("target", 1), 0x93c5fd);
    assert_eq!(segment_color("node_modules", 1), 0xd8f96a);
    assert_eq!(muted_color(1), 0x484d57);
    assert_eq!(muted_color(2), 0x3a3f49);
  }

  #[test]
  fn segments_carry_paths_and_children_flags() {
    let index = tree();
    let root_id = index.root_ids()[0].clone();
    let segments = build_sunburst(&index, &root_id, 3, 0.0);
    let big = segments.iter().find(|s| s.name == "big").unwrap();
    assert_eq!(big.node_id, node_id_of("/proj/big"));
    assert!(big.has_children);
    let small = segments.iter().find(|s| s.name == "small").unwrap();
    assert!(!small.has_children);
  }
}
