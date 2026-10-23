//! Raw, platform-specific sampling output (§4.2 of
//! docs/memory-cleaner-plan.md). Nothing here is on the wire; grouping and
//! safety turn these values into the frozen model in [`crate::model`], so the
//! field set is deliberately richer than the wire types.

use crate::model::MemoryPressure;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// One process as read straight from the operating system.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawProcess {
  pub pid: u32,
  pub ppid: Option<u32>,
  pub start_time_us: u64,
  pub uid: Option<u32>,
  pub name: String,
  pub exe: Option<PathBuf>,
  pub cpu_percent: Option<f32>,
  pub footprint_bytes: Option<u64>,
  pub resident_bytes: Option<u64>,
  pub virtual_bytes: Option<u64>,
  pub zombie: bool,
}

/// Page counters used to derive the macOS memory breakdown.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VmPages {
  pub page_size: u64,
  pub free: u64,
  pub active: u64,
  pub inactive: u64,
  pub speculative: u64,
  pub wired: u64,
  pub purgeable: u64,
  pub internal: u64,
  pub external: u64,
  pub compressor: u64,
}

/// The system-wide part of one sample.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawSystem {
  pub total_bytes: u64,
  pub used_bytes: u64,
  pub free_bytes: u64,
  pub available_bytes: Option<u64>,
  pub swap_total_bytes: u64,
  pub swap_used_bytes: u64,
  pub vm: Option<VmPages>,
  pub pressure: MemoryPressure,
}

/// The result of one sampler tick.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawSample {
  pub system: RawSystem,
  pub processes: Vec<RawProcess>,
}

/// What the application directory knows about one running application.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
  pub pid: u32,
  pub bundle_id: Option<String>,
  pub name: Option<String>,
  /// Whether the application shows in the Dock (activation policy `regular`).
  pub regular: bool,
}

/// A POSIX signal the signaller can deliver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
  Term,
  Kill,
}

/// Whether an application quit request was accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuitAck {
  /// The quit request was delivered to a running application.
  Sent,
  /// The pid is not a running application.
  NotAnApp,
}
