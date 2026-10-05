//! Read-only filesystem discovery shared by Space Lens product shells:
//! large files, disposable developer caches, and gitignored paths. Every
//! entry is derived from engine scans; nothing here deletes, moves, or
//! evicts anything.

#![deny(clippy::all)]

use serde::Serialize;
use space_lens::{scan_directory, IgnoredMode, ScanNode, ScanOptions};
use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// The discovery views, mirroring the desktop workbench's discovery panel.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum DiscoveryKind {
  LargeFiles,
  Caches,
  Gitignored,
}

impl DiscoveryKind {
  pub const ALL: [DiscoveryKind; 3] = [
    DiscoveryKind::LargeFiles,
    DiscoveryKind::Caches,
    DiscoveryKind::Gitignored,
  ];

  pub fn id(self) -> &'static str {
    match self {
      DiscoveryKind::LargeFiles => "large-files",
      DiscoveryKind::Caches => "caches",
      DiscoveryKind::Gitignored => "gitignored",
    }
  }

  pub fn parse(id: &str) -> Option<DiscoveryKind> {
    match id {
      "large-files" => Some(DiscoveryKind::LargeFiles),
      "caches" => Some(DiscoveryKind::Caches),
      "gitignored" => Some(DiscoveryKind::Gitignored),
      _ => None,
    }
  }
}

#[derive(Clone, Debug)]
pub struct DiscoveryOptions {
  pub roots: Vec<PathBuf>,
  pub ignore_hidden: bool,
  /// Items smaller than this many bytes are dropped from every view.
  pub min_size: u64,
  /// Per-view cap, applied after the size-descending sort.
  pub limit: usize,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryEntry {
  pub path: String,
  pub name: String,
  pub size: u64,
  pub depth: u32,
  pub is_directory: bool,
  /// True when the path itself or an ancestor is matched by .gitignore.
  pub ignored: bool,
  /// Present on cache entries: the heuristic category, e.g. "Node dependencies".
  pub category: Option<&'static str>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryView {
  pub kind: &'static str,
  pub items: Vec<DiscoveryEntry>,
  /// Matches after `min_size`, before `limit`.
  pub total: usize,
  pub total_size: u64,
  pub truncated: bool,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryReport {
  pub roots: Vec<String>,
  pub views: Vec<DiscoveryView>,
}

#[derive(Debug)]
pub struct DiscoveryError(pub String);

impl fmt::Display for DiscoveryError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&self.0)
  }
}

impl std::error::Error for DiscoveryError {}

pub fn run_discovery(options: DiscoveryOptions) -> Result<DiscoveryReport, DiscoveryError> {
  // Resolve roots once, up front. Ancestors may legitimately be reached
  // through system symlinks (macOS /tmp, /var), so only the final component
  // must be a real path; everything below is then compared canonically.
  let roots = canonical_roots(&options.roots)?;

  let scan_options = |respect_gitignore: bool| ScanOptions {
    directories: roots.clone(),
    ignore_hidden: options.ignore_hidden,
    full_path: true,
    respect_gitignore,
    ignored_mode: IgnoredMode::Summarize,
    follow_symlinks: false,
  };
  // The full tree enumerates files and cache directories; the gitignore view
  // only classifies rules. Same two-scan split as the desktop workbench.
  let full = scan_directory(scan_options(false));
  let summarized = scan_directory(scan_options(true));
  let mut ignored_paths: HashSet<PathBuf> = HashSet::new();
  for root in &summarized {
    collect_ignored_paths(root, &mut ignored_paths);
  }

  let mut large_files = Vec::new();
  let mut caches = Vec::new();
  let mut gitignored = Vec::new();
  for root in &full {
    walk_classify(
      root,
      0,
      &roots,
      &ignored_paths,
      &mut large_files,
      &mut caches,
      &mut gitignored,
    );
  }

  // Nested cache/gitignored rows collapse into their outermost ancestor;
  // large files stay individual — every file is its own cleanup target.
  prune_overlap(&mut caches);
  prune_overlap(&mut gitignored);

  Ok(DiscoveryReport {
    roots: roots
      .iter()
      .map(|root| root.to_string_lossy().to_string())
      .collect(),
    views: vec![
      finish_view(
        DiscoveryKind::LargeFiles,
        large_files,
        options.min_size,
        options.limit,
      ),
      finish_view(
        DiscoveryKind::Caches,
        caches,
        options.min_size,
        options.limit,
      ),
      finish_view(
        DiscoveryKind::Gitignored,
        gitignored,
        options.min_size,
        options.limit,
      ),
    ],
  })
}

