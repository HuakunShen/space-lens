//! Read-only APFS accounting that reconciles a scan with the disk's own
//! numbers.
//!
//! A scan measures the files it can walk. macOS reports what the whole APFS
//! container holds: every volume (Data, the sealed System volume, VM swap,
//! Preboot, Recovery, ...) plus local snapshots and purgeable space. This
//! crate reads those facts so a shell can show where the difference goes,
//! the way "hidden space" does in other disk tools.
//!
//! Everything here is a display fact: nothing is written, mounted or
//! thinned, and every failure (no APFS, `diskutil` missing or slow,
//! unparseable output) yields `None` instead of an invented number. macOS
//! does not report how many bytes each snapshot holds, so snapshots are
//! counted, never sized.

#![deny(clippy::all)]

#[cfg(target_os = "macos")]
mod macos;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// One scan root and the allocated bytes the scan measured under it.
#[derive(Clone, Debug)]
pub struct ScannedRoot {
  pub path: PathBuf,
  pub bytes: u64,
}

/// The APFS container holding a scan, with the scan's share of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageBreakdown {
  /// BSD name of the container, e.g. `disk3`.
  pub container: String,
  pub capacity_bytes: u64,
  /// Bytes allocated in the container: every volume, snapshot and purgeable
  /// file. This is the "used" figure System Settings shows.
  pub in_use_bytes: u64,
  /// Space macOS can free on demand (local snapshots, cached iCloud files).
  /// `None` when Foundation does not report it.
  pub purgeable_bytes: Option<u64>,
  /// Bytes the scan measured under roots that live in this container.
  pub scanned_bytes: u64,
  /// Every volume in the container, largest first.
  pub volumes: Vec<ApfsVolumeUsage>,
  /// Local snapshots of the container's Data volume; `None` when they could
  /// not be listed.
  pub snapshots: Option<SnapshotSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApfsVolumeUsage {
  pub name: String,
  /// APFS role (`Data`, `System`, `VM`, `Preboot`, `Recovery`, `Update`, ...)
  /// or `None` for a volume without one.
  pub role: String,
  pub device: String,
  pub in_use_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotSummary {
  /// Device of the volume the snapshots belong to.
  pub volume: String,
  pub count: u32,
  pub time_machine_count: u32,
  /// Local time of the oldest Time Machine snapshot, `YYYY-MM-DDTHH:MM:SS`.
  /// Anything deleted since then can still hold space.
  pub oldest_time_machine: Option<String>,
}

/// APFS accounting for the container holding most of the scanned bytes.
///
/// Roots outside APFS are ignored; `None` when no root is on APFS or the
/// platform is not macOS.
pub fn storage_breakdown(roots: &[ScannedRoot]) -> Option<StorageBreakdown> {
  #[cfg(target_os = "macos")]
  {
    macos::storage_breakdown(roots)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = roots;
    None
  }
}
