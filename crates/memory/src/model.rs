//! The frozen wire types of the memory view (§4.1 of
//! docs/memory-cleaner-plan.md), mirrored by the Zod schemas in
//! `packages/contract/src/memory.ts`. Every field name is camelCase on the
//! wire, byte counts and microsecond timestamps are unsigned integers below
//! 2^53, and optionals serialize as `null`.

use crate::ids::{AuditId, GroupId, PlanId};
use serde::{Deserialize, Serialize};

/// The operating system the snapshot was taken on.
#[non_exhaustive]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Platform {
  Macos,
  Linux,
  Windows,
  Unsupported,
}

/// A coarse memory-pressure level.
#[non_exhaustive]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PressureLevel {
  Normal,
  Warning,
  Critical,
  Unknown,
}

/// Where a pressure level came from.
#[non_exhaustive]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PressureSource {
  Kernel,
  Derived,
  Unavailable,
}

/// The role of a process in its group.
#[non_exhaustive]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProcessKind {
  App,
  Helper,
  Agent,
  Cli,
  System,
  Other,
}

/// The scheduler state of a process at sample time.
///
/// The wire keeps six stable states: the operating systems disagree on the
/// finer points (macOS `SSTOP` vs Linux `T`/`t`, kernel threads, traced
/// processes), so anything outside these buckets is `Unknown` rather than a
/// guess.
#[non_exhaustive]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProcessState {
  Running,
  Sleeping,
  Idle,
  Stopped,
  Zombie,
  Unknown,
}

/// Why a process must not be terminated.
#[non_exhaustive]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Protection {
  Kernel,
  Init,
  SessionCritical,
  SystemComponent,
  SelfOrAncestor,
  SelfChild,
  OtherUser,
  Unidentified,
  Zombie,
}

/// A caveat attached to a snapshot.
#[non_exhaustive]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotNote {
  CpuWarmingUp,
  RestrictedProcesses,
  BreakdownUnavailable,
  PressureDerived,
  Truncated,
}

/// How a termination is delivered.
#[non_exhaustive]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TerminateMode {
  Graceful,
  Force,
}

/// Who asked for the termination.
#[non_exhaustive]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TerminateOrigin {
  Manual,
  FreeUp,
  Advisor,
}

/// The mechanism used to terminate a process.
#[non_exhaustive]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TerminateMethod {
  AppQuit,
  Sigterm,
  Sigkill,
}

/// A warning shown next to a planned termination.
#[non_exhaustive]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RiskFlag {
  AppMayPromptToSave,
  UnsavedWorkMayBeLost,
  EndsChildProcesses,
  BackgroundAgent,
  RecentlyUsed,
  Restarts,
}

/// Why a target was refused.
#[non_exhaustive]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RefusalCode {
  Protected,
  NotFound,
  IdentityChanged,
  Duplicate,
  TooMany,
  Frontmost,
}

/// The result of acting on one planned entry.
#[non_exhaustive]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TerminateStatus {
  Exited,
  StillRunning,
  AlreadyGone,
  IdentityChanged,
  Refused,
  Failed,
}

/// Why a group is offered as a free-up candidate.
#[non_exhaustive]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ReasonCode {
  Large,
  Idle,
  Background,
  Growing,
  ManyProcesses,
}

/// One thing to terminate: a whole group or a single process identity.
///
/// Tagged on `kind` so the two arms are disjoint on the wire.
#[non_exhaustive]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(
  tag = "kind",
  rename_all = "camelCase",
  rename_all_fields = "camelCase",
  deny_unknown_fields
)]
pub enum TerminateTarget {
  /// Terminate every process in the group.
  Group {
    /// The group to terminate.
    group_id: GroupId,
  },
  /// Terminate one process, refusing if its start time changed.
  Process {
    /// The operating-system process id.
    pid: u32,
    /// The process start time in microseconds since the Unix epoch.
    start_time_us: u64,
  },
}

/// The system-wide memory accounting for one sample.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryBreakdown {
  pub total_bytes: u64,
  pub used_bytes: u64,
  pub free_bytes: u64,
  pub available_bytes: Option<u64>,
  pub app_bytes: Option<u64>,
  pub wired_bytes: Option<u64>,
  pub compressed_bytes: Option<u64>,
  pub cached_files_bytes: Option<u64>,
  pub swap_total_bytes: u64,
  pub swap_used_bytes: u64,
}

/// The current memory-pressure reading and where it came from.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryPressure {
  pub level: PressureLevel,
  pub source: PressureSource,
  pub available_percent: Option<f32>,
}

/// One process as shown in the table.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRow {
  pub pid: u32,
  pub ppid: Option<u32>,
  pub start_time_us: u64,
  pub name: String,
  pub owned_by_current_user: bool,
  pub group_id: GroupId,
  pub kind: ProcessKind,
  pub cpu_percent: Option<f32>,
  pub footprint_bytes: Option<u64>,
  pub resident_bytes: Option<u64>,
  pub virtual_bytes: Option<u64>,
  pub restricted: bool,
  pub protection: Option<Protection>,
  /// The full command line, joined with spaces. `None` when the kernel does
  /// not let this user read it (another account, restricted, or a platform
  /// without a reader). Capped at [`crate::group::MAX_COMMAND_BYTES`].
  pub command: Option<String>,
  /// The owning account name, when it can be resolved.
  pub user: Option<String>,
  /// The scheduler state at sample time.
  pub state: ProcessState,
  /// The number of threads, when the platform reports it.
  pub threads: Option<u32>,
  /// TCP ports this process is listening on, sorted ascending.
  pub listening_ports: Vec<u16>,
}