fn canonical_roots(requested: &[PathBuf]) -> Result<Vec<PathBuf>, DiscoveryError> {
  let mut roots = Vec::with_capacity(requested.len());
  for root in requested {
    match fs::symlink_metadata(root) {
      Ok(metadata) if metadata.file_type().is_symlink() => {
        return Err(DiscoveryError(format!(
          "scan root {} is a symbolic link; pass the resolved path instead",
          root.display()
        )));
      }
      Ok(_) => {}
      Err(_) => {
        return Err(DiscoveryError(format!(
          "scan root {} does not exist",
          root.display()
        )));
      }
    }
    roots.push(fs::canonicalize(root).map_err(|error| {
      DiscoveryError(format!(
        "scan root {} could not be resolved: {error}",
        root.display()
      ))
    })?);
  }
  Ok(roots)
}

fn finish_view(
  kind: DiscoveryKind,
  mut items: Vec<DiscoveryEntry>,
  min_size: u64,
  limit: usize,
) -> DiscoveryView {
  items.retain(|item| item.size >= min_size);
  items.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
  let total = items.len();
  let total_size = items.iter().map(|item| item.size).sum();
  let truncated = total > limit;
  items.truncate(limit);
  DiscoveryView {
    kind: kind.id(),
    items,
    total,
    total_size,
    truncated,
  }
}

fn walk_classify(
  node: &ScanNode,
  depth: u32,
  roots: &[PathBuf],
  ignored_paths: &HashSet<PathBuf>,
  large_files: &mut Vec<DiscoveryEntry>,
  caches: &mut Vec<DiscoveryEntry>,
  gitignored: &mut Vec<DiscoveryEntry>,
) {
  let path = &node.path;
  // Fail closed if a path or any ancestor was replaced by a link after the
  // scan; canonical paths must still lie under an original scan root.
  if let Some(metadata) = safe_metadata(path, roots) {
    let is_directory = metadata.is_dir();
    let ignored = path
      .ancestors()
      .any(|ancestor| ignored_paths.contains(ancestor));
    let entry = |category: Option<&'static str>, is_directory: bool| DiscoveryEntry {
      path: path.to_string_lossy().to_string(),
      name: path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string()),
      size: node.size,
      depth,
      is_directory,
      ignored,
      category,
    };
    if metadata.is_file() {
      large_files.push(entry(None, false));
    } else if is_directory {
      if let Some(category) = cache_category(path, roots) {
        caches.push(entry(Some(category), true));
      }
    }
    if ignored_paths.contains(path) {
      gitignored.push(entry(Some("Gitignored"), is_directory));
    }
  }
  for child in &node.children {
    walk_classify(
      child,
      depth + 1,
      roots,
      ignored_paths,
      large_files,
      caches,
      gitignored,
    );
  }
}

fn collect_ignored_paths(node: &ScanNode, out: &mut HashSet<PathBuf>) {
  if node.ignored {
    out.insert(node.path.clone());
  }
  for child in &node.children {
    collect_ignored_paths(child, out);
  }
}

