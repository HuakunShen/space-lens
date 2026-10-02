//! In-memory scan index: `Vec<ScanNode>` trees flattened into a
//! `HashMap<node_id, IndexEntry>` for O(1) lookup. Ported from the desktop
//! engine's session layer (`apps/desktop/src-tauri/src/engine.rs`), minus the
//! serde wire shapes and the offset/limit paging.

use std::collections::HashMap;
use std::path::Path;

use sha1::{Digest, Sha1};
use space_lens::ScanNode;

/// Sort orders for [`ScanIndex::children`], aligned with the contract's
/// `SortMode` (`packages/contract/src/scan.ts:6` — `['size', 'name', 'path']`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortMode {
  #[default]
  Size,
  Name,
  Path,
}

/// One node of the flattened scan tree.
#[derive(Clone, Debug)]
pub struct IndexEntry {
  pub id: String,
  pub name: String,
  pub path: String,
  pub size: u64,
  pub depth: u32,
  pub ignored: bool,
  pub collapsed: bool,
  pub parent: Option<String>,
  pub child_ids: Vec<String>,
}

/// Flattened index over one scan's trees.
#[derive(Debug, Default)]
pub struct ScanIndex {
  index: HashMap<String, IndexEntry>,
  root_ids: Vec<String>,
}

impl ScanIndex {
  /// Flattens every root tree into the index. Call once per scan result.
  pub fn new(trees: &[ScanNode]) -> Self {
    let mut this = Self::default();
    for root in trees {
      let root_id = walk(root, None, 0, &mut this.index);
      this.root_ids.push(root_id);
    }
    this
  }

  pub fn get(&self, node_id: &str) -> Option<&IndexEntry> {
    self.index.get(node_id)
  }

  /// Total indexed nodes across every root — the status bar's item count.
  pub fn len(&self) -> usize {
    self.index.len()
  }

  pub fn root_ids(&self) -> &[String] {
    &self.root_ids
  }

  /// All direct children of `node_id`, fully sorted. The GPUI table keeps
  /// every row (virtual scrolling), so unlike the desktop engine there is no
  /// offset/limit paging here.
  ///
  /// Order matches engine.rs `children`: size-descending with name as the
  /// tie-break by default; name/path are plain lexicographic.
  pub fn children(&self, node_id: &str, sort: SortMode) -> Option<Vec<&IndexEntry>> {
    let entry = self.index.get(node_id)?;
    let mut children: Vec<&IndexEntry> = entry
      .child_ids
      .iter()
      .filter_map(|id| self.index.get(id))
      .collect();
    match sort {
      SortMode::Name => children.sort_by(|a, b| a.name.cmp(&b.name)),
      SortMode::Path => children.sort_by(|a, b| a.path.cmp(&b.path)),
      SortMode::Size => {
        children.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)))
      }
    }
    Some(children)
  }

  /// Breadcrumb chain for `node_id`: root-first, excluding the node itself;
  /// roots get an empty chain. `None` when the node is not in this index.
  pub fn ancestors(&self, node_id: &str) -> Option<Vec<&IndexEntry>> {
    let mut chain = Vec::new();
    let mut cursor = self.index.get(node_id)?.parent.clone();
    while let Some(parent_id) = cursor {
      let parent = self.index.get(&parent_id)?;
      chain.push(parent);
      cursor = parent.parent.clone();
    }
    chain.reverse();
    Some(chain)
  }
}

/// Depth-first insert of `node` and its subtree; returns the node's id.
/// Children are inserted before their parent (engine.rs `walk_index` order).
fn walk(
  node: &ScanNode,
  parent: Option<String>,
  depth: u32,
  index: &mut HashMap<String, IndexEntry>,
) -> String {
  let path_string = node.path.to_string_lossy().to_string();
  let id = node_id_of(&path_string);
  let child_ids: Vec<String> = node
    .children
    .iter()
    .map(|child| walk(child, Some(id.clone()), depth + 1, index))
    .collect();
  index.insert(
    id.clone(),
    IndexEntry {
      id: id.clone(),
      name: Path::new(&path_string)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path_string.clone()),
      path: path_string,
      size: node.size,
      depth,
      ignored: node.ignored,
      collapsed: node.collapsed,
      parent,
      child_ids,
    },
  );
  id
}

