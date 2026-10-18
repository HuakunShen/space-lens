//! The stdio protocol spoken by `spacelens memory stdio` (§4.6 of
//! docs/memory-cleaner-plan.md). The child writes one JSON object per line, both
//! directions, UTF-8, at most four mebibytes per line: a `hello` first, then one
//! correlated reply (`{"id":N,"ok":true,"result":…}` or
//! `{"id":N,"ok":false,"problem":{…}}`) for every call. It exits on `shutdown`,
//! on EOF of the request stream, or after `idle_exit_ms` with no call.
//!
//! [`serve_stdio`] reads through a small reader thread so an idle stream can
//! time out, and it discards any line over [`MAX_LINE_BYTES`] with an
//! `InvalidRequest` problem instead of buffering it (the host kills an
//! overflowing child anyway, so this is the defensive backstop). This is the
//! only module allowed to hold a `serde_json::Value`.

use crate::error::MemoryError;
use crate::model::{
  CandidatesRequest, HistoryRequest, Platform, TerminateExecuteRequest, TerminatePlanRequest,
};
use crate::service::MemoryService;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, Read, Write};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::thread;
use std::time::Duration;

/// The protocol version carried in the `hello` line.
pub const PROTOCOL_VERSION: u32 = 1;

/// The most bytes one request or reply line may take.
pub const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

/// The capability line the child writes once at start (§4.6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
  /// The protocol version the child speaks.
  pub protocol: u32,
  /// The platform the child samples.
  pub platform: Platform,
  /// Whether the child can plan and execute terminations.
  pub terminate: bool,
  /// Whether the AppKit graceful quit is available.
  pub graceful_app_quit: bool,
}

/// The `{"hello":{…}}` envelope of the first line.
#[derive(Debug, Serialize)]
struct HelloEnvelope {
  hello: Hello,
}

/// One call from the host, tagged on `method` (§4.6).
#[derive(Debug, Deserialize)]
#[serde(tag = "method", rename_all = "camelCase", deny_unknown_fields)]
enum Call {
  Snapshot,
  History { request: HistoryRequest },
  Candidates { request: CandidatesRequest },
  Audit,
  Plan { request: TerminatePlanRequest },
  Execute { request: TerminateExecuteRequest },
  Shutdown,
}

/// One request line. `call` stays untyped so an unknown method still carries a
/// correlatable [`Request::id`] into the failure reply.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Request {
  id: u64,
  #[serde(default)]
  session_id: String,
  call: Value,
}

/// One reply line: exactly one of `result`/`problem` is present.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Reply {
  id: u64,
  ok: bool,
  #[serde(skip_serializing_if = "Option::is_none")]
  result: Option<Value>,
  #[serde(skip_serializing_if = "Option::is_none")]
  problem: Option<Problem>,
}

impl Reply {
  /// A successful reply holding `value`.
  fn ok<T: Serialize>(id: u64, value: &T) -> Reply {
    match serde_json::to_value(value) {
      Ok(value) => Reply {
        id,
        ok: true,
        result: Some(value),
        problem: None,
      },
      Err(error) => Reply::failure(
        id,
        &MemoryError::Internal {
          detail: format!("serializing the reply failed: {error}"),
        },
      ),
    }
  }

  /// A reply for a service call, mapping an error onto its problem.
  fn from_result<T: Serialize>(id: u64, result: Result<T, MemoryError>) -> Reply {
    match result {
      Ok(value) => Reply::ok(id, &value),
      Err(error) => Reply::failure(id, &error),
    }
  }

  /// A failure reply carrying the error's closed problem mapping.
  fn failure(id: u64, error: &MemoryError) -> Reply {
    Reply {
      id,
      ok: false,
      result: None,
      problem: Some(Problem {
        code: error.problem_code().to_string(),
        message: error.to_string(),
        retryable: error.retryable(),
      }),
    }
  }
}

/// The problem object of a failed reply.
#[derive(Debug, Serialize)]
struct Problem {
  code: String,
  message: String,
  retryable: bool,
}

/// What the reader thread hands to the protocol loop.
#[derive(Debug)]
enum Incoming {
  /// One complete line, including its newline.
  Line(String),
  /// A line longer than [`MAX_LINE_BYTES`]; the rest of it has been discarded.
  Oversized,
  /// The request stream ended.
  Eof,
  /// Reading the stream failed.
  Error(std::io::Error),
}

/// What the protocol loop does after one line.
enum Action {
  Reply(Reply),
  Shutdown,
  Ignore,
}