/// Classify a directory as a disposable developer cache. Environments
/// (.venv, venv) contain installed packages, not disposable build caches.
pub fn cache_category(path: &Path, roots: &[PathBuf]) -> Option<&'static str> {
  let parent = path.parent()?;
  if path
    .ancestors()
    .take_while(|ancestor| roots.iter().any(|root| ancestor.starts_with(root)))
    .any(|ancestor| {
      matches!(
        ancestor.file_name().and_then(|name| name.to_str()),
        Some(".venv" | "venv")
      ) || ancestor.join("pyvenv.cfg").is_file()
    })
  {
    return None;
  }
  match path.file_name()?.to_str()? {
    "node_modules" if parent.join("package.json").is_file() => Some("Node dependencies"),
    "target" if parent.join("Cargo.toml").is_file() => Some("Rust build output"),
    "__pycache__" => Some("Python bytecode"),
    ".pytest_cache" => Some("pytest cache"),
    ".mypy_cache" => Some("mypy cache"),
    ".ruff_cache" => Some("Ruff cache"),
    ".next" | ".nuxt" | ".turbo" | ".parcel-cache" | ".svelte-kit"
      if parent.join("package.json").is_file() =>
    {
      Some("JavaScript build cache")
    }
    _ => None,
  }
}

fn prune_overlap(rows: &mut Vec<DiscoveryEntry>) {
  rows.sort_by(|a, b| {
    Path::new(&a.path)
      .components()
      .count()
      .cmp(&Path::new(&b.path).components().count())
      .then_with(|| a.path.cmp(&b.path))
  });
  let mut kept: Vec<DiscoveryEntry> = Vec::new();
  for row in rows.drain(..) {
    if !kept
      .iter()
      .any(|parent| Path::new(&row.path).starts_with(&parent.path))
    {
      kept.push(row);
    }
  }
  *rows = kept;
}

