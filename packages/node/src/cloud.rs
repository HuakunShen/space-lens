use napi::{Error, Result, Status};
use napi_derive::napi;
use space_lens::cloud::{
  build_eviction_plan, execute_eviction_plan, inspect_item, request_download_one, DownloadState,
  EvictionOutcome, EvictionPlan, EvictionStatus, ItemInfo, ItemKind, NativeICloudBackend,
  ScanOptions,
};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{
  atomic::{AtomicU64, Ordering},
  Mutex,
};

pub(crate) fn decimal_bytes(value: u64) -> String {
  value.to_string()
}

#[napi(object)]
pub struct ICloudItem {
  pub path: String,
  pub kind: String,
  #[napi(js_name = "isICloud")]
  pub is_icloud: bool,
  #[napi(js_name = "downloadState")]
  pub download_state: String,
  #[napi(js_name = "logicalBytes")]
  pub logical_bytes: String,
  #[napi(js_name = "allocatedBytes")]
  pub allocated_bytes: Option<String>,
  #[napi(js_name = "foundationAllocatedBytes")]
  pub foundation_allocated_bytes: Option<String>,
  #[napi(js_name = "isUploaded")]
  pub is_uploaded: Option<bool>,
  #[napi(js_name = "isUploading")]
  pub is_uploading: Option<bool>,
  #[napi(js_name = "isDownloading")]
  pub is_downloading: Option<bool>,
  #[napi(js_name = "hasConflicts")]
  pub has_conflicts: Option<bool>,
}

#[napi(object)]
pub struct ICloudPlan {
  #[napi(js_name = "planId")]
  pub plan_id: String,
  pub root: String,
  #[napi(js_name = "candidateCount")]
  pub candidate_count: u32,
  #[napi(js_name = "allocatedBytes")]
  pub allocated_bytes: String,
  #[napi(js_name = "logicalBytes")]
  pub logical_bytes: String,
  #[napi(js_name = "cloudOnlyCount")]
  pub cloud_only_count: u32,
  #[napi(js_name = "skippedCount")]
  pub skipped_count: u32,
  #[napi(js_name = "coverageComplete")]
  pub coverage_complete: bool,
}

#[napi(object)]
pub struct ICloudEvictionResult {
  pub path: String,
  pub status: String,
  #[napi(js_name = "allocatedBytesBefore")]
  pub allocated_bytes_before: String,
  pub error: Option<String>,
}

#[napi(object)]
pub struct ICloudEvictionOutcome {
  pub results: Vec<ICloudEvictionResult>,
}

#[napi]
pub struct ICloudSession {
  backend: NativeICloudBackend,
  plans: Mutex<HashMap<u64, EvictionPlan>>,
  next_plan_id: AtomicU64,
}

#[napi]
impl ICloudSession {
  #[napi(constructor)]
  pub fn new() -> Self {
    Self {
      backend: NativeICloudBackend,
      plans: Mutex::new(HashMap::new()),
      next_plan_id: AtomicU64::new(1),
    }
  }

  #[napi(js_name = "inspectICloudItem")]
  pub fn inspect_icloud_item(&self, path: String) -> Result<ICloudItem> {
    let info = inspect_item(&self.backend, Path::new(&path)).map_err(to_napi_error)?;
    Ok(item_dto(path, &info))
  }

  #[napi(js_name = "planICloudEviction")]
  pub fn plan_icloud_eviction(&self, root: String) -> Result<ICloudPlan> {
    let plan = build_eviction_plan(&self.backend, Path::new(&root), ScanOptions::default())
      .map_err(to_napi_error)?;
    let plan_id = self.next_plan_id.fetch_add(1, Ordering::Relaxed);
    let summary = plan_dto(plan_id, &plan);
    self
      .plans
      .lock()
      .map_err(|_| Error::new(Status::GenericFailure, "iCloud plan store is poisoned"))?
      .insert(plan_id, plan);
    Ok(summary)
  }

