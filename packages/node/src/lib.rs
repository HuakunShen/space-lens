#![deny(clippy::all)]

use napi::{Error, Result, Status};
use napi_derive::napi;
use std::path::PathBuf;

use space_lens::{
  build_removal_plan as build_core_removal_plan, execute_removal_plan as execute_core_removal_plan,
  find_candidates as find_core_candidates, scan_directory as scan_core_directory, CandidateOptions,
  CleanupCandidate as CoreCleanupCandidate, CleanupPreset, IgnoredMode,
  RemovalEntry as CoreRemovalEntry, RemovalOutcome as CoreRemovalOutcome,
  RemovalPlan as CoreRemovalPlan, ScanNode, ScanOptions,
};

#[napi(object)]
pub struct DirectoryScanOptions {
  pub directories: Vec<String>,
  #[napi(js_name = "ignoreHidden")]
  pub ignore_hidden: Option<bool>,
  #[napi(js_name = "fullPath")]
  pub full_path: Option<bool>,
  #[napi(js_name = "respectGitignore")]
  pub respect_gitignore: Option<bool>,
  #[napi(js_name = "ignoredMode")]
  pub ignored_mode: Option<String>,
}

#[napi(object)]
pub struct DirectoryNode {
  pub name: String,
  pub path: String,
  #[napi(js_name = "size")]
  pub size: i64,
  pub children: Vec<DirectoryNode>,
  pub depth: u32,
  pub ignored: bool,
  pub collapsed: bool,
}

impl From<ScanNode> for DirectoryNode {
  fn from(node: ScanNode) -> Self {
    DirectoryNode {
      name: node.name,
      path: node.path.to_string_lossy().to_string(),
      size: node.size as i64,
      children: node.children.into_iter().map(DirectoryNode::from).collect(),
      depth: node.depth,
      ignored: node.ignored,
      collapsed: node.collapsed,
    }
  }
}

#[napi(js_name = "scanDirectory")]
pub fn scan_directory(options: DirectoryScanOptions) -> Vec<DirectoryNode> {
  let ignored_mode = match options.ignored_mode.as_deref() {
    Some("exclude") => IgnoredMode::Exclude,
    _ => IgnoredMode::Summarize,
  };

  scan_core_directory(ScanOptions {
    directories: options.directories.into_iter().map(PathBuf::from).collect(),
    ignore_hidden: options.ignore_hidden.unwrap_or(false),
    full_path: options.full_path.unwrap_or(false),
    respect_gitignore: options.respect_gitignore.unwrap_or(true),
    ignored_mode,
  })
  .into_iter()
  .map(DirectoryNode::from)
  .collect()
}

#[napi(object)]
pub struct CleanupCandidateOptions {
  pub directories: Vec<String>,
  pub presets: Option<Vec<String>>,
  #[napi(js_name = "ignoreHidden")]
  pub ignore_hidden: Option<bool>,
}

#[napi(object)]
pub struct CleanupCandidate {
  pub path: String,
  #[napi(js_name = "size")]
  pub size: i64,
  pub reason: String,
  pub preset: String,
  pub ignored: bool,
}

#[napi(object)]
pub struct RemovalEntry {
  pub path: String,
  #[napi(js_name = "size")]
  pub size: i64,
  pub reason: String,
  pub preset: String,
}

#[napi(object)]
pub struct RemovalPlan {
  pub entries: Vec<RemovalEntry>,
  #[napi(js_name = "totalSize")]
  pub total_size: i64,
  pub errors: Vec<String>,
}

#[napi(object)]
pub struct RemovalOutcome {
  pub removed: Vec<RemovalEntry>,
  #[napi(js_name = "bytesRemoved")]
  pub bytes_removed: i64,
  pub errors: Vec<String>,
}

impl From<CoreCleanupCandidate> for CleanupCandidate {
  fn from(candidate: CoreCleanupCandidate) -> Self {
    CleanupCandidate {
      path: candidate.path.to_string_lossy().to_string(),
      size: size_to_i64(candidate.size),
      reason: candidate.reason,
      preset: preset_to_string(candidate.preset),
      ignored: candidate.ignored,
    }
  }
}

