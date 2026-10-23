//! The macOS memory sampler (T8 of docs/memory-cleaner-plan.md): the portable,
//! sysinfo-based sample enriched with the readings only macOS offers — the
//! per-process physical footprint (`ri_phys_footprint`, which is what Activity
//! Monitor calls "Memory"), a microsecond-exact process start time, the
//! virtual-memory page counters that become the memory breakdown, and the
//! kernel's memory-pressure level.
//!
//! Everything this module cannot read for lack of permission stays `None`: a
//! process with no footprint is marked restricted by the grouping step (§4.2),
//! and a failed page or pressure read is reported as unknown rather than
//! guessed. The `unsafe` calls below are the only ones in the crate — `lib.rs`
//! denies `unsafe_code` crate-wide and this `allow` narrows it to this file,
//! where the work is passing correctly sized `#[repr(C)]` structs by pointer to
//! libproc, mach and sysctl.

#![allow(unsafe_code)]

use crate::error::MemoryError;
use crate::model::{MemoryBreakdown, MemoryPressure, Platform, PressureLevel, PressureSource};
use crate::platform::portable::PortableSampler;
use crate::raw::{RawSample, VmPages};
use crate::sampler::ProcessSampler;
use std::collections::{HashMap, HashSet};

/// `sysctl` name for the virtual-memory page size in bytes.
const HW_PAGESIZE: &[u8] = b"hw.pagesize\0";

/// `sysctl` name for the kernel's coarse memory-pressure level.
const KERN_PRESSURE_LEVEL: &[u8] = b"kern.memorystatus_vm_pressure_level\0";

/// `sysctl` name for the kernel's share of memory still available, in percent.
const KERN_AVAILABLE_PERCENT: &[u8] = b"kern.memorystatus_level\0";

/// A [`ProcessSampler`] for macOS: [`PortableSampler`] plus per-process
/// footprint and exact start time, VM page counters and kernel pressure.
#[derive(Debug)]
pub struct MacSampler {
  portable: PortableSampler,
  /// Exact start times in microseconds, keyed by `(pid, sysinfo start second)`,
  /// so a pid reused after its process exited is re-read instead of serving a
  /// stale identity. Pruned to the live keys on every tick.
  starts: HashMap<(u32, u64), u64>,
}

impl MacSampler {
  /// A sampler that has not taken a sample yet.
  pub fn new() -> Self {
    MacSampler {
      portable: PortableSampler::new(),
      starts: HashMap::new(),
    }
  }
}

impl Default for MacSampler {
  fn default() -> Self {
    MacSampler::new()
  }
}

impl ProcessSampler for MacSampler {
  fn platform(&self) -> Platform {
    Platform::Macos
  }

  fn sample(&mut self) -> Result<RawSample, MemoryError> {
    let mut sample = self.portable.sample()?;

    let mut live: HashSet<(u32, u64)> = HashSet::with_capacity(sample.processes.len());
    for process in &mut sample.processes {
      let key = (process.pid, process.start_time_us / 1_000_000);
      live.insert(key);
      if let Some(&exact) = self.starts.get(&key) {
        process.start_time_us = exact;
      } else if let Some(exact) = start_time_us(process.pid) {
        self.starts.insert(key, exact);
        process.start_time_us = exact;
      }
      process.footprint_bytes = footprint_bytes(process.pid);
    }
    self.starts.retain(|key, _| live.contains(key));

    sample.system.vm = read_vm_pages();
    sample.system.pressure = read_pressure();
    Ok(sample)
  }
}

/// The physical footprint of `pid` in bytes (`ri_phys_footprint`), or `None`
/// when the kernel refuses the read (another user, the kernel, already gone).
fn footprint_bytes(pid: u32) -> Option<u64> {
  let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
  let result = unsafe {
    libc::proc_pid_rusage(
      pid as libc::c_int,
      libc::RUSAGE_INFO_V2,
      &mut info as *mut libc::rusage_info_v2 as *mut libc::rusage_info_t,
    )
  };
  (result == 0).then_some(info.ri_phys_footprint)
}

