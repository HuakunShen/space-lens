//! Compact node store behind one scan. Browse, search, discovery and cleanup
//! planning all resolve through it.
//!
//! A full-disk scan holds millions of nodes, so nothing here is a `String`
//! per node: one flat `Vec<Node>` (48 bytes a node), names in one byte
//! buffer, children as ranges into one `u32` slot list, and paths rebuilt on
//! demand from the parent chain. Only nodes whose path is not
//! `parent path + separator + name` (roots, app-scan leaves) keep their path
//! verbatim. Wire ids are unchanged — `sha1(path)[..12]` hex, plus the
//! `approot_`/`appbundle_` app-scan prefixes — and resolve through a sorted
//! digest index.

use std::collections::HashMap;
use std::path::MAIN_SEPARATOR;

use sha1::{Digest, Sha1};

use super::{CleanupSnapshot, EntryFingerprint, TreeNodeSummary};

/// "No node": a root's parent, or a child slot whose id never resolved.
pub(super) const NONE: u32 = u32::MAX;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
enum IdKind {
  Path = 0,
  AppRoot = 1,
  AppBundle = 2,
}

/// A wire node id without its text: the id family plus the 12-byte digest.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub(super) struct NodeId {
  kind: IdKind,
  digest: [u8; 12],
}

fn digest12(bytes: &[u8]) -> [u8; 12] {
  let mut hasher = Sha1::new();
  hasher.update(bytes);
  let mut digest = [0u8; 12];
  digest.copy_from_slice(&hasher.finalize()[..12]);
  digest
}

fn nibble(byte: u8) -> Option<u8> {
  match byte {
    b'0'..=b'9' => Some(byte - b'0'),
    b'a'..=b'f' => Some(byte - b'a' + 10),
    _ => None,
  }
}

impl NodeId {
  /// The ordinary id: `sha1(path)[..12]` as lowercase hex.
  pub fn of_path(path: &str) -> Self {
    Self {
      kind: IdKind::Path,
      digest: digest12(path.as_bytes()),
    }
  }

  /// `approot_` + sha1(seed)[..12]; the seed names one audit generation + app.
  pub fn app_root(seed: &str) -> Self {
    Self {
      kind: IdKind::AppRoot,
      digest: digest12(seed.as_bytes()),
    }
  }

  /// `appbundle_` + sha1(bundle path)[..12].
  pub fn app_bundle(path: &str) -> Self {
    Self {
      kind: IdKind::AppBundle,
      digest: digest12(path.as_bytes()),
    }
  }

  /// Parses a wire id. Only the exact lowercase form the engine emits
  /// resolves, as with the string-keyed map this replaces.
  pub fn parse(wire: &str) -> Option<Self> {
    let (kind, hex) = if let Some(hex) = wire.strip_prefix("approot_") {
      (IdKind::AppRoot, hex)
    } else if let Some(hex) = wire.strip_prefix("appbundle_") {
      (IdKind::AppBundle, hex)
    } else {
      (IdKind::Path, wire)
    };
    let hex = hex.as_bytes();
    if hex.len() != 24 {
      return None;
    }
    let mut digest = [0u8; 12];
    for (slot, pair) in digest.iter_mut().zip(hex.chunks(2)) {
      *slot = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Some(Self { kind, digest })
  }

  pub fn hex(&self) -> String {
    self
      .digest
      .iter()
      .map(|byte| format!("{byte:02x}"))
      .collect()
  }

  pub fn to_wire(self) -> String {
    let prefix = match self.kind {
      IdKind::Path => "",
      IdKind::AppRoot => "approot_",
      IdKind::AppBundle => "appbundle_",
    };
    format!("{prefix}{}", self.hex())
  }
}

const IGNORED: u8 = 1;
const COLLAPSED: u8 = 1 << 1;
const HAS_LOGICAL: u8 = 1 << 2;
const DIRECTORY_KNOWN: u8 = 1 << 3;
const DIRECTORY: u8 = 1 << 4;
const EXPLICIT_PATH: u8 = 1 << 5;
const KIND_SHIFT: u8 = 6;

#[derive(Clone, Copy)]
struct Node {
  size: u64,
  logical_size: u64,
  digest: [u8; 12],
  parent: u32,
  children_start: u32,
  children_len: u32,
  /// Offset of the LEB128-length-prefixed name in `names`.
  name: u32,
  /// Index into the interned (scan state, skip reason) pairs.
  labels: u16,
  flags: u8,
}

/// Everything one node contributes when it is added.
pub(super) struct NodeFacts<'a> {
  pub id: NodeId,
  pub name: &'a str,
  pub path: &'a str,
  pub parent: u32,
  /// The parent's path, when known; a child whose path is exactly
  /// `parent_path + separator + name` stores no path of its own.
  pub parent_path: Option<&'a str>,
  pub size: u64,
  pub logical_size: Option<u64>,
  pub is_directory: Option<bool>,
  pub ignored: bool,
  pub collapsed: bool,
  pub scan_state: Option<&'a str>,
  pub skip_reason: Option<&'a str>,
  pub fingerprint: Option<EntryFingerprint>,
}

