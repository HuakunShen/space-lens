//! Contract drift guard and wire-budget check (§4.1 and §6.6 of
//! docs/memory-cleaner-plan.md). Every instance in the shared
//! `contract-sample.json` fixture must deserialize into its Rust wire type and
//! re-serialize to exactly the same JSON, so a drift against the Zod mirror in
//! `packages/contract` fails here; the second test keeps a 1000-process fixture
//! inside the size budget.

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use spacelens_memory::{
  AuditLog, AuditRecord, CandidateReason, CandidatesRequest, FreeUpCandidate, FreeUpCandidates,
  GroupId, GroupSeries, HistoryRequest, MemoryBreakdown, MemoryHistory, MemoryPressure,
  MemorySnapshot, ProcessGroup, ProcessKind, ProcessRow, ProcessState, SystemPoint,
  TerminateExecuteRequest, TerminateOutcome, TerminatePlan, TerminatePlanEntry,
  TerminatePlanRequest, TerminateRefusal, TerminateResult, TerminateTarget,
};

/// The fixture that holds one valid instance of every wire type, shared with
/// the TypeScript contract tests.
fn fixture() -> Value {
  let path = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/contract-sample.json"
  );
  let raw = std::fs::read_to_string(path).expect("read contract-sample.json");
  serde_json::from_str(&raw).expect("parse contract-sample.json")
}

/// Deserialize `root[key]` into `T` and assert it serializes back unchanged.
fn assert_round_trips<T: Serialize + DeserializeOwned>(root: &Value, key: &str) {
  let original = root
    .get(key)
    .unwrap_or_else(|| panic!("fixture is missing `{key}`"));
  let typed: T = serde_json::from_value(original.clone())
    .unwrap_or_else(|error| panic!("`{key}` failed to deserialize: {error}"));
  let back = serde_json::to_value(&typed)
    .unwrap_or_else(|error| panic!("`{key}` failed to re-serialize: {error}"));
  assert_eq!(&back, original, "`{key}` did not round-trip");
}

#[test]
fn every_fixture_key_round_trips() {
  let root = fixture();

  assert_round_trips::<MemoryBreakdown>(&root, "MemoryBreakdown");
  assert_round_trips::<MemoryPressure>(&root, "MemoryPressure");
  assert_round_trips::<ProcessRow>(&root, "ProcessRow");
  assert_round_trips::<ProcessGroup>(&root, "ProcessGroup");
  assert_round_trips::<MemorySnapshot>(&root, "MemorySnapshot");
  assert_round_trips::<TerminateTarget>(&root, "TerminateTarget");
  assert_round_trips::<TerminatePlanRequest>(&root, "TerminatePlanRequest");
  assert_round_trips::<TerminatePlanEntry>(&root, "TerminatePlanEntry");
  assert_round_trips::<TerminateRefusal>(&root, "TerminateRefusal");
  assert_round_trips::<TerminatePlan>(&root, "TerminatePlan");
  assert_round_trips::<TerminateExecuteRequest>(&root, "TerminateExecuteRequest");
  assert_round_trips::<TerminateResult>(&root, "TerminateResult");
  assert_round_trips::<TerminateOutcome>(&root, "TerminateOutcome");
  assert_round_trips::<CandidateReason>(&root, "CandidateReason");
  assert_round_trips::<FreeUpCandidate>(&root, "FreeUpCandidate");
  assert_round_trips::<FreeUpCandidates>(&root, "FreeUpCandidates");
  assert_round_trips::<CandidatesRequest>(&root, "CandidatesRequest");
  assert_round_trips::<SystemPoint>(&root, "SystemPoint");
  assert_round_trips::<GroupSeries>(&root, "GroupSeries");
  assert_round_trips::<MemoryHistory>(&root, "MemoryHistory");
  assert_round_trips::<HistoryRequest>(&root, "HistoryRequest");
  assert_round_trips::<AuditRecord>(&root, "AuditRecord");
  assert_round_trips::<AuditLog>(&root, "AuditLog");

  let known: std::collections::BTreeSet<&str> = [
    "MemoryBreakdown",
    "MemoryPressure",
    "ProcessRow",
    "ProcessGroup",
    "MemorySnapshot",
    "TerminateTarget",
    "TerminatePlanRequest",
    "TerminatePlanEntry",
    "TerminateRefusal",
    "TerminatePlan",
    "TerminateExecuteRequest",
    "TerminateResult",
    "TerminateOutcome",
    "CandidateReason",
    "FreeUpCandidate",
    "FreeUpCandidates",
    "CandidatesRequest",
    "SystemPoint",
    "GroupSeries",
    "MemoryHistory",
    "HistoryRequest",
    "AuditRecord",
    "AuditLog",
  ]
  .into_iter()
  .collect();
  let actual: std::collections::BTreeSet<&str> = root
    .as_object()
    .expect("the fixture is a JSON object")
    .keys()
    .map(String::as_str)
    .collect();
  assert_eq!(actual, known, "the fixture holds an unexpected set of keys");
}