/// The exact start time of `pid` in microseconds since the Unix epoch, from
/// `proc_pidinfo(PROC_PIDTBSDINFO)`, or `None` when it is not readable.
fn start_time_us(pid: u32) -> Option<u64> {
  let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
  let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
  let result = unsafe {
    libc::proc_pidinfo(
      pid as libc::c_int,
      libc::PROC_PIDTBSDINFO,
      0,
      &mut info as *mut libc::proc_bsdinfo as *mut libc::c_void,
      size,
    )
  };
  (result == size).then(|| info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
}

/// The current VM page counters, or `None` when the kernel call fails.
fn read_vm_pages() -> Option<VmPages> {
  let page_size = sysctl_u64(HW_PAGESIZE)?;
  if page_size == 0 {
    return None;
  }

  let mut stats: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
  let mut count = libc::HOST_VM_INFO64_COUNT;
  let result = unsafe {
    libc::host_statistics64(
      mach2::mach_init::mach_host_self(),
      libc::HOST_VM_INFO64,
      &mut stats as *mut libc::vm_statistics64 as libc::host_info64_t,
      &mut count,
    )
  };
  if result != libc::KERN_SUCCESS {
    return None;
  }

  Some(VmPages {
    page_size,
    free: u64::from(stats.free_count),
    active: u64::from(stats.active_count),
    inactive: u64::from(stats.inactive_count),
    speculative: u64::from(stats.speculative_count),
    wired: u64::from(stats.wire_count),
    purgeable: u64::from(stats.purgeable_count),
    internal: u64::from(stats.internal_page_count),
    external: u64::from(stats.external_page_count),
    compressor: u64::from(stats.compressor_page_count),
  })
}

/// The kernel's memory-pressure reading (§4.2): the level is `1` normal, `2`
/// warning, `4` critical and anything else unknown, and the source is always
/// [`PressureSource::Kernel`]. The share of memory still available comes from
/// `kern.memorystatus_level` and is `None` when that read fails.
fn read_pressure() -> MemoryPressure {
  let level = match sysctl_i32(KERN_PRESSURE_LEVEL) {
    Some(1) => PressureLevel::Normal,
    Some(2) => PressureLevel::Warning,
    Some(4) => PressureLevel::Critical,
    _ => PressureLevel::Unknown,
  };
  MemoryPressure {
    level,
    source: PressureSource::Kernel,
    available_percent: sysctl_i32(KERN_AVAILABLE_PERCENT).map(|percent| percent as f32),
  }
}

/// Read a 64-bit `sysctl` by name; `None` when the name is unknown or fails.
///
/// The buffer is a `u64` because `hw.pagesize` is a `u64`, but a 32-bit
/// `sysctl` read into it is fine: the buffer starts zeroed, the kernel writes
/// only its own width and reports it back through `size`.
fn sysctl_u64(name: &[u8]) -> Option<u64> {
  let mut value: u64 = 0;
  let mut size = std::mem::size_of::<u64>();
  let result = unsafe {
    libc::sysctlbyname(
      name.as_ptr() as *const libc::c_char,
      &mut value as *mut u64 as *mut libc::c_void,
      &mut size,
      std::ptr::null_mut(),
      0,
    )
  };
  (result == 0).then_some(value)
}

/// Read a 32-bit `sysctl` by name; `None` when the name is unknown or fails.
fn sysctl_i32(name: &[u8]) -> Option<i32> {
  let mut value: i32 = 0;
  let mut size = std::mem::size_of::<i32>();
  let result = unsafe {
    libc::sysctlbyname(
      name.as_ptr() as *const libc::c_char,
      &mut value as *mut i32 as *mut libc::c_void,
      &mut size,
      std::ptr::null_mut(),
      0,
    )
  };
  (result == 0).then_some(value)
}

/// Map the raw page counters onto the wire breakdown (§2 and T8 of
/// docs/memory-cleaner-plan.md).
///
/// App is anonymous memory minus what the kernel may reclaim (purgeable),
/// cached files are file-backed memory plus that same purgeable part, wired and
/// compressed are the page counts the kernel names, and free is free minus the
/// speculative pages the kernel keeps ready but has not handed out. `used_bytes`
/// is what Activity Monitor calls "Memory Used": app plus wired plus compressed.
/// Every subtraction saturates, so no counter can wrap. `swap` is
/// `(total_bytes, used_bytes)`.
pub fn breakdown_from_pages(
  pages: &VmPages,
  total_bytes: u64,
  swap: (u64, u64),
) -> MemoryBreakdown {
  let page_size = pages.page_size;
  let app_bytes = pages
    .internal
    .saturating_sub(pages.purgeable)
    .saturating_mul(page_size);
  let cached_files_bytes = pages
    .external
    .saturating_add(pages.purgeable)
    .saturating_mul(page_size);
  let wired_bytes = pages.wired.saturating_mul(page_size);
  let compressed_bytes = pages.compressor.saturating_mul(page_size);
  let free_bytes = pages
    .free
    .saturating_sub(pages.speculative)
    .saturating_mul(page_size);
  let used_bytes = app_bytes
    .saturating_add(wired_bytes)
    .saturating_add(compressed_bytes)
    .min(total_bytes);

  MemoryBreakdown {
    total_bytes,
    used_bytes,
    free_bytes,
    available_bytes: Some(total_bytes.saturating_sub(used_bytes)),
    app_bytes: Some(app_bytes),
    wired_bytes: Some(wired_bytes),
    compressed_bytes: Some(compressed_bytes),
    cached_files_bytes: Some(cached_files_bytes),
    swap_total_bytes: swap.0,
    swap_used_bytes: swap.1,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Page size of the reference machine (§ appendix).
  const PAGE: u64 = 16_384;

  /// `hw.memsize` of the reference machine (§ appendix).
  const TOTAL: u64 = 51_539_607_552;

  /// The counters `vm_stat` printed on the reference machine, page size 16384.
  fn appendix_pages() -> VmPages {
    VmPages {
      page_size: PAGE,
      free: 164_414,
      active: 949_112,
      inactive: 1_198_694,
      speculative: 5_926,
      wired: 344_138,
      purgeable: 40_480,
      internal: 1_147_765,
      external: 1_005_967,
      compressor: 429_434,
    }
  }

  #[test]
  fn breakdown_maps_vm_stat_pages_to_bytes() {
    // `vm.swapusage` on the reference machine: 4096 MiB total, 3112.5 MiB used.
    let swap = (4_294_967_296, 3_263_232_000);
    let breakdown = breakdown_from_pages(&appendix_pages(), TOTAL, swap);

    assert_eq!(breakdown.app_bytes, Some((1_147_765 - 40_480) * PAGE));
    assert_eq!(
      breakdown.cached_files_bytes,
      Some((1_005_967 + 40_480) * PAGE)
    );
    assert_eq!(breakdown.wired_bytes, Some(344_138 * PAGE));
    assert_eq!(breakdown.compressed_bytes, Some(429_434 * PAGE));
    assert_eq!(breakdown.free_bytes, (164_414 - 5_926) * PAGE);
    assert_eq!(breakdown.swap_total_bytes, 4_294_967_296);
    assert_eq!(breakdown.swap_used_bytes, 3_263_232_000);
  }

  #[test]
  fn breakdown_parts_never_exceed_total_memory() {
    let breakdown = breakdown_from_pages(&appendix_pages(), TOTAL, (0, 0));

    let used = breakdown.used_bytes;
    let free = breakdown.free_bytes;
    let cached = breakdown
      .cached_files_bytes
      .expect("macOS reports cached files");
    assert_eq!(breakdown.total_bytes, TOTAL);
    assert!(
      used + free + cached <= TOTAL,
      "app + wired + compressed ({used}), free ({free}) and cached ({cached}) exceed {TOTAL}"
    );
    assert_eq!(breakdown.available_bytes, Some(TOTAL - used));
  }

  #[test]
  fn breakdown_saturates_instead_of_wrapping() {
    let pages = VmPages {
      page_size: PAGE,
      free: 10,
      active: 0,
      inactive: 0,
      speculative: 50,
      wired: 0,
      purgeable: 99,
      internal: 10,
      external: 0,
      compressor: 0,
    };
    let breakdown = breakdown_from_pages(&pages, 1_000, (0, 0));

    assert_eq!(
      breakdown.app_bytes,
      Some(0),
      "internal smaller than purgeable"
    );
    assert_eq!(breakdown.free_bytes, 0, "free smaller than speculative");
    assert_eq!(breakdown.used_bytes, 0);
    assert_eq!(breakdown.available_bytes, Some(1_000));
  }
}