type Labels = (Option<Box<str>>, Option<Box<str>>);

#[derive(Default)]
pub(super) struct NodeIndex {
  nodes: Vec<Node>,
  slots: Vec<u32>,
  names: Vec<u8>,
  /// Node indices sorted by id; duplicates resolved to the last added node.
  by_id: Vec<u32>,
  paths: HashMap<u32, Box<str>>,
  labels: Vec<Labels>,
  /// Scan-time fingerprints (legacy walks only; protected scans never stat).
  fingerprints: HashMap<u32, EntryFingerprint>,
  /// Discovery-time snapshots that cleanup prefers over browse values.
  pub cleanup: HashMap<u32, CleanupSnapshot>,
}

/// Whether `path` is `parent + separator + name`, without a doubled
/// separator after a root such as `/` — the rule `push_path` rebuilds.
fn is_joined(parent: &str, name: &str, path: &str) -> bool {
  let Some(rest) = path.strip_prefix(parent) else {
    return false;
  };
  let rest = if parent.ends_with(MAIN_SEPARATOR) {
    Some(rest)
  } else {
    rest.strip_prefix(MAIN_SEPARATOR)
  };
  rest == Some(name)
}

/// Gives back spare capacity only when it is worth a copy: reallocating a
/// buffer of hundreds of megabytes leaves the old one cached by the allocator.
fn shrink_if_slack<T>(buffer: &mut Vec<T>) {
  if buffer.capacity() - buffer.len() > buffer.len() / 8 {
    buffer.shrink_to_fit();
  }
}

impl NodeIndex {
  pub fn reserve(&mut self, nodes: usize, name_bytes: usize) {
    self.nodes.reserve_exact(nodes);
    self.slots.reserve_exact(nodes);
    self.names.reserve_exact(name_bytes);
  }

  pub fn push(&mut self, facts: NodeFacts<'_>) -> u32 {
    let index = u32::try_from(self.nodes.len())
      .ok()
      .filter(|index| *index != NONE)
      .expect("scan index holds at most u32::MAX - 1 nodes");
    let name = u32::try_from(self.names.len()).expect("scan index names exceed 4 GiB");
    let mut length = facts.name.len();
    loop {
      let byte = (length & 0x7f) as u8;
      length >>= 7;
      if length == 0 {
        self.names.push(byte);
        break;
      }
      self.names.push(byte | 0x80);
    }
    self.names.extend_from_slice(facts.name.as_bytes());
    let derived = facts.parent != NONE
      && facts
        .parent_path
        .is_some_and(|parent| is_joined(parent, facts.name, facts.path));
    let mut flags = (facts.id.kind as u8) << KIND_SHIFT;
    if facts.ignored {
      flags |= IGNORED;
    }
    if facts.collapsed {
      flags |= COLLAPSED;
    }
    if facts.logical_size.is_some() {
      flags |= HAS_LOGICAL;
    }
    if let Some(directory) = facts.is_directory {
      flags |= DIRECTORY_KNOWN;
      if directory {
        flags |= DIRECTORY;
      }
    }
    if !derived {
      flags |= EXPLICIT_PATH;
      self.paths.insert(index, facts.path.into());
    }
    if let Some(fingerprint) = facts.fingerprint {
      self.fingerprints.insert(index, fingerprint);
    }
    let labels = self.intern(facts.scan_state, facts.skip_reason);
    self.nodes.push(Node {
      size: facts.size,
      logical_size: facts.logical_size.unwrap_or(0),
      digest: facts.id.digest,
      parent: facts.parent,
      children_start: 0,
      children_len: 0,
      name,
      labels,
      flags,
    });
    index
  }

