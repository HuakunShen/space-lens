//! Strip geometry over the scan index — the ranked-list counterpart to the
//! area charts: one row per direct child of the focused node, ordered biggest
//! first, and each row split left to right among that child's own children.
//! Every row is the same length on purpose. Two rows can then be compared
//! without mentally rescaling bars — "this folder is half caches, that one is
//! nearly all source" — and the ranking is carried by the order and the row
//! label, which is information the treemap already encodes by area. Segments
//! normalize against the row's own children, the same convention the other
//! families use, so a row always fills exactly instead of stopping short when
//! summarized entries make a folder's children sum to less than the folder.
//! Pure data and math (`docs/chart-modes.md` is the shared contract); the
//! view paints it and hit-tests it from the same rows.

use crate::session::index::{ScanIndex, SortMode};

/// One ranked row: a direct child of the focused node plus the strip of its
/// own children that fills it. Widths are fractions of the row, not pixels —
/// the caller picks the canvas width, so painting and hit-testing share one
/// geometry without scaling.
#[derive(Clone, Debug)]
pub struct StripRow {
  pub node_id: String,
  pub name: String,
  /// Full path, carried for the hover summary: strip labels are ellipsized
  /// inside their segments, and the summary is where the exact location
  /// shows.
  pub path: String,
  /// 1-based depth of the row's node — rows are always the focused node's
  /// direct children, so this is 1. It is kept so a row speaks the same
  /// shape as the other families' primitives.
  pub depth: u32,
  pub size: u64,
  /// Row size ÷ the sized siblings' total, so the rows of one chart sum to
  /// 1 — the ranked fraction the row label shows.
  pub share: f32,
  pub ignored: bool,
  pub has_children: bool,
  /// The row's own children, left to right, each a `width_fraction` of the
  /// row. Empty when the row is a leaf (or when all of its children are
  /// zero-size): the view then paints one solid bar in the row's color.
  pub segments: Vec<StripSegment>,
}

/// One segment of a row: a child of the row's node.
#[derive(Clone, Debug)]
pub struct StripSegment {
  pub node_id: String,
  pub name: String,
  pub path: String,
  /// 2 when the row's node is a depth-1 child of the focus.
  pub depth: u32,
  pub size: u64,
  /// Segment size ÷ the row's sized children total — the segments of a row
  /// tile it exactly, so these sum to 1. This plays the role `share` plays
  /// in the other families.
  pub width_fraction: f32,
  pub ignored: bool,
  pub has_children: bool,
}

