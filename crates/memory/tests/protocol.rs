//! Stdio protocol cases (T10 and §4.6 of docs/memory-cleaner-plan.md): the
//! child writes one `hello` line, answers each request line with a correlated
//! reply, reports a problem for malformed, unknown and oversized lines, and
//! exits on `shutdown`, on EOF or after the idle timeout. The requests are fed
//! from an in-memory `Cursor`, so the framing is tested without a pipe.

use serde_json::Value;
use spacelens_memory::fake::{FakeClock, FakeSleeper};
use spacelens_memory::platform::NullApps;
use spacelens_memory::raw::{RawProcess, RawSample, RawSystem};
use spacelens_memory::sampler::{Clock, ProcessSampler, Sleeper};
use spacelens_memory::{MemoryError, MemoryPressure, MemoryService, Platform, PressureLevel};
use std::io::{BufReader, Cursor};
use std::sync::Arc;

/// The instant the fake clock starts at.
const NOW: i64 = 1_700_000_000_000;
/// A pid that belongs to neither the service nor its sample.
const OWN_PID: u32 = 999_999;
/// The uid the service and the sampled process both run as.
const UID: u32 = 501;
/// The sampled process a plan can target.
const WORKER_PID: u32 = 4001;
/// The exact start time of the sampled process.
const WORKER_START_US: u64 = 1_700_000_000_000_000 + WORKER_PID as u64;

/// A [`ProcessSampler`] that returns the same minimal sample on every tick.
#[derive(Debug)]
struct CannedSampler;

impl ProcessSampler for CannedSampler {
  fn platform(&self) -> Platform {
    Platform::Linux
  }

  fn sample(&mut self) -> Result<RawSample, MemoryError> {
    Ok(RawSample {
      system: RawSystem {
        total_bytes: 16_000_000_000,
        used_bytes: 8_000_000_000,
        free_bytes: 4_000_000_000,
        available_bytes: Some(6_000_000_000),
        swap_total_bytes: 0,
        swap_used_bytes: 0,
        vm: None,
        pressure: MemoryPressure {
          level: PressureLevel::Normal,
          source: spacelens_memory::PressureSource::Derived,
          available_percent: Some(37.5),
        },
      },
      processes: vec![RawProcess {
        pid: WORKER_PID,
        ppid: Some(900_000),
        start_time_us: WORKER_START_US,
        uid: Some(UID),
        name: "worker".to_string(),
        exe: Some("/usr/local/bin/worker".into()),
        cpu_percent: Some(0.5),
        footprint_bytes: Some(512 * 1024 * 1024),
        resident_bytes: Some(256 * 1024 * 1024),
        virtual_bytes: None,
        zombie: false,
      }],
    })
  }
}

/// A service around the canned sampler, ready to speak the protocol.
fn service() -> Arc<MemoryService> {
  let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(NOW));
  let sleeper: Arc<dyn Sleeper> = Arc::new(FakeSleeper::new(Arc::new(FakeClock::new(NOW))));
  MemoryService::start(
    Box::new(CannedSampler),
    Box::new(NullApps),
    Box::new(spacelens_memory::fake::FakeSignaller::new(
      OWN_PID,
      Some(UID),
    )),
    clock,
    sleeper,
    None,
  )
}

/// Run `input` through `serve_stdio` and parse every output line as JSON.
fn exchange(input: &str) -> Vec<Value> {
  let service = service();
  let mut output = Vec::new();
  spacelens_memory::serve_stdio(
    &service,
    Cursor::new(input.as_bytes().to_vec()),
    &mut output,
    60_000,
  )
  .expect("serve_stdio finishes cleanly");
  String::from_utf8(output)
    .expect("the protocol output is UTF-8")
    .lines()
    .map(|line| serde_json::from_str::<Value>(line).expect("every output line is JSON"))
    .collect()
}

const SHUTDOWN: &str = "{\"id\":99,\"sessionId\":\"\",\"call\":{\"method\":\"shutdown\"}}\n";

#[test]
fn hello_leads_and_snapshot_replies_with_a_sequence() {
  let input =
    format!("{{\"id\":1,\"sessionId\":\"s\",\"call\":{{\"method\":\"snapshot\"}}}}\n{SHUTDOWN}");
  let lines = exchange(&input);

  assert_eq!(lines.len(), 2, "hello plus one reply");
  assert_eq!(lines[0]["hello"]["protocol"], 1);
  assert_eq!(lines[0]["hello"]["platform"], "linux");
  assert_eq!(lines[0]["hello"]["terminate"], true);
  assert_eq!(
    lines[0]["hello"]["gracefulAppQuit"],
    cfg!(all(target_os = "macos", feature = "appkit")),
    "graceful AppKit quit is available exactly on macOS with the appkit feature"
  );

  assert_eq!(lines[1]["id"], 1);
  assert_eq!(lines[1]["ok"], true);
  assert!(
    lines[1]["result"]["sequence"].as_u64().unwrap() >= 1,
    "the snapshot carries a real sequence"
  );
}