  fn intern(&mut self, scan_state: Option<&str>, skip_reason: Option<&str>) -> u16 {
    if self.labels.is_empty() {
      self.labels.push((None, None));
    }
    if let Some(found) = self.labels.iter().position(|(state, reason)| {
      state.as_deref() == scan_state && reason.as_deref() == skip_reason
    }) {
      return found as u16;
    }
    let next =
      u16::try_from(self.labels.len()).expect("scan index holds at most 65536 label pairs");
    self
      .labels
      .push((scan_state.map(Into::into), skip_reason.map(Into::into)));
    next
  }

  /// Records `node`'s children in order. A `NONE` slot is a child id that
  /// resolved to nothing: it counts toward `child_count` but never lists.
  pub fn set_children(&mut self, node: u32, children: &[u32]) {
    let start = u32::try_from(self.slots.len()).expect("scan index child slots exceed u32");
    self.slots.extend_from_slice(children);
    let entry = &mut self.nodes[node as usize];
    entry.children_start = start;
    entry.children_len = children.len() as u32;
  }

  pub fn set_size(&mut self, node: u32, size: u64, logical_size: Option<u64>) {
    let entry = &mut self.nodes[node as usize];
    entry.size = size;
    entry.logical_size = logical_size.unwrap_or(0);
    if logical_size.is_some() {
      entry.flags |= HAS_LOGICAL;
    } else {
      entry.flags &= !HAS_LOGICAL;
    }
  }

  pub fn set_ignored(&mut self, node: u32, ignored: bool) {
    let entry = &mut self.nodes[node as usize];
    if ignored {
      entry.flags |= IGNORED;
    } else {
      entry.flags &= !IGNORED;
    }
  }

  /// Rebuilds the id lookup after nodes were added. Two nodes with one id
  /// resolve to the later one, as overwriting a map entry did; child slots
  /// and parents that named the earlier copy follow it to the survivor.
  ///
  /// Digests are uniform, so a counting sort on the id family and the first
  /// two digest bytes leaves buckets of a few dozen nodes. That keeps the
  /// only large allocation the lookup itself: freed scratch buffers of this
  /// size stay resident in the macOS large-allocation cache.
  pub fn seal(&mut self) {
    const BUCKETS: usize = 3 << 16;
    let bucket = |node: &Node| {
      (((node.flags >> KIND_SHIFT) as usize) << 16)
        | ((node.digest[0] as usize) << 8)
        | node.digest[1] as usize
    };
    let mut starts = vec![0u32; BUCKETS + 1];
    for node in &self.nodes {
      starts[bucket(node) + 1] += 1;
    }
    for position in 1..=BUCKETS {
      starts[position] += starts[position - 1];
    }
    let mut by_id = std::mem::take(&mut self.by_id);
    by_id.clear();
    by_id.resize(self.nodes.len(), NONE);
    let mut cursor = starts.clone();
    for (index, node) in self.nodes.iter().enumerate() {
      let slot = &mut cursor[bucket(node)];
      by_id[*slot as usize] = index as u32;
      *slot += 1;
    }
    drop(cursor);
    let key = |index: u32| {
      let node = &self.nodes[index as usize];
      (node.flags >> KIND_SHIFT, node.digest, index)
    };
    for window in starts.windows(2) {
      by_id[window[0] as usize..window[1] as usize].sort_unstable_by_key(|index| key(*index));
    }
    drop(starts);
    // Collapse each run of one id to its last (highest-index) node.
    let mut replaced: HashMap<u32, u32> = HashMap::new();
    let mut kept = 0;
    for position in 0..by_id.len() {
      let current = by_id[position];
      if let Some(&next) = by_id.get(position + 1) {
        if key(next).1 == key(current).1 && key(next).0 == key(current).0 {
          replaced.insert(current, next);
          continue;
        }
      }
      by_id[kept] = current;
      kept += 1;
    }
    by_id.truncate(kept);
    if !replaced.is_empty() {
      // A chain of three copies maps the first to the second; follow it.
      let survivor = |mut index: u32| {
        while let Some(next) = replaced.get(&index) {
          index = *next;
        }
        index
      };
      for slot in &mut self.slots {
        if *slot != NONE {
          *slot = survivor(*slot);
        }
      }
      for node in &mut self.nodes {
        if node.parent != NONE {
          node.parent = survivor(node.parent);
        }
      }
      by_id.shrink_to_fit();
    }
    self.by_id = by_id;
    shrink_if_slack(&mut self.nodes);
    shrink_if_slack(&mut self.slots);
    shrink_if_slack(&mut self.names);
  }

  /// Distinct ids held (what the old map's `len()` counted).
  pub fn len(&self) -> usize {
    self.by_id.len()
  }

