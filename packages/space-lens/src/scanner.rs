use ignore::gitignore::{Gitignore, GitignoreBuilder};
use rayon::iter::ParallelBridge;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IgnoredMode {
  Exclude,
  Summarize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanOptions {
  pub directories: Vec<PathBuf>,
  pub ignore_hidden: bool,
  pub full_path: bool,
  pub respect_gitignore: bool,
  pub ignored_mode: IgnoredMode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanNode {
  pub name: String,
  pub path: PathBuf,
  pub size: u64,
  pub children: Vec<ScanNode>,
  pub depth: u32,
  pub ignored: bool,
  pub collapsed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanProgress {
  pub path: PathBuf,
  pub bytes_scanned: u64,
  pub entries_scanned: u64,
}

type IgnoreStack = Vec<Arc<Gitignore>>;
type SeenInodes = Arc<Mutex<HashSet<(u64, u64)>>>;
type ProgressCallback = Arc<dyn Fn(ScanProgress) + Send + Sync>;

#[derive(Default)]
struct ProgressState {
  bytes_scanned: AtomicU64,
  entries_scanned: AtomicU64,
  callback: Option<ProgressCallback>,
}

struct ScanContext<'a> {
  options: &'a ScanOptions,
  seen_inodes: &'a SeenInodes,
  progress: &'a ProgressState,
}

#[derive(Clone, Copy)]
struct ScanPathState<'a> {
  depth: u32,
  ignore_stack: &'a [Arc<Gitignore>],
  ignored: bool,
  collapsed: bool,
}

impl ProgressState {
  fn new(callback: Option<ProgressCallback>) -> Self {
    Self {
      bytes_scanned: AtomicU64::new(0),
      entries_scanned: AtomicU64::new(0),
      callback,
    }
  }

  fn record(&self, path: &Path, size: u64) {
    let bytes_scanned = self.bytes_scanned.fetch_add(size, Ordering::Relaxed) + size;
    let entries_scanned = self.entries_scanned.fetch_add(1, Ordering::Relaxed) + 1;

    if let Some(callback) = &self.callback {
      callback(ScanProgress {
        path: path.to_path_buf(),
        bytes_scanned,
        entries_scanned,
      });
    }
  }
}

pub fn scan_directory(options: ScanOptions) -> Vec<ScanNode> {
  scan_directory_inner(options, None)
}

pub fn scan_directory_with_progress(
  options: ScanOptions,
  progress: impl Fn(ScanProgress) + Send + Sync + 'static,
) -> Vec<ScanNode> {
  scan_directory_inner(options, Some(Arc::new(progress)))
}

fn scan_directory_inner(options: ScanOptions, progress: Option<ProgressCallback>) -> Vec<ScanNode> {
  let seen_inodes = Arc::new(Mutex::new(HashSet::new()));
  let progress = Arc::new(ProgressState::new(progress));
  let context = ScanContext {
    options: &options,
    seen_inodes: &seen_inodes,
    progress: &progress,
  };

  options
    .directories
    .iter()
    .filter_map(|directory| scan_path(directory, ScanPathState::root(), &context))
    .collect()
}

pub fn measure_path(path: &Path) -> u64 {
  let seen_inodes = Arc::new(Mutex::new(HashSet::new()));
  summarize_path(path, &seen_inodes)
}

impl<'a> ScanPathState<'a> {
  fn root() -> Self {
    Self {
      depth: 0,
      ignore_stack: &[],
      ignored: false,
      collapsed: false,
    }
  }
}

fn scan_path(path: &Path, state: ScanPathState<'_>, context: &ScanContext<'_>) -> Option<ScanNode> {
  let metadata = std::fs::symlink_metadata(path).ok()?;
  let own_size = unique_allocated_size(path, &metadata, context.seen_inodes)?;
  context.progress.record(path, own_size);
  let is_dir = metadata.is_dir();

  if !is_dir {
    return Some(ScanNode {
      name: display_name(path, context.options.full_path),
      path: path.to_path_buf(),
      size: own_size,
      children: vec![],
      depth: state.depth,
      ignored: state.ignored,
      collapsed: state.collapsed,
    });
  }

  let current_stack = if context.options.respect_gitignore && !state.collapsed {
    append_gitignore(path, state.ignore_stack)
  } else {
    state.ignore_stack.to_vec()
  };

  if state.collapsed {
    return Some(ScanNode {
      name: display_name(path, context.options.full_path),
      path: path.to_path_buf(),
      size: own_size + summarize_dir_children(path, context.seen_inodes, context.progress),
      children: vec![],
      depth: state.depth,
      ignored: state.ignored,
      collapsed: state.collapsed,
    });
  }

  let children = match std::fs::read_dir(path) {
    Ok(entries) => entries
      .par_bridge()
      .filter_map(|entry| {
        let entry = entry.ok()?;
        let entry_path = entry.path();
        let file_type = entry.file_type().ok()?;
        let is_entry_dir = file_type.is_dir();

        if context.options.ignore_hidden && is_hidden(&entry_path) {
          return None;
        }

        let is_ignored = context.options.respect_gitignore
          && is_gitignored(&entry_path, is_entry_dir, &current_stack);
        if is_ignored && context.options.ignored_mode == IgnoredMode::Exclude {
          return None;
        }

        let collapse_child =
          is_ignored && context.options.ignored_mode == IgnoredMode::Summarize && is_entry_dir;

        scan_path(
          &entry_path,
          ScanPathState {
            depth: if is_entry_dir {
              state.depth + 1
            } else {
              state.depth
            },
            ignore_stack: &current_stack,
            ignored: is_ignored,
            collapsed: collapse_child,
          },
          context,
        )
      })
      .collect::<Vec<_>>(),
    Err(_) => vec![],
  };

  let children_size = children.iter().map(|child| child.size).sum::<u64>();

  Some(ScanNode {
    name: display_name(path, context.options.full_path),
    path: path.to_path_buf(),
    size: own_size + children_size,
    children,
    depth: state.depth,
    ignored: state.ignored,
    collapsed: state.collapsed,
  })
}

fn summarize_path(path: &Path, seen_inodes: &SeenInodes) -> u64 {
  summarize_path_with_progress(path, seen_inodes, &ProgressState::default())
}

fn summarize_path_with_progress(
  path: &Path,
  seen_inodes: &SeenInodes,
  progress: &ProgressState,
) -> u64 {
  let metadata = match std::fs::symlink_metadata(path) {
    Ok(metadata) => metadata,
    Err(_) => return 0,
  };
  let own_size = match unique_allocated_size(path, &metadata, seen_inodes) {
    Some(size) => size,
    None => return 0,
  };
  progress.record(path, own_size);

  if metadata.is_dir() {
    own_size + summarize_dir_children(path, seen_inodes, progress)
  } else {
    own_size
  }
}

fn append_gitignore(path: &Path, ignore_stack: &[Arc<Gitignore>]) -> IgnoreStack {
  let gitignore_path = path.join(".gitignore");
  if !gitignore_path.exists() {
    return ignore_stack.to_vec();
  }

  let mut builder = GitignoreBuilder::new(path);
  let _ = builder.add(&gitignore_path);

  match builder.build() {
    Ok(gitignore) => {
      let mut next = ignore_stack.to_vec();
      next.push(Arc::new(gitignore));
      next
    }
    Err(_) => ignore_stack.to_vec(),
  }
}

fn is_gitignored(path: &Path, is_dir: bool, ignore_stack: &[Arc<Gitignore>]) -> bool {
  let mut ignored = false;

  for gitignore in ignore_stack {
    let matched = gitignore.matched(path, is_dir);
    if matched.is_ignore() {
      ignored = true;
    } else if matched.is_whitelist() {
      ignored = false;
    }
  }

  ignored
}

fn summarize_dir_children(path: &Path, seen_inodes: &SeenInodes, progress: &ProgressState) -> u64 {
  match std::fs::read_dir(path) {
    Ok(entries) => entries
      .par_bridge()
      .filter_map(|entry| {
        let entry = entry.ok()?;
        let entry_path = entry.path();
        let metadata = std::fs::symlink_metadata(&entry_path).ok()?;
        let own_size = unique_allocated_size(&entry_path, &metadata, seen_inodes)?;
        progress.record(&entry_path, own_size);

        if metadata.is_dir() {
          Some(own_size + summarize_dir_children(&entry_path, seen_inodes, progress))
        } else {
          Some(own_size)
        }
      })
      .sum(),
    Err(_) => 0,
  }
}

fn display_name(path: &Path, full_path: bool) -> String {
  if full_path {
    return path.to_string_lossy().to_string();
  }

  path
    .file_name()
    .map(|name| name.to_string_lossy().to_string())
    .unwrap_or_else(|| path.to_string_lossy().to_string())
}

fn is_hidden(path: &Path) -> bool {
  path
    .file_name()
    .and_then(|name| name.to_str())
    .is_some_and(|name| name.starts_with('.'))
}

fn unique_allocated_size(
  path: &Path,
  metadata: &std::fs::Metadata,
  seen_inodes: &SeenInodes,
) -> Option<u64> {
  if let Some(key) = inode_key(path, metadata) {
    let mut seen = seen_inodes.lock().ok()?;
    if !seen.insert(key) {
      return None;
    }
  }

  Some(allocated_size(metadata))
}

#[cfg(unix)]
fn inode_key(_path: &Path, metadata: &std::fs::Metadata) -> Option<(u64, u64)> {
  use std::os::unix::fs::MetadataExt;

  Some((metadata.ino(), metadata.dev()))
}

#[cfg(windows)]
fn inode_key(path: &Path, metadata: &std::fs::Metadata) -> Option<(u64, u64)> {
  use std::mem::MaybeUninit;
  use std::os::windows::io::AsRawHandle;
  use windows_sys::Win32::Foundation::HANDLE;
  use windows_sys::Win32::Storage::FileSystem::{
    GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
  };

  if metadata.is_dir() {
    return None;
  }

  let file = std::fs::File::open(path).ok()?;
  let mut info = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
  let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, info.as_mut_ptr()) };
  if ok == 0 {
    return None;
  }

  let info = unsafe { info.assume_init() };
  let file_index = ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64;
  Some((file_index, info.dwVolumeSerialNumber as u64))
}