#[test]
fn read_calls_round_trip() {
  let input = format!(
    "{{\"id\":1,\"sessionId\":\"s\",\"call\":{{\"method\":\"snapshot\"}}}}\n\
     {{\"id\":2,\"sessionId\":\"s\",\"call\":{{\"method\":\"history\",\"request\":{{\"groupIds\":[]}}}}}}\n\
     {{\"id\":3,\"sessionId\":\"s\",\"call\":{{\"method\":\"candidates\",\"request\":{{\"exclude\":[]}}}}}}\n\
     {{\"id\":4,\"sessionId\":\"s\",\"call\":{{\"method\":\"audit\"}}}}\n{SHUTDOWN}"
  );
  let lines = exchange(&input);

  assert_eq!(lines.len(), 5, "hello plus four replies");
  for (index, id) in [1u64, 2, 3, 4].iter().enumerate() {
    assert_eq!(lines[index + 1]["id"], *id);
    assert_eq!(lines[index + 1]["ok"], true, "call {id} succeeds");
  }
  assert!(lines[2]["result"]["points"].is_array());
  assert!(lines[3]["result"]["candidates"].is_array());
  assert!(lines[4]["result"]["records"].is_array());
}

#[test]
fn an_unknown_method_is_an_invalid_request() {
  let input =
    format!("{{\"id\":9,\"sessionId\":\"s\",\"call\":{{\"method\":\"frobnicate\"}}}}\n{SHUTDOWN}");
  let lines = exchange(&input);

  assert_eq!(lines.len(), 2);
  assert_eq!(lines[1]["id"], 9);
  assert_eq!(lines[1]["ok"], false);
  assert_eq!(lines[1]["problem"]["code"], "InvalidRequest");
}

#[test]
fn malformed_json_is_an_invalid_request() {
  let input = format!("this is not json\n{SHUTDOWN}");
  let lines = exchange(&input);

  assert_eq!(lines.len(), 2);
  assert_eq!(lines[1]["ok"], false);
  assert_eq!(lines[1]["problem"]["code"], "InvalidRequest");
}

#[test]
fn a_line_over_four_mib_is_discarded_with_a_problem() {
  let padding = "x".repeat(4 * 1024 * 1024 + 10);
  let input = format!(
    "{{\"id\":5,\"sessionId\":\"s\",\"call\":{{\"method\":\"snapshot\"}},\"pad\":\"{padding}\"}}\n{SHUTDOWN}"
  );
  let lines = exchange(&input);

  assert_eq!(
    lines.len(),
    2,
    "hello plus the one problem, no snapshot reply"
  );
  assert_eq!(lines[1]["ok"], false);
  assert_eq!(lines[1]["problem"]["code"], "InvalidRequest");
}

#[test]
fn plan_and_execute_arms_are_reachable() {
  let target = format!("\"pid\":{WORKER_PID},\"startTimeUs\":{WORKER_START_US}");
  let input = format!(
    "{{\"id\":1,\"sessionId\":\"s\",\"call\":{{\"method\":\"snapshot\"}}}}\n\
     {{\"id\":2,\"sessionId\":\"s\",\"call\":{{\"method\":\"plan\",\"request\":{{\"targets\":[{{\"kind\":\"process\",{target}}}],\"mode\":\"graceful\",\"origin\":\"manual\"}}}}}}\n\
     {{\"id\":3,\"sessionId\":\"s\",\"call\":{{\"method\":\"execute\",\"request\":{{\"planId\":\"tplan_0000000000000000\",\"confirm\":true}}}}}}\n\
     {{\"id\":4,\"sessionId\":\"s\",\"call\":{{\"method\":\"execute\",\"request\":{{\"planId\":\"tplan_0000000000000000\",\"confirm\":false}}}}}}\n{SHUTDOWN}"
  );
  let lines = exchange(&input);

  assert_eq!(lines.len(), 5, "hello plus four replies");
  assert_eq!(lines[2]["ok"], true, "the plan is built");
  assert_eq!(lines[2]["result"]["entries"].as_array().unwrap().len(), 1);
  assert_eq!(lines[3]["problem"]["code"], "NotFound");
  assert_eq!(
    lines[4]["problem"]["code"], "InvalidRequest",
    "an unconfirmed execute is refused before the plan is looked up"
  );
}

#[test]
fn an_idle_stream_exits_after_the_timeout() {
  let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
  let address = listener.local_addr().expect("the bound address");
  let writer = std::net::TcpStream::connect(address).expect("connect to the listener");
  let (reader, _) = listener.accept().expect("accept the connection");

  let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(NOW));
  let sleeper: Arc<dyn Sleeper> = Arc::new(FakeSleeper::new(Arc::new(FakeClock::new(NOW))));
  let service = MemoryService::start(
    Box::new(CannedSampler),
    Box::new(NullApps),
    Box::new(spacelens_memory::fake::FakeSignaller::new(
      OWN_PID,
      Some(UID),
    )),
    clock,
    sleeper,
    None,
  );

  let mut output = Vec::new();
  spacelens_memory::serve_stdio(&service, BufReader::new(reader), &mut output, 30)
    .expect("serve_stdio exits on idle");

  let text = String::from_utf8(output).expect("UTF-8");
  assert_eq!(text.lines().count(), 1, "only the hello line is written");
  assert!(text.contains("\"hello\""));

  drop(writer);
}