  /// Every resolvable node, one per id.
  pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
    self.by_id.iter().copied()
  }

  pub fn get(&self, id: NodeId) -> Option<u32> {
    let key = (id.kind as u8, id.digest);
    self
      .by_id
      .binary_search_by(|&index| {
        let node = &self.nodes[index as usize];
        (node.flags >> KIND_SHIFT, node.digest).cmp(&key)
      })
      .ok()
      .map(|position| self.by_id[position])
  }

  /// Resolves a wire id.
  pub fn find(&self, wire: &str) -> Option<u32> {
    NodeId::parse(wire).and_then(|id| self.get(id))
  }

  fn node(&self, index: u32) -> &Node {
    &self.nodes[index as usize]
  }

  pub fn id(&self, index: u32) -> NodeId {
    let node = self.node(index);
    let kind = match node.flags >> KIND_SHIFT {
      1 => IdKind::AppRoot,
      2 => IdKind::AppBundle,
      _ => IdKind::Path,
    };
    NodeId {
      kind,
      digest: node.digest,
    }
  }

  pub fn wire_id(&self, index: u32) -> String {
    self.id(index).to_wire()
  }

  pub fn name(&self, index: u32) -> &str {
    let mut offset = self.node(index).name as usize;
    let mut length = 0usize;
    let mut shift = 0;
    loop {
      let byte = self.names[offset];
      offset += 1;
      length |= ((byte & 0x7f) as usize) << shift;
      if byte & 0x80 == 0 {
        break;
      }
      shift += 7;
    }
    std::str::from_utf8(&self.names[offset..offset + length]).unwrap_or_default()
  }

  pub fn path(&self, index: u32) -> String {
    let mut path = String::new();
    self.push_path(index, &mut path);
    path
  }

  /// Appends the node's path to `out`.
  pub fn push_path(&self, index: u32, out: &mut String) {
    let mut chain = Vec::new();
    let mut cursor = index;
    while self.node(cursor).flags & EXPLICIT_PATH == 0 {
      chain.push(cursor);
      cursor = self.node(cursor).parent;
    }
    let start = out.len();
    out.push_str(
      self
        .paths
        .get(&cursor)
        .map(|path| &**path)
        .unwrap_or_default(),
    );
    for &node in chain.iter().rev() {
      if !out[start..].ends_with(MAIN_SEPARATOR) {
        out.push(MAIN_SEPARATOR);
      }
      out.push_str(self.name(node));
    }
  }

  /// The byte length `push_path` would append, without building the path.
  pub fn path_len(&self, index: u32) -> usize {
    let mut names = Vec::new();
    let mut cursor = index;
    while self.node(cursor).flags & EXPLICIT_PATH == 0 {
      names.push(self.name(cursor));
      cursor = self.node(cursor).parent;
    }
    let base = self
      .paths
      .get(&cursor)
      .map(|path| &**path)
      .unwrap_or_default();
    let mut length = base.len();
    let mut ends_with_separator = base.ends_with(MAIN_SEPARATOR);
    for name in names.iter().rev() {
      if !ends_with_separator {
        length += MAIN_SEPARATOR.len_utf8();
      }
      length += name.len();
      ends_with_separator = name.ends_with(MAIN_SEPARATOR);
    }
    length
  }

  /// True when this node's path is not derived from its parent's.
  pub fn has_explicit_path(&self, index: u32) -> bool {
    self.node(index).flags & EXPLICIT_PATH != 0
  }

  pub fn parent(&self, index: u32) -> Option<u32> {
    Some(self.node(index).parent).filter(|parent| *parent != NONE)
  }

  pub fn depth(&self, index: u32) -> u32 {
    let mut depth = 0;
    let mut cursor = index;
    while let Some(parent) = self.parent(cursor) {
      depth += 1;
      cursor = parent;
    }
    depth
  }

  pub fn child_count(&self, index: u32) -> usize {
    self.node(index).children_len as usize
  }

  pub fn children(&self, index: u32) -> impl Iterator<Item = u32> + '_ {
    let node = self.node(index);
    let start = node.children_start as usize;
    self.slots[start..start + node.children_len as usize]
      .iter()
      .copied()
      .filter(|child| *child != NONE)
  }

  pub fn size(&self, index: u32) -> u64 {
    self.node(index).size
  }

  pub fn logical_size(&self, index: u32) -> Option<u64> {
    let node = self.node(index);
    (node.flags & HAS_LOGICAL != 0).then_some(node.logical_size)
  }

  pub fn is_directory(&self, index: u32) -> Option<bool> {
    let flags = self.node(index).flags;
    (flags & DIRECTORY_KNOWN != 0).then_some(flags & DIRECTORY != 0)
  }

  pub fn ignored(&self, index: u32) -> bool {
    self.node(index).flags & IGNORED != 0
  }

  pub fn collapsed(&self, index: u32) -> bool {
    self.node(index).flags & COLLAPSED != 0
  }

  pub fn scan_state(&self, index: u32) -> Option<&str> {
    self.labels[self.node(index).labels as usize].0.as_deref()
  }

  pub fn skip_reason(&self, index: u32) -> Option<&str> {
    self.labels[self.node(index).labels as usize].1.as_deref()
  }

  pub fn fingerprint(&self, index: u32) -> Option<EntryFingerprint> {
    self.fingerprints.get(&index).cloned()
  }

  pub fn summary(&self, index: u32) -> TreeNodeSummary {
    let child_count = self.child_count(index);
    TreeNodeSummary {
      id: self.wire_id(index),
      name: self.name(index).to_string(),
      path: self.path(index),
      size: self.size(index),
      depth: self.depth(index),
      ignored: self.ignored(index),
      collapsed: self.collapsed(index),
      has_children: child_count > 0,
      child_count,
      logical_size: self.logical_size(index),
      is_directory: self.is_directory(index),
      scan_state: self.scan_state(index).map(str::to_string),
      skip_reason: self.skip_reason(index).map(str::to_string),
    }
  }

  /// Adds `other`'s nodes that `keep` accepts and this index lacks, linking
  /// them by id exactly as a string-keyed map merge would, then reseals.
  /// Returns `(this, other)` index pairs for kept nodes already present, so
  /// the caller can attach the newer snapshot to them.
  pub fn merge(&mut self, other: &NodeIndex, keep: impl Fn(u32) -> bool) -> Vec<(u32, u32)> {
    let mut mapped = vec![NONE; other.nodes.len()];
    let mut existing = Vec::new();
    let mut added = Vec::new();
    for source in other.iter() {
      match self.get(other.id(source)) {
        Some(target) => {
          mapped[source as usize] = target;
          if keep(source) {
            existing.push((target, source));
          }
        }
        None if keep(source) => added.push(source),
        None => {}
      }
    }
    let base = self.nodes.len();
    for (offset, source) in added.iter().enumerate() {
      mapped[*source as usize] =
        u32::try_from(base + offset).expect("scan index exceeds u32 nodes");
    }
    for &source in &added {
      let parent = other
        .parent(source)
        .map(|parent| mapped[parent as usize])
        .unwrap_or(NONE);
      let path = other.path(source);
      let parent_path = (parent != NONE && !other.has_explicit_path(source))
        .then(|| other.parent(source).map(|parent| other.path(parent)))
        .flatten();
      let node = self.push(NodeFacts {
        id: other.id(source),
        name: other.name(source),
        path: &path,
        parent,
        parent_path: parent_path.as_deref(),
        size: other.size(source),
        logical_size: other.logical_size(source),
        is_directory: other.is_directory(source),
        ignored: other.ignored(source),
        collapsed: other.collapsed(source),
        scan_state: other.scan_state(source),
        skip_reason: other.skip_reason(source),
        fingerprint: other.fingerprint(source),
      });
      let source_node = other.node(source);
      let start = source_node.children_start as usize;
      let children: Vec<u32> = other.slots[start..start + source_node.children_len as usize]
        .iter()
        .map(|child| {
          if *child == NONE {
            NONE
          } else {
            mapped[*child as usize]
          }
        })
        .collect();
      self.set_children(node, &children);
    }
    self.seal();
    existing
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn facts<'a>(
    path: &'a str,
    name: &'a str,
    parent: u32,
    parent_path: Option<&'a str>,
  ) -> NodeFacts<'a> {
    NodeFacts {
      id: NodeId::of_path(path),
      name,
      path,
      parent,
      parent_path,
      size: 1,
      logical_size: Some(1),
      is_directory: Some(true),
      ignored: false,
      collapsed: false,
      scan_state: Some("complete"),
      skip_reason: None,
      fingerprint: None,
    }
  }

  #[test]
  fn wire_ids_round_trip_and_reject_other_spellings() {
    let path = NodeId::of_path("/tmp/a");
    assert_eq!(NodeId::parse(&path.to_wire()), Some(path));
    assert_eq!(path.to_wire().len(), 24);
    let root = NodeId::app_root("app-scan:x:y");
    assert!(root.to_wire().starts_with("approot_"));
    assert_eq!(NodeId::parse(&root.to_wire()), Some(root));
    let bundle = NodeId::app_bundle("/Applications/X.app");
    assert_eq!(NodeId::parse(&bundle.to_wire()), Some(bundle));
    // The bundle leaf and a plain node at the same path stay distinct.
    assert_ne!(bundle, NodeId::of_path("/Applications/X.app"));
    assert_eq!(NodeId::parse(&path.to_wire().to_uppercase()), None);
    assert_eq!(NodeId::parse("abc"), None);
    assert_eq!(NodeId::parse(&format!("approot_{}", "z".repeat(24))), None);
  }

  #[test]
  fn paths_rebuild_from_names_and_keep_odd_paths_verbatim() {
    let mut index = NodeIndex::default();
    let root = index.push(facts("/", "/", NONE, None));
    let users = index.push(facts("/Users", "Users", root, Some("/")));
    let me = index.push(facts("/Users/me", "me", users, Some("/Users")));
    // Not parent + separator + name: stored as given.
    let odd = index.push(facts("/elsewhere/x", "x", users, Some("/Users")));
    index.set_children(root, &[users]);
    index.set_children(users, &[me, odd, NONE]);
    index.seal();
    assert_eq!(index.path(root), "/");
    assert_eq!(index.path(me), "/Users/me");
    assert_eq!(index.path(odd), "/elsewhere/x");
    for node in [root, users, me, odd] {
      assert_eq!(index.path_len(node), index.path(node).len());
    }
    assert!(!index.has_explicit_path(me));
    assert!(index.has_explicit_path(odd));
    assert_eq!(index.depth(me), 2);
    // A dangling slot counts but never lists.
    assert_eq!(index.child_count(users), 3);
    assert_eq!(index.children(users).collect::<Vec<_>>(), vec![me, odd]);
    assert_eq!(
      index.find(&NodeId::of_path("/Users/me").to_wire()),
      Some(me)
    );
    assert_eq!(index.len(), 4);
    let summary = index.summary(me);
    assert_eq!(summary.name, "me");
    assert_eq!(summary.path, "/Users/me");
    assert_eq!(summary.scan_state.as_deref(), Some("complete"));
  }

  #[test]
  fn a_repeated_id_resolves_to_the_later_node() {
    let mut index = NodeIndex::default();
    let outer = index.push(facts("/a", "a", NONE, None));
    let first = index.push(facts("/a/b", "b", outer, Some("/a")));
    index.set_children(outer, &[first]);
    let second = index.push(facts("/a/b", "/a/b", NONE, None));
    index.seal();
    assert_eq!(index.len(), 2);
    assert_eq!(index.find(&NodeId::of_path("/a/b").to_wire()), Some(second));
    assert_eq!(index.children(outer).collect::<Vec<_>>(), vec![second]);
    assert_eq!(index.depth(second), 0);
  }

  #[test]
  fn merge_links_new_nodes_by_id_and_reports_shared_ones() {
    let mut browse = NodeIndex::default();
    let root = browse.push(facts("/r", "/r", NONE, None));
    let shown = browse.push(facts("/r/a", "a", root, Some("/r")));
    browse.set_children(root, &[shown]);
    browse.seal();

    let mut found = NodeIndex::default();
    let froot = found.push(facts("/r", "/r", NONE, None));
    let fa = found.push(facts("/r/a", "a", froot, Some("/r")));
    let fdeep = found.push(facts("/r/a/deep", "deep", fa, Some("/r/a")));
    let fgone = found.push(facts("/r/a/gone", "gone", fa, Some("/r/a")));
    found.set_children(froot, &[fa]);
    found.set_children(fa, &[fdeep, fgone]);
    found.seal();

    let shared = browse.merge(&found, |node| node != fgone);
    assert!(shared.contains(&(shown, fa)));
    let deep = browse
      .find(&NodeId::of_path("/r/a/deep").to_wire())
      .unwrap();
    assert_eq!(browse.path(deep), "/r/a/deep");
    assert_eq!(browse.parent(deep), Some(shown));
    assert!(browse
      .find(&NodeId::of_path("/r/a/gone").to_wire())
      .is_none());
    // Browse topology is untouched: `a` still lists no children.
    assert_eq!(browse.children(shown).count(), 0);
  }
}
