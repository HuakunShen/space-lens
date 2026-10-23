//! End-to-end test of `spacelens memory stdio` (T12 of
//! docs/memory-cleaner-plan.md): run the built binary, read the `hello` line it
//! writes first, send a `snapshot` call and assert the reply is `ok` with a
//! positive `sequence`, then `shutdown` and assert the child exits cleanly.

use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

/// The child under test, killed and reaped on drop so a failed assertion cannot
/// leave `spacelens memory stdio` running.
struct StdioChild {
  child: Child,
}

impl StdioChild {
  /// Spawn `spacelens memory stdio` with piped stdio.
  fn spawn() -> StdioChild {
    let child = Command::new(env!("CARGO_BIN_EXE_spacelens"))
      .args(["memory", "stdio"])
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .spawn()
      .expect("spawning `spacelens memory stdio` must succeed");
    StdioChild { child }
  }
}

impl Drop for StdioChild {
  fn drop(&mut self) {
    let _ = self.child.kill();
    let _ = self.child.wait();
  }
}

/// Read one line from `reader`, failing if the child closed its output first.
fn read_line(reader: &mut impl BufRead) -> String {
  let mut line = String::new();
  let read = reader
    .read_line(&mut line)
    .expect("reading a protocol line");
  assert!(read > 0, "the child closed its output before replying");
  line
}

#[test]
fn stdio_prints_hello_then_answers_snapshot_and_shutdown() {
  let mut child = StdioChild::spawn();
  let mut stdin = child.child.stdin.take().expect("stdin pipe is open");
  let mut stdout = BufReader::new(child.child.stdout.take().expect("stdout pipe is open"));

  let hello: Value =
    serde_json::from_str(read_line(&mut stdout).trim()).expect("the hello line is JSON");
  let hello = hello
    .get("hello")
    .expect("the first line is the hello envelope");
  assert_eq!(hello.get("protocol").and_then(Value::as_u64), Some(1));
  assert!(
    hello.get("platform").and_then(Value::as_str).is_some(),
    "hello names the platform"
  );
  assert!(
    hello.get("terminate").and_then(Value::as_bool).is_some(),
    "hello says whether termination is available"
  );
  assert!(
    hello
      .get("gracefulAppQuit")
      .and_then(Value::as_bool)
      .is_some(),
    "hello says whether AppKit quit is available"
  );

  let call = serde_json::json!({
    "id": 1,
    "sessionId": "sess_cli_test",
    "call": { "method": "snapshot" }
  });
  writeln!(stdin, "{call}").expect("writing the snapshot call");
  stdin.flush().expect("flushing the snapshot call");

  let reply: Value =
    serde_json::from_str(read_line(&mut stdout).trim()).expect("the reply line is JSON");
  assert_eq!(
    reply.get("ok").and_then(Value::as_bool),
    Some(true),
    "the snapshot reply is ok: {reply}"
  );
  let sequence = reply
    .get("result")
    .and_then(|result| result.get("sequence"))
    .and_then(Value::as_u64)
    .expect("the snapshot carries a sequence");
  assert!(
    sequence > 0,
    "the first published snapshot has a positive sequence"
  );

  let shutdown = serde_json::json!({
    "id": 2,
    "sessionId": "",
    "call": { "method": "shutdown" }
  });
  writeln!(stdin, "{shutdown}").expect("writing the shutdown call");
  stdin.flush().expect("flushing the shutdown call");

  let status = child.child.wait().expect("waiting for the child");
  assert!(
    status.success(),
    "the child exits cleanly after shutdown: {status}"
  );
}
