use super::guard::{absolute_path, ensure_within_scope};
use super::model::{
  Bytes, CloudError, DownloadState, ErrorKind, Fingerprint, ItemInfo, ItemKind, Result,
};
use super::platform::CloudBackend;
use super::policy::{eviction_skip_reason, SkipReason};
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const DEFAULT_MAX_ENTRIES: usize = 100_000;
const DEFAULT_MAX_DEPTH: usize = 128;
const DEFAULT_MAX_TIME: Duration = Duration::from_secs(120);

#[derive(Clone, Debug)]
pub struct ScanOptions {
  pub max_entries: usize,
  pub max_depth: usize,
  pub max_time: Duration,
}

impl Default for ScanOptions {
  fn default() -> Self {
    Self {
      max_entries: DEFAULT_MAX_ENTRIES,
      max_depth: DEFAULT_MAX_DEPTH,
      max_time: DEFAULT_MAX_TIME,
    }
  }
}

#[derive(Clone, Debug, Serialize)]
pub struct ItemSummary {
  pub path: PathBuf,
  pub logical_bytes: Bytes,
  pub allocated_bytes: Option<Bytes>,
  pub state: DownloadState,
  pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SkippedItem {
  pub path: PathBuf,
  pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct EvictionCandidate {
  pub path: PathBuf,
  pub logical_bytes: Bytes,
  pub allocated_bytes: Bytes,
  #[serde(skip)]
  pub(crate) fingerprint: Fingerprint,
}

#[derive(Clone, Debug, Serialize)]
pub struct EvictionPlan {
  pub root: PathBuf,
  pub candidates: Vec<EvictionCandidate>,
  pub cloud_only: Vec<ItemSummary>,
  pub skipped: Vec<SkippedItem>,
  pub coverage_complete: bool,
  pub visited_entries: usize,
  pub notes_total: usize,
}

impl EvictionPlan {
  pub fn total_allocated_bytes(&self) -> u64 {
    self
      .candidates
      .iter()
      .map(|candidate| candidate.allocated_bytes.0)
      .sum()
  }

  pub fn total_logical_bytes(&self) -> u64 {
    self
      .candidates
      .iter()
      .map(|candidate| candidate.logical_bytes.0)
      .sum()
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvictionStatus {
  Evicted,
  RequestedUnverified,
  Skipped,
  Failed,
}

#[derive(Clone, Debug, Serialize)]
pub struct EvictionResult {
  pub path: PathBuf,
  pub status: EvictionStatus,
  pub allocated_bytes_before: Bytes,
  pub error: Option<CloudError>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct EvictionOutcome {
  pub results: Vec<EvictionResult>,
}

pub fn inspect_item<B: CloudBackend + ?Sized>(backend: &B, path: &Path) -> Result<ItemInfo> {
  backend.inspect(path)
}

pub fn build_eviction_plan<B: CloudBackend + ?Sized>(
  backend: &B,
  selected_root: &Path,
  options: ScanOptions,
) -> Result<EvictionPlan> {
  if options.max_entries == 0 || options.max_depth == 0 || options.max_time.is_zero() {
    return Err(CloudError::new(
      ErrorKind::InvalidState,
      "scan limits must be positive",
    ));
  }

  let root = absolute_path(selected_root)?;
  let root_info = backend.inspect(&root)?;
  if !root_info.is_icloud {
    return Err(CloudError::new(
      ErrorKind::NotICloud,
      format!(
        "selected root is not recognized as an iCloud item: {}",
        root.display()
      ),
    ));
  }

  let started = Instant::now();
  let mut plan = EvictionPlan {
    root: root.clone(),
    candidates: Vec::new(),
    cloud_only: Vec::new(),
    skipped: Vec::new(),
    coverage_complete: true,
    visited_entries: 0,
    notes_total: 0,
  };
  let mut pending = vec![(root, 0_usize)];
  let mut seen_identities = HashSet::new();

  while let Some((path, depth)) = pending.pop() {
    if started.elapsed() >= options.max_time {
      plan.coverage_complete = false;
      add_note(&mut plan, &path, "scan time budget exceeded");
      break;
    }
    if plan.visited_entries >= options.max_entries || depth > options.max_depth {
      plan.coverage_complete = false;
      add_note(&mut plan, &path, "scan entry or depth budget exceeded");
      break;
    }

    ensure_within_scope(&plan.root, &path)?;
    plan.visited_entries += 1;

    let info = match backend.inspect(&path) {
      Ok(info) => info,
      Err(error) => {
        plan.coverage_complete = false;
        add_note(&mut plan, &path, error.to_string());
        continue;
      }
    };

    if info.kind() == ItemKind::Directory {
      match info.is_package {
        Some(true) => {
          plan.skipped.push(SkippedItem {
            path,
            reason: SkipReason::Package.to_string(),
          });
          continue;
        }
        None => {
          plan.skipped.push(SkippedItem {
            path,
            reason: SkipReason::UnknownPackageState.to_string(),
          });
          continue;
        }
        Some(false) => {}
      }

      let entries = match fs::read_dir(&path) {
        Ok(entries) => entries,
        Err(error) => {
          plan.coverage_complete = false;
          add_note(&mut plan, &path, error.to_string());
          continue;
        }
      };

      let mut children = Vec::new();
      for entry in entries {
        match entry {
          Ok(entry) => children.push(entry.path()),
          Err(error) => {
            plan.coverage_complete = false;
            add_note(&mut plan, &path, error.to_string());
          }
        }
      }
      children.sort();
      pending.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
      continue;
    }

    if let Some(reason) = eviction_skip_reason(&info) {
      if info.download_state == DownloadState::CloudOnly {
        plan.cloud_only.push(ItemSummary {
          path,
          logical_bytes: info.logical_bytes(),
          allocated_bytes: info.allocated_bytes,
          state: info.download_state,
          reason: Some(reason.to_string()),
        });
      } else {
        plan.skipped.push(SkippedItem {
          path,
          reason: reason.to_string(),
        });
      }
      continue;
    }

    let identity = (info.fingerprint.device, info.fingerprint.inode);
    if identity != (0, 0) && !seen_identities.insert(identity) {
      plan.skipped.push(SkippedItem {
        path,
        reason: SkipReason::HardLinked.to_string(),
      });
      continue;
    }

    let Some(allocated_bytes) = info.allocated_bytes else {
      plan.skipped.push(SkippedItem {
        path,
        reason: SkipReason::UnknownLocalAllocation.to_string(),
      });
      continue;
    };

    plan.candidates.push(EvictionCandidate {
      path,
      logical_bytes: info.logical_bytes(),
      allocated_bytes,
      fingerprint: info.fingerprint,
    });
  }

  Ok(plan)
}

pub fn execute_eviction_plan<B: CloudBackend + ?Sized>(
  backend: &B,
  plan: &EvictionPlan,
) -> Result<EvictionOutcome> {
  if !plan.coverage_complete {
    return Err(CloudError::new(
      ErrorKind::IncompletePlan,
      "cannot execute an incomplete eviction plan",
    ));
  }

  let mut outcome = EvictionOutcome::default();
  for candidate in &plan.candidates {
    let allocated_bytes_before = candidate.allocated_bytes;
    let result = match backend.inspect(&candidate.path) {
      Ok(current) if current.fingerprint == candidate.fingerprint => {
        if let Some(reason) = eviction_skip_reason(&current) {
          EvictionResult {
            path: candidate.path.clone(),
            status: EvictionStatus::Skipped,
            allocated_bytes_before,
            error: Some(CloudError::new(
              ErrorKind::InvalidState,
              format!("candidate is no longer eligible: {reason}"),
            )),
          }
        } else {
          match backend.evict_local_copy(&candidate.path, &candidate.fingerprint) {
            Ok(()) => match backend.inspect(&candidate.path) {
              Ok(after)
                if after.download_state == DownloadState::CloudOnly
                  && after.fingerprint == candidate.fingerprint =>
              {
                EvictionResult {
                  path: candidate.path.clone(),
                  status: EvictionStatus::Evicted,
                  allocated_bytes_before,
                  error: None,
                }
              }
              Ok(_) => EvictionResult {
                path: candidate.path.clone(),
                status: EvictionStatus::RequestedUnverified,
                allocated_bytes_before,
                error: None,
              },
              Err(error) => EvictionResult {
                path: candidate.path.clone(),
                status: EvictionStatus::RequestedUnverified,
                allocated_bytes_before,
                error: Some(error),
              },
            },
            Err(error) => EvictionResult {
              path: candidate.path.clone(),
              status: EvictionStatus::Failed,
              allocated_bytes_before,
              error: Some(error),
            },
          }
        }
      }
      Ok(_) => EvictionResult {
        path: candidate.path.clone(),
        status: EvictionStatus::Skipped,
        allocated_bytes_before,
        error: Some(CloudError::new(
          ErrorKind::ChangedSincePlan,
          "file changed since the eviction plan was created",
        )),
      },
      Err(error) => EvictionResult {
        path: candidate.path.clone(),
        status: EvictionStatus::Skipped,
        allocated_bytes_before,
        error: Some(error),
      },
    };
    outcome.results.push(result);
  }

  Ok(outcome)
}

pub fn request_download_one<B: CloudBackend + ?Sized>(backend: &B, path: &Path) -> Result<()> {
  let info = backend.inspect(path)?;
  if !info.is_icloud || info.kind() != ItemKind::RegularFile {
    return Err(CloudError::new(
      ErrorKind::InvalidState,
      "download requests require an iCloud regular file",
    ));
  }
  backend.request_download(path, &info.fingerprint)
}

fn add_note(plan: &mut EvictionPlan, path: &Path, message: impl Into<String>) {
  plan.notes_total += 1;
  plan.skipped.push(SkippedItem {
    path: path.to_path_buf(),
    reason: message.into(),
  });
}