/// A valid group id for the synthetic fixture; the body is a 16-digit hex pad
/// of `value`, matching `^grp_[0-9a-f]{16}$`.
fn synthetic_group_id(value: usize) -> GroupId {
  GroupId::try_from(format!("grp_{value:016x}")).expect("synthetic group ids are well formed")
}

fn synthetic_row(index: usize) -> ProcessRow {
  ProcessRow {
    pid: index as u32 + 1,
    ppid: None,
    start_time_us: index as u64,
    name: format!("p{index}"),
    owned_by_current_user: true,
    group_id: synthetic_group_id(index % 300),
    kind: ProcessKind::Cli,
    cpu_percent: None,
    footprint_bytes: None,
    resident_bytes: None,
    virtual_bytes: None,
    restricted: false,
    protection: None,
    command: Some(format!(
      "/usr/local/bin/worker --job {index} --token 0123456789abcdef"
    )),
    user: Some("hk".to_string()),
    state: ProcessState::Sleeping,
    threads: Some(2),
    listening_ports: vec![],
  }
}

fn synthetic_group(index: usize) -> ProcessGroup {
  ProcessGroup {
    group_id: synthetic_group_id(index),
    name: format!("app{index}"),
    bundle_id: None,
    bundle_path: None,
    kind: ProcessKind::App,
    main_pid: index as u32 + 1,
    pids: vec![index as u32 + 1],
    cpu_percent: None,
    footprint_bytes: index as u64,
    resident_bytes: index as u64,
    frontmost: false,
    last_frontmost_at: None,
    started_at: "2026-10-18T09:00:00.000Z".to_string(),
    protection: None,
  }
}

/// §6.6 wire budget. The fixture is a 1 Hz snapshot with 1000 process rows and
/// 300 groups. The frozen field set cannot fit both collections inside a single
/// document: the required field names, ids, command lines and 16-digit
/// timestamps put the process floor near 358 KB, so the budget is asserted for
/// each collection. The command column is part of the budget on purpose — a
/// process view that only pays for it when it is shown would hide the 1 Hz cost.
#[test]
fn thousand_process_fixture_stays_within_wire_budget() {
  let processes: Vec<ProcessRow> = (0..1000).map(synthetic_row).collect();
  let groups: Vec<ProcessGroup> = (0..300).map(synthetic_group).collect();

  let processes_bytes = serde_json::to_vec(&processes)
    .expect("serialize processes")
    .len();
  let groups_bytes = serde_json::to_vec(&groups).expect("serialize groups").len();

  assert!(
    processes_bytes <= 400_000,
    "1000 process rows serialize to {processes_bytes} bytes"
  );
  assert!(
    groups_bytes <= 250_000,
    "300 groups serialize to {groups_bytes} bytes"
  );
}