/// sha1(path)[..24] — the node-id scheme shared with the desktop engine
/// (engine.rs:241-246), so ids stay interchangeable across products. Public
/// because cleanup candidates are collected by path, not by scan-tree node,
/// and must land in the same id space.
pub fn node_id_of(path: &str) -> String {
  let mut hasher = Sha1::new();
  hasher.update(path.as_bytes());
  let digest = hasher.finalize();
  digest
    .iter()
    .take(12)
    .map(|byte| format!("{byte:02x}"))
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn node(path: &str, size: u64, children: Vec<ScanNode>) -> ScanNode {
    ScanNode {
      name: String::new(), // engine's walk derives names from the path
      path: path.into(),
      size,
      children,
      depth: 0,
      ignored: false,
      collapsed: false,
    }
  }

  #[test]
  fn node_id_is_sha1_prefix_and_stable() {
    // Known SHA-1 test vector: sha1("abc") = a9993e36...; the id is its
    // first 24 hex chars.
    let tree = [node("abc", 1, vec![])];
    let index = ScanIndex::new(&tree);
    let id = index.root_ids()[0].clone();
    assert_eq!(id, "a9993e364706816aba3e2571");
    // Same path → same id across rebuilds; ids are 24 lowercase hex chars.
    let again = ScanIndex::new(&tree);
    assert_eq!(again.root_ids()[0], id);
    assert_eq!(id.len(), 24);
    assert!(id
      .bytes()
      .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
  }

  #[test]
  fn walk_flattens_depth_parent_and_flags() {
    let tree = [node(
      "/tmp/proj",
      1400,
      vec![
        node(
          "/tmp/proj/node_modules",
          600,
          vec![node("/tmp/proj/node_modules/foo", 100, vec![])],
        ),
        node("/tmp/proj/target", 600, vec![]),
        node("/tmp/proj/src", 200, vec![]),
      ],
    )];
    let index = ScanIndex::new(&tree);
    assert_eq!(index.root_ids().len(), 1);

    let root = index.get(&index.root_ids()[0]).unwrap();
    assert_eq!(
      (root.name.as_str(), root.path.as_str(), root.depth),
      ("proj", "/tmp/proj", 0)
    );
    assert_eq!(root.child_ids.len(), 3);

    let foo = index
      .get(&node_id_of("/tmp/proj/node_modules/foo"))
      .unwrap();
    assert_eq!(foo.name, "foo");
    assert_eq!(foo.depth, 2);
    let modules = index.get(&foo.parent.clone().unwrap()).unwrap();
    assert_eq!(modules.name, "node_modules");
    assert!(modules.child_ids.iter().any(|id| *id == foo.id));

    // Size aggregates up the tree unchanged; flags come from the ScanNode.
    assert_eq!(root.size, 1400);
  }

  #[test]
  fn children_size_descends_with_name_tiebreak() {
    let tree = [node(
      "/tmp/proj",
      1400,
      vec![
        node("/tmp/proj/node_modules", 600, vec![]),
        node("/tmp/proj/target", 600, vec![]), // same size: name breaks the tie
        node("/tmp/proj/src", 200, vec![]),
      ],
    )];
    let index = ScanIndex::new(&tree);
    let root_id = index.root_ids()[0].clone();
    let names: Vec<&str> = index
      .children(&root_id, SortMode::Size)
      .unwrap()
      .iter()
      .map(|e| e.name.as_str())
      .collect();
    assert_eq!(names, ["node_modules", "target", "src"]);
  }

  #[test]
  fn children_name_and_path_orders() {
    let tree = [node(
      "/tmp/proj",
      1400,
      vec![
        node("/tmp/proj/node_modules", 600, vec![]),
        node("/tmp/proj/target", 600, vec![]),
        node("/tmp/proj/src", 200, vec![]),
      ],
    )];
    let index = ScanIndex::new(&tree);
    let root_id = index.root_ids()[0].clone();
    let names = |sort| {
      let picked: Vec<&str> = index
        .children(&root_id, sort)
        .unwrap()
        .iter()
        .map(|e| e.name.as_str())
        .collect();
      picked
    };
    assert_eq!(names(SortMode::Name), ["node_modules", "src", "target"]);
    assert_eq!(names(SortMode::Path), ["node_modules", "src", "target"]);
  }

  #[test]
  fn children_returns_every_row_no_paging() {
    let kids: Vec<ScanNode> = (0..250)
      .map(|i| node(&format!("/r/k{i:03}"), i, vec![]))
      .collect();
    let tree = [node("/r", 0, kids)];
    let index = ScanIndex::new(&tree);
    let root_id = index.root_ids()[0].clone();
    assert_eq!(index.children(&root_id, SortMode::Size).unwrap().len(), 250);
  }

  #[test]
  fn ancestors_run_root_first_and_exclude_self() {
    let tree = [node(
      "/tmp/proj",
      1400,
      vec![node(
        "/tmp/proj/node_modules",
        600,
        vec![node("/tmp/proj/node_modules/foo", 100, vec![])],
      )],
    )];
    let index = ScanIndex::new(&tree);
    let foo_id = node_id_of("/tmp/proj/node_modules/foo");
    let chain: Vec<&str> = index
      .ancestors(&foo_id)
      .unwrap()
      .iter()
      .map(|e| e.path.as_str())
      .collect();
    assert_eq!(chain, ["/tmp/proj", "/tmp/proj/node_modules"]);
    // A root's breadcrumb is empty; unknown ids yield None.
    let root_id = index.root_ids()[0].clone();
    assert!(index.ancestors(&root_id).unwrap().is_empty());
    assert!(index.ancestors("missing").is_none());
    assert!(index.children("missing", SortMode::Size).is_none());
  }

  #[test]
  fn multiple_roots_land_in_root_ids() {
    let tree = [node("/a", 1, vec![]), node("/b", 2, vec![])];
    let index = ScanIndex::new(&tree);
    assert_eq!(index.root_ids().len(), 2);
    assert_eq!(index.get(&index.root_ids()[1]).unwrap().path, "/b");
  }

  #[test]
  fn path_without_file_name_falls_back_to_whole_path() {
    let tree = [node("/", 1, vec![])];
    let index = ScanIndex::new(&tree);
    let root = index.get(&index.root_ids()[0]).unwrap();
    assert_eq!(root.name, "/");
  }

  #[test]
  fn walk_preserves_ignored_and_collapsed_flags() {
    let mut flagged = node("/r/ignored-dir", 10, vec![]);
    flagged.ignored = true;
    flagged.collapsed = true;
    let tree = [node("/r", 10, vec![flagged])];
    let index = ScanIndex::new(&tree);
    let entry = index.get(&node_id_of("/r/ignored-dir")).unwrap();
    assert!(entry.ignored);
    assert!(entry.collapsed);
    let root = index.get(&index.root_ids()[0]).unwrap();
    assert!(!root.ignored);
    assert!(!root.collapsed);
  }
}