/// Builds the ranked rows for `center_id`'s direct children. Rows come back
/// in [`SortMode::Size`] order (size descending, name as the tie-break), and
/// each row's segments tile it in the same order. Zero-size children are
/// dropped everywhere — they would be zero-width segments that hit-test as
/// nothing — and a row whose children are all zero-size comes back with no
/// segments, which the view paints as one solid leaf bar.
pub fn build_strips(index: &ScanIndex, center_id: &str) -> Vec<StripRow> {
  let mut rows = Vec::new();
  let Some(children) = index.children(center_id, SortMode::Size) else {
    return rows;
  };
  let total: u64 = children.iter().map(|child| child.size).sum();
  if total == 0 {
    return rows;
  }
  for child in children {
    if child.size == 0 {
      continue;
    }
    let mut segments = Vec::new();
    let grandchildren = index
      .children(&child.id, SortMode::Size)
      .unwrap_or_default();
    let kid_total: u64 = grandchildren.iter().map(|kid| kid.size).sum();
    if kid_total > 0 {
      for kid in grandchildren {
        if kid.size == 0 {
          continue;
        }
        segments.push(StripSegment {
          node_id: kid.id.clone(),
          name: kid.name.clone(),
          path: kid.path.clone(),
          depth: 2,
          size: kid.size,
          width_fraction: kid.size as f32 / kid_total as f32,
          ignored: kid.ignored,
          has_children: !kid.child_ids.is_empty(),
        });
      }
    }
    rows.push(StripRow {
      node_id: child.id.clone(),
      name: child.name.clone(),
      path: child.path.clone(),
      depth: 1,
      size: child.size,
      share: child.size as f32 / total as f32,
      ignored: child.ignored,
      has_children: !child.child_ids.is_empty(),
      segments,
    });
  }
  rows
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
        node(
          "/proj/big",
          60,
          vec![
            node("/proj/big/a", 40, vec![]),
            node("/proj/big/b", 20, vec![]),
          ],
        ),
        node("/proj/small", 20, vec![]),
        // A zero-size entry must be dropped, not become a zero-width row.
        node("/proj/empty", 0, vec![]),
      ],
    )])
  }

  fn built(index: &ScanIndex) -> Vec<StripRow> {
    let root_id = index.root_ids()[0].clone();
    build_strips(index, &root_id)
  }

  #[test]
  fn rows_rank_by_size_descending() {
    let index = ScanIndex::new(&[node(
      "/r",
      600,
      vec![
        node("/r/small", 100, vec![]),
        node("/r/big", 400, vec![]),
        node("/r/mid", 200, vec![]),
      ],
    )]);
    let rows = built(&index);
    let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
    assert_eq!(names, ["big", "mid", "small"]);
    assert!((rows[0].share - 400.0 / 700.0).abs() < 1e-4);
    assert!((rows.iter().map(|row| row.share).sum::<f32>() - 1.0).abs() < 1e-4);
  }

  #[test]
  fn segments_tile_their_row_exactly() {
    let index = tree();
    let rows = built(&index);
    let big = rows.iter().find(|row| row.name == "big").unwrap();
    assert_eq!(big.segments.len(), 2);
    let sum: f32 = big.segments.iter().map(|seg| seg.width_fraction).sum();
    assert!((sum - 1.0).abs() < 1e-4);
    // 40 : 20 of the 60 the children account for.
    assert!((big.segments[0].width_fraction - 2.0 / 3.0).abs() < 1e-4);
    assert_eq!(big.segments[0].name, "a");
    assert_eq!(big.segments[1].name, "b");
  }

  #[test]
  fn segment_sizes_sum_to_the_row_size_when_the_children_account_for_it() {
    let index = tree();
    let rows = built(&index);
    let big = rows.iter().find(|row| row.name == "big").unwrap();
    let sum: u64 = big.segments.iter().map(|seg| seg.size).sum();
    assert_eq!(sum, big.size);
  }

  #[test]
  fn children_are_normalized_against_each_other_not_the_row_size() {
    // `/r/big` reports 60 but its children only account for 30: the segments
    // still tile the row instead of stopping halfway.
    let index = ScanIndex::new(&[node(
      "/r",
      100,
      vec![node(
        "/r/big",
        60,
        vec![node("/r/big/a", 20, vec![]), node("/r/big/b", 10, vec![])],
      )],
    )]);
    let rows = built(&index);
    let big = rows.first().unwrap();
    let sum: f32 = big.segments.iter().map(|seg| seg.width_fraction).sum();
    assert!((sum - 1.0).abs() < 1e-4);
    // The raw share of the row is still visible through the sizes.
    assert_eq!(big.segments.iter().map(|seg| seg.size).sum::<u64>(), 30);
    assert_eq!(big.size, 60);
  }

  #[test]
  fn zero_size_children_are_dropped_without_zero_width_segments() {
    let index = tree();
    let rows = built(&index);
    assert!(rows.iter().all(|row| row.name != "empty"));
    for row in &rows {
      for segment in &row.segments {
        assert!(segment.size > 0);
        assert!(segment.width_fraction > 0.0);
        assert!(segment.width_fraction <= 1.0 + 1e-4);
      }
    }
    // A row whose children are all zero-size still shows as a solid bar.
    let index = ScanIndex::new(&[node(
      "/r",
      50,
      vec![node("/r/a", 50, vec![node("/r/a/void", 0, vec![])])],
    )]);
    let rows = built(&index);
    let a = rows.first().unwrap();
    assert!(a.has_children);
    assert!(a.segments.is_empty());
  }

  #[test]
  fn a_leaf_row_has_no_segments() {
    let index = tree();
    let rows = built(&index);
    let small = rows.iter().find(|row| row.name == "small").unwrap();
    assert!(small.segments.is_empty());
    assert!(!small.has_children);
  }

  #[test]
  fn a_single_child_row_is_one_full_width_segment() {
    let index = ScanIndex::new(&[node(
      "/r",
      100,
      vec![node("/r/only", 100, vec![node("/r/only/leaf", 90, vec![])])],
    )]);
    let rows = built(&index);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert!((row.share - 1.0).abs() < 1e-4);
    assert_eq!(row.segments.len(), 1);
    assert!((row.segments[0].width_fraction - 1.0).abs() < 1e-4);
  }

  #[test]
  fn an_empty_index_or_a_leaf_focus_yields_no_rows() {
    let index = tree();
    assert!(build_strips(&index, "missing").is_empty());
    let leaf = ScanIndex::new(&[node("/leaf", 10, vec![])]);
    assert!(build_strips(&leaf, &leaf.root_ids()[0]).is_empty());
    let empty = ScanIndex::new(&[node("/r", 0, vec![node("/r/a", 0, vec![])])]);
    assert!(build_strips(&empty, &empty.root_ids()[0]).is_empty());
  }

  #[test]
  fn rows_carry_ids_paths_flags_and_segment_links() {
    let index = tree();
    let rows = built(&index);
    let big = rows.iter().find(|row| row.name == "big").unwrap();
    assert_eq!(big.node_id, node_id_of("/proj/big"));
    assert_eq!(big.path, "/proj/big");
    assert_eq!(big.depth, 1);
    assert_eq!(big.size, 60);
    assert!(big.has_children);
    let a = &big.segments[0];
    assert_eq!(a.node_id, node_id_of("/proj/big/a"));
    assert_eq!(a.path, "/proj/big/a");
    assert_eq!(a.depth, 2);
    assert_eq!(a.size, 40);
    assert!(!a.has_children);

    let mut flagged = node("/proj/quiet", 20, vec![]);
    flagged.ignored = true;
    let index = ScanIndex::new(&[node(
      "/proj",
      120,
      vec![node("/proj/loud", 100, vec![]), flagged],
    )]);
    let rows = built(&index);
    let quiet = rows.iter().find(|row| row.name == "quiet").unwrap();
    assert!(quiet.ignored);
  }
}