/// Serve the stdio protocol until `shutdown`, EOF or the idle timeout.
///
/// The reader must be `Send + 'static` because reading runs on its own thread
/// so the idle timeout can end a stream that is still open.
pub fn serve_stdio(
  service: &MemoryService,
  input: impl BufRead + Send + 'static,
  mut output: impl Write,
  idle_exit_ms: u64,
) -> Result<(), MemoryError> {
  let (sender, receiver) = mpsc::sync_channel(8);
  thread::spawn(move || read_lines(input, sender));

  write_line(
    &mut output,
    &HelloEnvelope {
      hello: service.capabilities(),
    },
  )?;

  let idle = Duration::from_millis(idle_exit_ms);
  loop {
    match receiver.recv_timeout(idle) {
      Ok(Incoming::Line(line)) => match handle_line(service, &line) {
        Action::Reply(reply) => write_line(&mut output, &reply)?,
        Action::Shutdown => break,
        Action::Ignore => {}
      },
      Ok(Incoming::Oversized) => {
        let error = MemoryError::InvalidRequest {
          detail: format!("the request line exceeds the {MAX_LINE_BYTES} byte limit."),
        };
        write_line(&mut output, &Reply::failure(0, &error))?;
      }
      Ok(Incoming::Eof) | Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => {
        break
      }
      Ok(Incoming::Error(error)) => {
        return Err(MemoryError::Internal {
          detail: format!("reading the request stream failed: {error}"),
        });
      }
    }
  }
  Ok(())
}

/// Read request lines until end of input, reporting each to `sender`.
fn read_lines(mut input: impl BufRead + Send + 'static, sender: SyncSender<Incoming>) {
  loop {
    let mut line: Vec<u8> = Vec::new();
    let read = {
      let mut limited = (&mut input).take(MAX_LINE_BYTES as u64 + 1);
      limited.read_until(b'\n', &mut line)
    };
    match read {
      Err(error) => {
        let _ = sender.send(Incoming::Error(error));
        return;
      }
      Ok(0) => {
        let _ = sender.send(Incoming::Eof);
        return;
      }
      Ok(_) => {}
    }

    if line.len() > MAX_LINE_BYTES {
      if !line.ends_with(b"\n") {
        discard_line(&mut input);
      }
      if sender.send(Incoming::Oversized).is_err() {
        return;
      }
      continue;
    }

    match String::from_utf8(line) {
      Ok(text) => {
        if sender.send(Incoming::Line(text)).is_err() {
          return;
        }
      }
      Err(_) => {
        if sender.send(Incoming::Oversized).is_err() {
          return;
        }
      }
    }
  }
}

/// Read and drop the remainder of an overlong line, through its newline.
fn discard_line(input: &mut impl BufRead) {
  loop {
    let mut scratch: Vec<u8> = Vec::new();
    match input.read_until(b'\n', &mut scratch) {
      Ok(0) => return,
      Ok(_) => {
        if scratch.ends_with(b"\n") {
          return;
        }
      }
      Err(_) => return,
    }
  }
}

/// Turn one request line into the action it calls for.
fn handle_line(service: &MemoryService, line: &str) -> Action {
  let text = line.trim();
  if text.is_empty() {
    return Action::Ignore;
  }

  let request: Request = match serde_json::from_str(text) {
    Ok(request) => request,
    Err(error) => return Action::Reply(Reply::failure(0, &invalid_request(&error))),
  };

  let Request {
    id,
    session_id,
    call,
  } = request;

  let call: Call = match serde_json::from_value(call) {
    Ok(call) => call,
    Err(error) => return Action::Reply(Reply::failure(id, &invalid_request(&error))),
  };

  match call {
    Call::Shutdown => Action::Shutdown,
    Call::Snapshot => Action::Reply(Reply::from_result(id, service.snapshot())),
    Call::History { request } => Action::Reply(Reply::from_result(id, service.history(&request))),
    Call::Candidates { request } => {
      Action::Reply(Reply::from_result(id, service.candidates(&request)))
    }
    Call::Audit => Action::Reply(Reply::ok(id, &service.audit())),
    Call::Plan { request } => {
      Action::Reply(Reply::from_result(id, service.plan(&session_id, &request)))
    }
    Call::Execute { request } => Action::Reply(Reply::from_result(
      id,
      service.execute(&session_id, &request),
    )),
  }
}

/// The `InvalidRequest` a malformed request line decodes to.
fn invalid_request(error: &serde_json::Error) -> MemoryError {
  MemoryError::InvalidRequest {
    detail: format!("the request is not a valid call: {error}"),
  }
}

/// Serialize `value` as one JSON line and flush it.
fn write_line<W: Write>(output: &mut W, value: &impl Serialize) -> Result<(), MemoryError> {
  let mut line = serde_json::to_string(value).map_err(|error| MemoryError::Internal {
    detail: format!("serializing a protocol line failed: {error}"),
  })?;
  line.push('\n');
  output
    .write_all(line.as_bytes())
    .map_err(|error| MemoryError::Internal {
      detail: format!("writing a protocol line failed: {error}"),
    })?;
  output.flush().map_err(|error| MemoryError::Internal {
    detail: format!("flushing the protocol stream failed: {error}"),
  })?;
  Ok(())
}