  #[napi(js_name = "executeICloudEviction")]
  pub fn execute_icloud_eviction(&self, plan_id: String) -> Result<ICloudEvictionOutcome> {
    let plan_id = parse_plan_id(&plan_id)?;
    let plan = self
      .plans
      .lock()
      .map_err(|_| Error::new(Status::GenericFailure, "iCloud plan store is poisoned"))?
      .remove(&plan_id)
      .ok_or_else(|| {
        Error::new(
          Status::InvalidArg,
          "unknown or already consumed iCloud plan",
        )
      })?;

    let outcome = execute_eviction_plan(&self.backend, &plan).map_err(to_napi_error)?;
    Ok(outcome_dto(outcome))
  }

  #[napi(js_name = "requestICloudDownload")]
  pub fn request_icloud_download(&self, path: String) -> Result<()> {
    request_download_one(&self.backend, Path::new(&path)).map_err(to_napi_error)
  }
}

fn item_dto(path: String, info: &ItemInfo) -> ICloudItem {
  ICloudItem {
    path,
    kind: item_kind_name(info.kind()).to_string(),
    is_icloud: info.is_icloud,
    download_state: download_state_name(info.download_state).to_string(),
    logical_bytes: decimal_bytes(info.fingerprint.logical_bytes),
    allocated_bytes: info.allocated_bytes.map(|bytes| decimal_bytes(bytes.0)),
    foundation_allocated_bytes: info
      .foundation_allocated_bytes
      .map(|bytes| decimal_bytes(bytes.0)),
    is_uploaded: info.is_uploaded,
    is_uploading: info.is_uploading,
    is_downloading: info.is_downloading,
    has_conflicts: info.has_conflicts,
  }
}

fn plan_dto(plan_id: u64, plan: &EvictionPlan) -> ICloudPlan {
  ICloudPlan {
    plan_id: decimal_bytes(plan_id),
    root: plan.root.to_string_lossy().to_string(),
    candidate_count: saturating_count(plan.candidates.len()),
    allocated_bytes: decimal_bytes(plan.total_allocated_bytes()),
    logical_bytes: decimal_bytes(plan.total_logical_bytes()),
    cloud_only_count: saturating_count(plan.cloud_only.len()),
    skipped_count: saturating_count(plan.skipped.len()),
    coverage_complete: plan.coverage_complete,
  }
}

fn outcome_dto(outcome: EvictionOutcome) -> ICloudEvictionOutcome {
  ICloudEvictionOutcome {
    results: outcome
      .results
      .into_iter()
      .map(|result| ICloudEvictionResult {
        path: result.path.to_string_lossy().to_string(),
        status: eviction_status_name(result.status).to_string(),
        allocated_bytes_before: decimal_bytes(result.allocated_bytes_before.0),
        error: result
          .error
          .map(|error| format!("{:?}: {}", error.kind, error.message)),
      })
      .collect(),
  }
}

fn parse_plan_id(value: &str) -> Result<u64> {
  value
    .parse::<u64>()
    .map_err(|_| Error::new(Status::InvalidArg, "planId must be a decimal string"))
}

fn to_napi_error(error: space_lens::cloud::CloudError) -> Error {
  Error::from_reason(format!("{:?}: {}", error.kind, error.message))
}

fn saturating_count(value: usize) -> u32 {
  u32::try_from(value).unwrap_or(u32::MAX)
}

fn item_kind_name(kind: ItemKind) -> &'static str {
  match kind {
    ItemKind::RegularFile => "regular_file",
    ItemKind::Directory => "directory",
    ItemKind::SymbolicLink => "symbolic_link",
    ItemKind::Other => "other",
  }
}

fn download_state_name(state: DownloadState) -> &'static str {
  match state {
    DownloadState::CloudOnly => "cloud_only",
    DownloadState::LocalCurrent => "local_current",
    DownloadState::LocalStale => "local_stale",
    DownloadState::Unknown => "unknown",
  }
}

fn eviction_status_name(status: EvictionStatus) -> &'static str {
  match status {
    EvictionStatus::Evicted => "evicted",
    EvictionStatus::RequestedUnverified => "requested_unverified",
    EvictionStatus::Skipped => "skipped",
    EvictionStatus::Failed => "failed",
  }
}

#[cfg(test)]
mod tests {
  use super::decimal_bytes;

  #[test]
  fn cloud_byte_counts_are_decimal_strings() {
    assert_eq!(decimal_bytes(u64::MAX), "18446744073709551615");
  }
}