impl From<CoreRemovalPlan> for RemovalPlan {
  fn from(plan: CoreRemovalPlan) -> Self {
    RemovalPlan {
      entries: plan
        .entries
        .into_iter()
        .map(|entry| RemovalEntry {
          path: entry.path.to_string_lossy().to_string(),
          size: size_to_i64(entry.size),
          reason: entry.reason,
          preset: preset_to_string(entry.preset),
        })
        .collect(),
      total_size: size_to_i64(plan.total_size),
      errors: plan.errors,
    }
  }
}

impl From<CoreRemovalOutcome> for RemovalOutcome {
  fn from(outcome: CoreRemovalOutcome) -> Self {
    RemovalOutcome {
      removed: outcome
        .removed
        .into_iter()
        .map(|entry| RemovalEntry {
          path: entry.path.to_string_lossy().to_string(),
          size: size_to_i64(entry.size),
          reason: entry.reason,
          preset: preset_to_string(entry.preset),
        })
        .collect(),
      bytes_removed: size_to_i64(outcome.bytes_removed),
      errors: outcome.errors,
    }
  }
}

#[napi(js_name = "findCleanupCandidates")]
pub fn find_cleanup_candidates(options: CleanupCandidateOptions) -> Result<Vec<CleanupCandidate>> {
  let candidates = find_core_candidates(candidate_options(options)?);
  Ok(candidates.into_iter().map(CleanupCandidate::from).collect())
}

#[napi(js_name = "planCleanup")]
pub fn plan_cleanup(options: CleanupCandidateOptions) -> Result<RemovalPlan> {
  let candidates = find_core_candidates(candidate_options(options)?);
  Ok(RemovalPlan::from(build_core_removal_plan(candidates)))
}

#[napi(js_name = "executeCleanup")]
pub fn execute_cleanup(plan: RemovalPlan) -> Result<RemovalOutcome> {
  let plan = core_removal_plan(plan)?;
  Ok(RemovalOutcome::from(execute_core_removal_plan(&plan)))
}

fn candidate_options(options: CleanupCandidateOptions) -> Result<CandidateOptions> {
  let presets = options
    .presets
    .unwrap_or_default()
    .into_iter()
    .map(|preset| parse_preset(&preset))
    .collect::<Result<Vec<_>>>()?;

  Ok(CandidateOptions {
    roots: options.directories.into_iter().map(PathBuf::from).collect(),
    presets,
    ignore_hidden: options.ignore_hidden.unwrap_or(false),
  })
}

fn core_removal_plan(plan: RemovalPlan) -> Result<CoreRemovalPlan> {
  let entries = plan
    .entries
    .into_iter()
    .map(|entry| {
      Ok(CoreRemovalEntry {
        path: PathBuf::from(entry.path),
        size: size_to_u64(entry.size, "entry.size")?,
        reason: entry.reason,
        preset: parse_preset(&entry.preset)?,
      })
    })
    .collect::<Result<Vec<_>>>()?;

  Ok(CoreRemovalPlan {
    entries,
    total_size: size_to_u64(plan.total_size, "totalSize")?,
    errors: plan.errors,
  })
}

fn parse_preset(preset: &str) -> Result<CleanupPreset> {
  match preset {
    "node" => Ok(CleanupPreset::Node),
    "rust" => Ok(CleanupPreset::Rust),
    "gitignored" => Ok(CleanupPreset::Gitignored),
    _ => Err(Error::new(
      Status::InvalidArg,
      format!("Unknown cleanup preset \"{preset}\". Use node, rust, or gitignored."),
    )),
  }
}

fn preset_to_string(preset: CleanupPreset) -> String {
  match preset {
    CleanupPreset::Node => "node",
    CleanupPreset::Rust => "rust",
    CleanupPreset::Gitignored => "gitignored",
  }
  .to_string()
}

fn size_to_i64(size: u64) -> i64 {
  i64::try_from(size).unwrap_or(i64::MAX)
}

fn size_to_u64(size: i64, field: &str) -> Result<u64> {
  u64::try_from(size).map_err(|_| {
    Error::new(
      Status::InvalidArg,
      format!("{field} must be a non-negative safe integer."),
    )
  })
}