#[cfg(not(any(unix, windows)))]
fn inode_key(_path: &Path, _metadata: &std::fs::Metadata) -> Option<(u64, u64)> {
  None
}

#[cfg(unix)]
fn allocated_size(metadata: &std::fs::Metadata) -> u64 {
  use std::os::unix::fs::MetadataExt;

  metadata.blocks().saturating_mul(512)
}

#[cfg(not(unix))]
fn allocated_size(metadata: &std::fs::Metadata) -> u64 {
  metadata.len()
}

#[cfg(test)]
mod tests {
  use super::{scan_directory_with_progress, IgnoredMode, ScanOptions};
  use std::fs::{create_dir_all, remove_dir_all, write};
  use std::path::PathBuf;
  use std::sync::{Arc, Mutex};
  use std::time::{SystemTime, UNIX_EPOCH};

  #[test]
  fn scan_directory_reports_cumulative_progress() {
    let root = test_dir("progress");
    create_dir_all(root.join("nested")).unwrap();
    write(root.join("alpha.txt"), b"alpha").unwrap();
    write(root.join("nested").join("beta.txt"), b"beta").unwrap();

    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&events);
    let trees = scan_directory_with_progress(
      ScanOptions {
        directories: vec![root.clone()],
        ignore_hidden: false,
        full_path: false,
        respect_gitignore: false,
        ignored_mode: IgnoredMode::Summarize,
      },
      move |event| captured.lock().unwrap().push(event),
    );

    let _ = remove_dir_all(&root);

    let events = events.lock().unwrap();
    assert!(!events.is_empty());
    assert!(events.iter().any(|event| event.path.ends_with("alpha.txt")));
    assert!(events.iter().any(|event| event.path.ends_with("beta.txt")));
    assert_eq!(events.last().unwrap().bytes_scanned, trees[0].size);
    assert_eq!(events.last().unwrap().entries_scanned, events.len() as u64);
  }

  fn test_dir(label: &str) -> PathBuf {
    let suffix = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap()
      .as_nanos();
    std::env::temp_dir().join(format!("space-lens-{label}-{suffix}"))
  }
}