/// Never follow links themselves and never accept a path that escaped its
/// root through a canonicalization race.
fn safe_metadata(path: &Path, roots: &[PathBuf]) -> Option<fs::Metadata> {
  if !roots.iter().any(|root| path.starts_with(root)) {
    return None;
  }
  let metadata = fs::symlink_metadata(path).ok()?;
  if metadata.file_type().is_symlink() {
    return None;
  }
  let canonical = fs::canonicalize(path).ok()?;
  if canonical != path || !roots.iter().any(|root| canonical.starts_with(root)) {
    return None;
  }
  Some(metadata)
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::io::Write;

  struct Fixture {
    root: PathBuf,
  }

  impl Fixture {
    fn new(tag: &str) -> Fixture {
      let root =
        std::env::temp_dir().join(format!("spacelens-discovery-{tag}-{}", std::process::id()));
      let _ = fs::remove_dir_all(&root);
      fs::create_dir_all(&root).unwrap();
      Fixture { root }
    }

    fn dir(&self, relative: &str) -> PathBuf {
      let path = self.root.join(relative);
      fs::create_dir_all(&path).unwrap();
      path
    }

    fn file(&self, relative: &str, bytes: usize) -> PathBuf {
      let path = self.root.join(relative);
      if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
      }
      let mut file = fs::File::create(&path).unwrap();
      if bytes > 0 {
        file.write_all(&vec![0u8; bytes]).unwrap();
      }
      path
    }

    fn view(report: &DiscoveryReport, kind: DiscoveryKind) -> &DiscoveryView {
      report
        .views
        .iter()
        .find(|view| view.kind == kind.id())
        .unwrap()
    }

    fn paths(view: &DiscoveryView) -> Vec<String> {
      view.items.iter().map(|item| item.path.clone()).collect()
    }
  }

  impl Drop for Fixture {
    fn drop(&mut self) {
      let _ = fs::remove_dir_all(&self.root);
    }
  }

  fn options(root: &Path, min_size: u64, limit: usize) -> DiscoveryOptions {
    DiscoveryOptions {
      roots: vec![root.to_path_buf()],
      ignore_hidden: false,
      min_size,
      limit,
    }
  }

  #[test]
  fn classifies_caches_prunes_nesting_and_excludes_environments() {
    let fixture = Fixture::new("caches");
    let project = fixture.dir("project");
    fixture.file("project/package.json", 2);
    fixture.file("project/Cargo.toml", 2);
    fixture.dir("project/node_modules/vendor/node_modules");
    fixture.file("project/node_modules/vendor/package.json", 2);
    fixture.dir("project/target");
    fixture.dir("project/__pycache__");
    fixture.file("project/.venv/pyvenv.cfg", 2);
    fixture.file("project/.venv/x/package.json", 2);
    fixture.dir("project/.venv/x/node_modules");

    let report = run_discovery(options(&project, 0, 200)).unwrap();
    let caches = Fixture::view(&report, DiscoveryKind::Caches);
    let paths = Fixture::paths(caches);
    assert!(paths
      .iter()
      .any(|path| path.ends_with("project/node_modules")));
    assert!(paths.iter().any(|path| path.ends_with("project/target")));
    assert!(paths
      .iter()
      .any(|path| path.ends_with("project/__pycache__")));
    // Nested node_modules collapses into the outermost ancestor.
    assert!(!paths
      .iter()
      .any(|path| path.contains("vendor/node_modules")));
    // Virtualenv contents are installed packages, not disposable caches.
    assert!(!paths.iter().any(|path| path.contains(".venv")));
    let node_modules = caches
      .items
      .iter()
      .find(|item| item.path.ends_with("project/node_modules"))
      .unwrap();
    assert_eq!(node_modules.category, Some("Node dependencies"));
  }

  #[test]
  fn lists_gitignored_paths_and_files_inside_them() {
    let fixture = Fixture::new("gitignored");
    let project = fixture.dir("project");
    fs::write(project.join(".gitignore"), "build\n").unwrap();
    let ignored_binary = fixture.file("project/build/ignored.bin", 2 * 1024 * 1024);
    fixture.file("project/kept.txt", 2);

    let report = run_discovery(options(&project, 0, 200)).unwrap();
    let gitignored = Fixture::view(&report, DiscoveryKind::Gitignored);
    assert!(Fixture::paths(gitignored)
      .iter()
      .any(|path| path.ends_with("project/build")));
    assert_eq!(gitignored.items[0].category, Some("Gitignored"));

    // The full enumeration still measures gitignored subtrees, so large
    // files inside them show up as reclaimable individual targets. Report
    // paths are canonical (macOS temp roots live under symlinked ancestors).
    let large = Fixture::view(&report, DiscoveryKind::LargeFiles);
    let ignored_binary = ignored_binary.canonicalize().unwrap();
    assert!(Fixture::paths(large)
      .iter()
      .any(|path| Path::new(path) == ignored_binary));
  }

  #[test]
  fn min_size_and_limit_bound_every_view() {
    let fixture = Fixture::new("limits");
    let project = fixture.dir("project");
    fixture.file("project/big.bin", 3 * 1024 * 1024);
    fixture.file("project/small.bin", 1024 * 1024);
    fixture.file("project/tiny.txt", 4);

    let report = run_discovery(options(&project, 2 * 1024 * 1024, 200)).unwrap();
    let large = Fixture::view(&report, DiscoveryKind::LargeFiles);
    let paths = Fixture::paths(large);
    assert!(paths.iter().any(|path| path.ends_with("big.bin")));
    assert!(!paths.iter().any(|path| path.ends_with("small.bin")));
    assert!(!paths.iter().any(|path| path.ends_with("tiny.txt")));
    assert_eq!(large.total, 1);
    assert!(!large.truncated);

    let capped = run_discovery(options(&project, 0, 1)).unwrap();
    let large = Fixture::view(&capped, DiscoveryKind::LargeFiles);
    assert_eq!(large.items.len(), 1);
    assert_eq!(large.total, 3);
    assert!(large.truncated);
    assert!(large.items[0].path.ends_with("big.bin"));
  }

  #[test]
  fn missing_root_fails_closed() {
    let missing = std::env::temp_dir().join("spacelens-discovery-missing-root");
    let error = run_discovery(options(&missing, 0, 200)).unwrap_err();
    assert!(error.0.contains("does not exist"));
  }

  #[test]
  fn symlinked_root_fails_closed() {
    let fixture = Fixture::new("symlink-root");
    let real = fixture.dir("real");
    let link = fixture.root.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let error = run_discovery(options(&link, 0, 200)).unwrap_err();
    assert!(error.0.contains("symbolic link"));
  }

  #[test]
  fn kinds_round_trip_through_ids() {
    for kind in DiscoveryKind::ALL {
      assert_eq!(DiscoveryKind::parse(kind.id()), Some(kind));
    }
    assert_eq!(DiscoveryKind::parse("other"), None);
  }
}
