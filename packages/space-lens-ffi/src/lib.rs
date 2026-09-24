use std::ffi::{c_char, c_void, CStr, CString};
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use space_lens::cloud::{
  build_eviction_plan, execute_eviction_plan_with_state, EvictionPlan, EvictionProgressState,
  NativeICloudBackend,
};

pub const SPACE_LENS_FFI_ABI_VERSION: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SlAbiInfo {
  pub struct_size: u32,
  pub abi_version: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SlProgressEvent {
  pub struct_size: u32,
  pub abi_version: u32,
  pub phase: u32,
  pub processed: u64,
  pub total: u64,
  pub logical_bytes: u64,
  pub allocated_bytes: u64,
  pub user_data: *mut c_void,
}

#[repr(C)]
pub struct SlRuntime {
  _private: [u8; 0],
}

#[repr(C)]
pub struct SlScanSnapshot {
  _private: [u8; 0],
}

pub struct SlICloudPlan {
  plan: EvictionPlan,
  progress: Arc<EvictionProgressState>,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SlICloudProgress {
  pub struct_size: u32,
  pub abi_version: u32,
  pub processed: u64,
  pub total: u64,
  pub evicted: u64,
  pub failed: u64,
  pub freed_bytes: u64,
  pub active: u64,
  pub running: u8,
  pub paused: u8,
  pub cancelled: u8,
  pub reserved: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SlOwnedBuffer {
  pub ptr: *mut u8,
  pub len: usize,
}

#[no_mangle]
pub extern "C" fn sl_abi_version() -> u32 {
  SPACE_LENS_FFI_ABI_VERSION
}

#[no_mangle]
pub extern "C" fn sl_abi_info() -> SlAbiInfo {
  SlAbiInfo {
    struct_size: std::mem::size_of::<SlAbiInfo>() as u32,
    abi_version: SPACE_LENS_FFI_ABI_VERSION,
  }
}

#[no_mangle]
pub extern "C" fn sl_capability_count() -> u32 {
  let _ = space_lens::PlatformCapabilities::for_current_platform();
  4
}

#[no_mangle]
/// # Safety
///
/// `path` must be null or point to a valid NUL-terminated UTF-8 C string for
/// the duration of the call. The returned buffer must be released exactly
/// once with `sl_free_buffer`.
pub unsafe extern "C" fn sl_icloud_plan_json(path: *const c_char) -> SlOwnedBuffer {
  if path.is_null() {
    return SlOwnedBuffer {
      ptr: std::ptr::null_mut(),
      len: 0,
    };
  }

  let path = match CStr::from_ptr(path).to_str() {
    Ok(path) => PathBuf::from(path),
    Err(_) => {
      return SlOwnedBuffer {
        ptr: std::ptr::null_mut(),
        len: 0,
      }
    }
  };
  match build_cloud_plan(path) {
    Some(plan) => owned_json(&plan),
    None => empty_buffer(),
  }
}

#[no_mangle]
/// # Safety
///
/// `path` must be null or point to a valid NUL-terminated UTF-8 C string for
/// the duration of the call. The returned plan must be released exactly once
/// with `sl_icloud_plan_free`.
pub unsafe extern "C" fn sl_icloud_plan_create(path: *const c_char) -> *mut SlICloudPlan {
  if path.is_null() {
    return std::ptr::null_mut();
  }
  let path = match CStr::from_ptr(path).to_str() {
    Ok(path) => PathBuf::from(path),
    Err(_) => return std::ptr::null_mut(),
  };
  let Some(plan) = build_cloud_plan(path) else {
    return std::ptr::null_mut();
  };
  Box::into_raw(Box::new(SlICloudPlan {
    plan,
    progress: Arc::new(EvictionProgressState::new()),
  }))
}

#[no_mangle]
/// # Safety
///
/// `plan` must be a live pointer returned by `sl_icloud_plan_create` and must
/// not be freed until this call returns. The returned buffer must be released
/// exactly once with `sl_free_buffer`.
pub unsafe extern "C" fn sl_icloud_plan_handle_json(plan: *const SlICloudPlan) -> SlOwnedBuffer {
  let Some(plan) = plan.as_ref() else {
    return empty_buffer();
  };
  owned_json(&plan.plan)
}

#[no_mangle]
/// # Safety
///
/// `plan` must be a live pointer returned by `sl_icloud_plan_create`. The
/// caller must not free it or start another execution until this call returns.
/// The returned buffer must be released exactly once with `sl_free_buffer`.
pub unsafe extern "C" fn sl_icloud_plan_execute(
  plan: *const SlICloudPlan,
  max_concurrency: u32,
) -> SlOwnedBuffer {
  let Some(plan) = plan.as_ref() else {
    return empty_buffer();
  };
  let outcome = match execute_eviction_plan_with_state(
    &NativeICloudBackend,
    &plan.plan,
    max_concurrency as usize,
    &plan.progress,
  ) {
    Ok(outcome) => outcome,
    Err(_) => return empty_buffer(),
  };
  owned_json(&outcome)
}

#[no_mangle]
/// # Safety
///
/// `plan` must be a live pointer returned by `sl_icloud_plan_create`.
pub unsafe extern "C" fn sl_icloud_progress(plan: *const SlICloudPlan) -> SlICloudProgress {
  let Some(plan) = plan.as_ref() else {
    return SlICloudProgress {
      struct_size: std::mem::size_of::<SlICloudProgress>() as u32,
      abi_version: SPACE_LENS_FFI_ABI_VERSION,
      processed: 0,
      total: 0,
      evicted: 0,
      failed: 0,
      freed_bytes: 0,
      active: 0,
      running: 0,
      paused: 0,
      cancelled: 0,
      reserved: 0,
    };
  };
  let progress = plan.progress.snapshot();
  SlICloudProgress {
    struct_size: std::mem::size_of::<SlICloudProgress>() as u32,
    abi_version: SPACE_LENS_FFI_ABI_VERSION,
    processed: progress.processed as u64,
    total: progress.total as u64,
    evicted: progress.evicted as u64,
    failed: progress.failed as u64,
    freed_bytes: progress.freed_bytes,
    active: progress.active as u64,
    running: progress.running as u8,
    paused: progress.paused as u8,
    cancelled: progress.cancelled as u8,
    reserved: 0,
  }
}

#[no_mangle]
/// # Safety
///
/// `plan` must be a live pointer returned by `sl_icloud_plan_create`.
pub unsafe extern "C" fn sl_icloud_pause(plan: *const SlICloudPlan) {
  if let Some(plan) = plan.as_ref() {
    plan.progress.pause();
  }
}

#[no_mangle]
/// # Safety
///
/// `plan` must be a live pointer returned by `sl_icloud_plan_create`.
pub unsafe extern "C" fn sl_icloud_resume(plan: *const SlICloudPlan) {
  if let Some(plan) = plan.as_ref() {
    plan.progress.resume();
  }
}

#[no_mangle]
/// # Safety
///
/// `plan` must be a live pointer returned by `sl_icloud_plan_create`.
pub unsafe extern "C" fn sl_icloud_cancel(plan: *const SlICloudPlan) {
  if let Some(plan) = plan.as_ref() {
    plan.progress.cancel();
  }
}

#[no_mangle]
/// # Safety
///
/// `plan` must be a live pointer returned by `sl_icloud_plan_create` and must
/// not have an execution in progress.
pub unsafe extern "C" fn sl_icloud_plan_free(plan: *mut SlICloudPlan) {
  if !plan.is_null() {
    drop(Box::from_raw(plan));
  }
}

fn build_cloud_plan(path: PathBuf) -> Option<EvictionPlan> {
  build_eviction_plan(
    &NativeICloudBackend,
    &path,
    space_lens::cloud::ScanOptions::default(),
  )
  .ok()
}

fn empty_buffer() -> SlOwnedBuffer {
  SlOwnedBuffer {
    ptr: std::ptr::null_mut(),
    len: 0,
  }
}

fn owned_json<T: Serialize>(value: &T) -> SlOwnedBuffer {
  let Ok(json) = serde_json::to_vec(value) else {
    return empty_buffer();
  };
  let Ok(json) = CString::new(json) else {
    return empty_buffer();
  };
  let bytes = json.into_bytes_with_nul();
  let len = bytes.len() - 1;
  let ptr = Box::into_raw(bytes.into_boxed_slice()) as *mut u8;
  SlOwnedBuffer { ptr, len }
}

#[no_mangle]
/// # Safety
///
/// `path` must be null or point to a valid NUL-terminated UTF-8 C string for
/// the duration of the call. The returned buffer must be released exactly
/// once with `sl_free_buffer`.
pub unsafe extern "C" fn sl_scan_directory_json(path: *const c_char) -> SlOwnedBuffer {
  if path.is_null() {
    return SlOwnedBuffer {
      ptr: std::ptr::null_mut(),
      len: 0,
    };
  }

  let path = match CStr::from_ptr(path).to_str() {
    Ok(path) => PathBuf::from(path),
    Err(_) => {
      return SlOwnedBuffer {
        ptr: std::ptr::null_mut(),
        len: 0,
      }
    }
  };
  let tree = space_lens::scan_directory(space_lens::ScanOptions {
    directories: vec![path],
    ignore_hidden: false,
    full_path: false,
    respect_gitignore: true,
    ignored_mode: space_lens::IgnoredMode::Summarize,
  });
  let snapshot = space_lens::SnapshotEnvelope::from_scan_nodes(&tree, env!("CARGO_PKG_VERSION"));
  let json = match serde_json::to_vec(&snapshot) {
    Ok(json) => json,
    Err(_) => {
      return SlOwnedBuffer {
        ptr: std::ptr::null_mut(),
        len: 0,
      }
    }
  };
  let json = match CString::new(json) {
    Ok(json) => json,
    Err(_) => {
      return SlOwnedBuffer {
        ptr: std::ptr::null_mut(),
        len: 0,
      }
    }
  };
  let bytes = json.into_bytes_with_nul();
  let len = bytes.len() - 1;
  let ptr = Box::into_raw(bytes.into_boxed_slice()) as *mut u8;
  SlOwnedBuffer { ptr, len }
}

#[no_mangle]
/// # Safety
///
/// `buffer` must be a value returned by `sl_scan_directory_json` that has not
/// already been released. Its pointer and length must not be modified.
pub unsafe extern "C" fn sl_free_buffer(buffer: SlOwnedBuffer) {
  if buffer.ptr.is_null() {
    return;
  }
  let slice = std::ptr::slice_from_raw_parts_mut(buffer.ptr, buffer.len + 1);
  drop(Box::from_raw(slice));
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn exposes_stable_abi_header_shape() {
    assert_eq!(sl_abi_version(), 1);
    let info = sl_abi_info();
    assert_eq!(info.abi_version, 1);
    assert_eq!(info.struct_size as usize, std::mem::size_of::<SlAbiInfo>());
  }

  #[test]
  fn exposes_icloud_progress_shape_without_a_live_plan() {
    let progress = unsafe { sl_icloud_progress(std::ptr::null()) };
    assert_eq!(
      progress.struct_size as usize,
      std::mem::size_of::<SlICloudProgress>()
    );
    assert_eq!(progress.abi_version, SPACE_LENS_FFI_ABI_VERSION);
    assert_eq!(progress.total, 0);
    assert_eq!(progress.running, 0);
  }
}