/// A group of related processes treated as one unit.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessGroup {
  pub group_id: GroupId,
  pub name: String,
  pub bundle_id: Option<String>,
  pub bundle_path: Option<String>,
  pub kind: ProcessKind,
  pub main_pid: u32,
  pub pids: Vec<u32>,
  pub cpu_percent: Option<f32>,
  pub footprint_bytes: u64,
  pub resident_bytes: u64,
  pub frontmost: bool,
  pub last_frontmost_at: Option<String>,
  pub started_at: String,
  pub protection: Option<Protection>,
}

/// One complete sample of system and process memory.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MemorySnapshot {
  pub sequence: u64,
  pub sampled_at: String,
  pub interval_ms: u32,
  pub platform: Platform,
  pub breakdown: MemoryBreakdown,
  pub pressure: MemoryPressure,
  pub groups: Vec<ProcessGroup>,
  pub processes: Vec<ProcessRow>,
  pub notes: Vec<SnapshotNote>,
}

/// A request to plan the termination of one or more targets.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TerminatePlanRequest {
  pub targets: Vec<TerminateTarget>,
  pub mode: TerminateMode,
  pub origin: TerminateOrigin,
}

/// One process inside a termination plan.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TerminatePlanEntry {
  pub pid: u32,
  pub start_time_us: u64,
  pub name: String,
  pub group_id: GroupId,
  pub footprint_bytes: Option<u64>,
  pub method: TerminateMethod,
  pub risks: Vec<RiskFlag>,
  pub descendant_count: u32,
}

/// A refused target inside a termination plan.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TerminateRefusal {
  pub pid: Option<u32>,
  pub group_id: Option<GroupId>,
  pub name: String,
  pub code: RefusalCode,
  pub protection: Option<Protection>,
}

/// A ready-to-execute termination plan.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TerminatePlan {
  pub plan_id: PlanId,
  pub mode: TerminateMode,
  pub origin: TerminateOrigin,
  pub entries: Vec<TerminatePlanEntry>,
  pub refused: Vec<TerminateRefusal>,
  pub expected_release_bytes: u64,
  pub created_at: String,
  pub expires_at: String,
}

/// A request to execute a previously built plan.
///
/// `confirm` must be the literal `true`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TerminateExecuteRequest {
  pub plan_id: PlanId,
  pub confirm: bool,
}

/// The outcome for one process in an executed plan.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TerminateResult {
  pub pid: u32,
  pub start_time_us: u64,
  pub name: String,
  pub status: TerminateStatus,
  pub method: TerminateMethod,
  pub errno: Option<i32>,
}

/// The outcome of executing a plan.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TerminateOutcome {
  pub plan_id: PlanId,
  pub results: Vec<TerminateResult>,
  pub released_bytes_estimate: u64,
  pub audit_ids: Vec<AuditId>,
}

/// One reason a group is offered for freeing memory.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CandidateReason {
  pub code: ReasonCode,
  pub bytes: Option<u64>,
  pub minutes: Option<u32>,
  pub cpu_percent: Option<f32>,
  pub count: Option<u32>,
  pub from_bytes: Option<u64>,
}

/// A group offered for freeing memory.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FreeUpCandidate {
  pub group_id: GroupId,
  pub name: String,
  pub footprint_bytes: u64,
  pub reasons: Vec<CandidateReason>,
  pub risks: Vec<RiskFlag>,
  pub preselected: bool,
}

/// The list of free-up candidates for the latest snapshot.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FreeUpCandidates {
  pub sequence: u64,
  pub pressure: MemoryPressure,
  pub observed_minutes: u32,
  pub candidates: Vec<FreeUpCandidate>,
  pub potential_release_bytes: u64,
}

/// A request for free-up candidates, excluding some groups.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CandidatesRequest {
  pub exclude: Vec<GroupId>,
}

/// One point in the system history series.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SystemPoint {
  pub at: String,
  pub used_bytes: u64,
  pub compressed_bytes: Option<u64>,
  pub swap_used_bytes: u64,
  pub pressure: PressureLevel,
}

/// The footprint history of one group.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GroupSeries {
  pub group_id: GroupId,
  pub name: String,
  pub footprint_bytes: Vec<Option<u64>>,
}

/// The requested history window.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryHistory {
  pub step_ms: u32,
  pub points: Vec<SystemPoint>,
  pub groups: Vec<GroupSeries>,
}

/// A request for history, selecting up to eight groups.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRequest {
  pub group_ids: Vec<GroupId>,
}

/// One recorded termination.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuditRecord {
  pub audit_id: AuditId,
  pub at: String,
  pub origin: TerminateOrigin,
  pub mode: TerminateMode,
  pub method: TerminateMethod,
  pub pid: u32,
  pub start_time_us: u64,
  pub name: String,
  pub group_name: String,
  pub bundle_id: Option<String>,
  pub status: TerminateStatus,
  pub footprint_bytes: Option<u64>,
}

/// The audit ring, newest first.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuditLog {
  pub records: Vec<AuditRecord>,
}
