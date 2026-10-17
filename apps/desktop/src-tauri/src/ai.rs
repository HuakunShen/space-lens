//! BYOK AI proxy (slice B of `docs/ai-advisor-plan.md`).
//!
//! The WebView's agent loop (`apps/web/src/lib/ai`) owns the conversation and
//! the tool budget; Rust owns exactly two things it cannot own itself: the API
//! key and the HTTPS call. The key is stored in the app config dir `ai.json`
//! (0600) and is never handed back to the renderer — `ai_get_config` reports
//! only whether one exists. The command set is closed: no raw URL, no header
//! passthrough, so the WebView can ask for one chat completion — buffered
//! (`ai_chat`) or streamed chunk-by-chunk (`ai_chat_stream`, cancelled by
//! `ai_chat_cancel`) — and nothing else.

use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

/// Long enough for a slow tool-calling completion, short enough to fail before
/// a user gives up on the panel.
const CHAT_TIMEOUT: Duration = Duration::from_secs(120);
/// The response body kept in an error message. A 300-char tail is enough to see
/// "invalid api key" and small enough that a proxy's HTML error page never
/// floods the transcript.
const MAX_ERROR_BODY_CHARS: usize = 300;
const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
const CONFIG_FILE_NAME: &str = "ai.json";
const CHAT_COMPLETIONS_PATH: &str = "/chat/completions";
/// `docs/ai-advisor-plan.md` "Context and memory budget" item 5. The WebView
/// transcript is already bounded, so an oversized body is a caller bug: it is
/// refused before the key is attached and before anything leaves the machine.
const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;
/// The same budget on the way back: a runaway (or hostile) endpoint must never
/// make the shell buffer an unbounded body.
const MAX_RESPONSE_BODY_BYTES: usize = 2 * 1024 * 1024;
/// The streamed-body cap. A streamed body is forwarded, not buffered, but a
/// runaway endpoint must still be stopped: past this many forwarded bytes the
/// reader aborts and reports an error. Larger than the buffered cap because a
/// full SSE transcript is verbatim upstream bytes, not a re-serialized object.
const MAX_STREAM_RESPONSE_BODY_BYTES: usize = 4 * 1024 * 1024;
/// How many body chunks may queue between the reader task and the command
/// before the reader waits. Bounded backpressure, never unbounded memory.
const STREAM_CHANNEL_CAPACITY: usize = 16;

// ------------------------------------------------------------------- config

/// The on-disk shape. Deliberately *not* the reply shape: `ai_get_config`
/// builds its value by hand (`config_view`), so a stored key has no path into
/// the wire format even if this struct grows later.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct StoredConfig {
  base_url: String,
  model: String,
  /// `None` means no key is stored; a stored key is never returned to the
  /// WebView, only its presence.
  api_key: Option<String>,
  /// Sends `thinking: { type: "disabled" }` (GLM / DeepSeek style) so a reasoning model
  /// answers in seconds instead of tens of seconds. Off by default because strict
  /// OpenAI-compatible servers reject unknown request fields.
  disable_thinking: bool,
}

impl Default for StoredConfig {
  fn default() -> Self {
    Self {
      base_url: DEFAULT_BASE_URL.to_string(),
      model: String::new(),
      api_key: None,
      disable_thinking: false,
    }
  }
}

impl StoredConfig {
  fn has_key(&self) -> bool {
    self.api_key.as_deref().is_some_and(|key| !key.is_empty())
  }
}

/// The frozen reply shape of `ai_get_config`. Hand-built so the key cannot
/// leak through a serialization change to `StoredConfig`.
fn config_view(config: &StoredConfig) -> Value {
  json!({
    "baseUrl": config.base_url,
    "model": config.model,
    "hasKey": config.has_key(),
    "disableThinking": config.disable_thinking,
  })
}

/// Reads the config file. A missing *or unreadable* file falls back to
/// defaults: the frozen reply shape has nowhere to report "your config file is
/// broken", and the panel must open on first run. `ai_set_config` overwrites
/// the file, so a damaged one is repaired by the next save.
fn load_config(path: &Path) -> StoredConfig {
  let Ok(text) = fs::read_to_string(path) else {
    return StoredConfig::default();
  };
  serde_json::from_str(&text).unwrap_or_default()
}

/// Writes the config file atomically and owner-only: a temp file in the same
/// directory (so the rename stays on one filesystem), chmod 0600, then rename
/// over the target. A reader therefore never sees a half-written file.
fn save_config(path: &Path, config: &StoredConfig) -> Result<(), String> {
  let parent = path
    .parent()
    .ok_or_else(|| "AI config path has no parent directory".to_string())?;
  fs::create_dir_all(parent)
    .map_err(|error| format!("cannot create the AI config directory: {error}"))?;
  let bytes = serde_json::to_vec_pretty(config)
    .map_err(|error| format!("cannot serialize the AI config: {error}"))?;
  let temp = temp_path(path);
  if let Err(error) = write_private(&temp, &bytes) {
    let _ = fs::remove_file(&temp);
    return Err(error);
  }
  if let Err(error) = fs::rename(&temp, path) {
    let _ = fs::remove_file(&temp);
    return Err(format!("cannot save the AI config: {error}"));
  }
  Ok(())
}

/// Sibling of the target, process-tagged so two writers cannot collide.
fn temp_path(path: &Path) -> PathBuf {
  let name = path
    .file_name()
    .map(|name| name.to_string_lossy().into_owned())
    .unwrap_or_else(|| CONFIG_FILE_NAME.to_string());
  path.with_file_name(format!("{name}.{}.tmp", std::process::id()))
}

/// Creates the temp file with mode 0600 *at creation* on unix, so the window
/// between "file exists" and "chmod" never leaves the key group/world
/// readable, and re-applies the mode in case a crashed run left the temp name
/// behind.
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
  let mut options = fs::OpenOptions::new();
  options.write(true).create(true).truncate(true);
  #[cfg(unix)]
  {
    use std::os::unix::fs::OpenOptionsExt as _;
    options.mode(0o600);
  }
  let mut file = options
    .open(path)
    .map_err(|error| format!("cannot write the AI config: {error}"))?;
  file
    .write_all(bytes)
    .map_err(|error| format!("cannot write the AI config: {error}"))?;
  file
    .sync_all()
    .map_err(|error| format!("cannot flush the AI config: {error}"))?;
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
      .map_err(|error| format!("cannot restrict the AI config file: {error}"))?;
  }
  Ok(())
}

// ------------------------------------------------------------------ URL guard

/// Where one chat completion goes. Pure and total — the only place a base URL
/// becomes a request target, so the transport rule cannot be bypassed by a
/// later caller.
///
/// `https` is required, except for loopback (`localhost`, `127.0.0.1`,
/// `[::1]`) where plaintext never leaves the machine — that is what makes a
/// local llama.cpp/ollama endpoint usable without weakening remote calls.
/// Userinfo is rejected so credentials cannot hide inside the URL, and the
/// path is appended after trimming trailing slashes so `…/v1/` and `…/v1`
/// produce the same endpoint.
fn chat_endpoint(base_url: &str) -> Result<String, String> {
  let base = base_url.trim().trim_end_matches('/');
  if base.is_empty() {
    return Err("AI base URL is not set".to_string());
  }
  let parsed =
    reqwest::Url::parse(base).map_err(|_| "AI base URL is not a valid URL".to_string())?;
  if !parsed.username().is_empty() || parsed.password().is_some() {
    return Err("AI base URL must not contain credentials".to_string());
  }
  match parsed.scheme() {
    "https" => {}
    "http" if is_loopback(&parsed) => {}
    "http" => return Err("AI base URL must use https unless the host is localhost".to_string()),
    scheme => return Err(format!("AI base URL scheme is not supported: {scheme}")),
  }
  // Pasting the full endpoint is a common slip; appending again would 404.
  if base.ends_with(CHAT_COMPLETIONS_PATH) {
    return Ok(base.to_string());
  }
  Ok(format!("{base}{CHAT_COMPLETIONS_PATH}"))
}

fn is_loopback(url: &reqwest::Url) -> bool {
  matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
}

// -------------------------------------------------------------------- chat

/// `temperature` is deliberately absent: OpenAI-compatible servers disagree on
/// its default and the agent loop wants the model's own default.
fn chat_body(
  model: &str,
  messages: &[Value],
  tools: Option<&[Value]>,
  disable_thinking: bool,
) -> Value {
  let mut body = json!({ "model": model, "messages": messages });
  if disable_thinking {
    body["thinking"] = json!({ "type": "disabled" });
  }
  // Omitted rather than sent as `[]`: several compatible servers reject an
  // empty tools array outright.
  if let Some(tools) = tools.filter(|tools| !tools.is_empty()) {
    body["tools"] = json!(tools);
  }
  body
}

// ---------------------------------------------------------------- size caps

/// The one cap predicate, shared by both directions so the two limits cannot
/// drift. Pure, so the boundary is testable at the exact byte.
fn ensure_within_cap(label: &str, len: usize, cap: usize) -> Result<(), String> {
  if len > cap {
    return Err(format!(
      "{label} is too large ({len} bytes; the limit is {cap} bytes)"
    ));
  }
  Ok(())
}

/// Serializes one request body and enforces the 1 MiB cap before anything is
/// sent. The returned bytes are exactly what goes on the wire, so the checked
/// size and the sent size cannot disagree.
fn encode_chat_body(body: &Value) -> Result<Vec<u8>, String> {
  let bytes = serde_json::to_vec(body)
    .map_err(|error| format!("cannot serialize the AI request: {error}"))?;
  // Only byte counts reach this message, so there is nothing to redact.
  ensure_within_cap("AI request body", bytes.len(), MAX_REQUEST_BODY_BYTES)?;
  Ok(bytes)
}

/// Appends one streamed chunk under a hard cap. A chunk that would cross the
/// cap is refused *before* it is buffered, so an oversized body costs one
/// chunk of memory instead of the whole body.
fn append_capped(buffer: &mut Vec<u8>, chunk: &[u8], cap: usize) -> Result<(), String> {
  ensure_within_cap(
    "AI response body",
    buffer.len().saturating_add(chunk.len()),
    cap,
  )?;
  buffer.extend_from_slice(chunk);
  Ok(())
}

/// Reads the body chunk by chunk under the cap. `Response::chunk` keeps one
/// chunk in flight, and returning early drops the response — which aborts the
/// connection — so the body is never buffered unbounded.
async fn read_capped_body(mut response: reqwest::Response, cap: usize) -> Result<Vec<u8>, String> {
  let mut buffer: Vec<u8> = Vec::new();
  while let Some(chunk) = response
    .chunk()
    .await
    .map_err(|error| format!("AI response could not be read: {error}"))?
  {
    append_capped(&mut buffer, &chunk, cap)?;
  }
  Ok(buffer)
}

fn build_client() -> Result<reqwest::Client, String> {
  install_crypto_provider();
  reqwest::Client::builder()
    .timeout(CHAT_TIMEOUT)
    .build()
    .map_err(|error| format!("cannot create the HTTP client: {error}"))
}

/// reqwest is compiled with `rustls-no-provider` (the variant already resolved
/// in this lockfile), so a process default must exist before the first
/// `Client` is built or reqwest panics. The updater plugin installs the same
/// ring provider, but only while it wants a client — installing here makes
/// `ai_chat` independent of that timing. Installing twice is harmless.
fn install_crypto_provider() {
  if rustls::crypto::CryptoProvider::get_default().is_none() {
    let _ = rustls::crypto::ring::default_provider().install_default();
  }
}

/// Every error message crosses this: an API key must never appear in a string
/// that reaches the WebView or a log.
fn redact(message: String, key: Option<&str>) -> String {
  match key {
    Some(key) if !key.is_empty() => message.replace(key, "[redacted]"),
    _ => message,
  }
}

fn truncate(text: &str, max_chars: usize) -> String {
  if text.chars().count() <= max_chars {
    return text.to_string();
  }
  let mut out: String = text.chars().take(max_chars).collect();
  out.push('…');
  out
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
  app
    .path()
    .app_config_dir()
    .map(|dir| dir.join(CONFIG_FILE_NAME))
    .map_err(|error| format!("cannot resolve the app config directory: {error}"))
}

/// `-> { baseUrl, model, hasKey }`. The key itself never crosses the IPC
/// boundary.
#[tauri::command]
pub fn ai_get_config(app: AppHandle) -> Result<Value, String> {
  Ok(config_view(&load_config(&config_path(&app)?)))
}

/// `{ baseUrl, model, apiKey? }`. The tri-state `api_key` parameter is what
/// the contract needs: a missing `apiKey` keeps the stored key, `null` clears
/// it, and a string replaces it (trimmed; an empty string clears too, so a
/// cleared field does not store a blank key).
///
/// The URL is validated here with the same guard `ai_chat` applies, so a bad
/// base URL fails where the user is looking at the field.
#[tauri::command]
pub fn ai_set_config(
  app: AppHandle,
  base_url: String,
  model: String,
  api_key: Option<Option<String>>,
  disable_thinking: Option<bool>,
) -> Result<Value, String> {
  let path = config_path(&app)?;
  let base_url = base_url.trim().to_string();
  chat_endpoint(&base_url)?;
  let mut config = load_config(&path);
  config.base_url = base_url;
  config.model = model.trim().to_string();
  if let Some(disable_thinking) = disable_thinking {
    config.disable_thinking = disable_thinking;
  }
  if let Some(api_key) = api_key {
    config.api_key = api_key
      .map(|key| key.trim().to_string())
      .filter(|key| !key.is_empty());
  }
  save_config(&path, &config)?;
  // Same shape as `ai_get_config`: the caller can refresh its settings form
  // without a second round trip, and the key still never comes back.
  Ok(config_view(&config))
}

/// Forwards one non-streaming chat completion and returns the parsed body.
/// The WebView sends the whole conversation (and tool schemas); Rust adds the
/// model, the key, and the endpoint.
#[tauri::command]
pub async fn ai_chat(
  app: AppHandle,
  messages: Vec<Value>,
  tools: Option<Vec<Value>>,
) -> Result<Value, String> {
  let config = load_config(&config_path(&app)?);
  if config.model.trim().is_empty() {
    return Err("AI model is not set".to_string());
  }
  if messages.is_empty() {
    return Err("ai_chat needs at least one message".to_string());
  }
  let endpoint = chat_endpoint(&config.base_url)?;
  let key = config.api_key.filter(|key| !key.is_empty());
  // Serialized once and checked before the key is attached: the 1 MiB cap
  // refuses an oversized transcript without ever building a request.
  let body = encode_chat_body(&chat_body(
    &config.model,
    &messages,
    tools.as_deref(),
    config.disable_thinking,
  ))?;
  let mut request = build_client()?
    .post(&endpoint)
    .header(reqwest::header::CONTENT_TYPE, "application/json")
    .body(body);
  if let Some(key) = key.as_deref() {
    request = request.bearer_auth(key);
  }
  let response = request
    .send()
    .await
    .map_err(|error| redact(format!("AI request failed: {error}"), key.as_deref()))?;
  let status = response.status();
  let bytes = read_capped_body(response, MAX_RESPONSE_BODY_BYTES)
    .await
    .map_err(|error| redact(error, key.as_deref()))?;
  let text = String::from_utf8_lossy(&bytes);
  if !status.is_success() {
    return Err(upstream_error(status, &endpoint, &text, key.as_deref()));
  }
  serde_json::from_slice(&bytes).map_err(|error| {
    redact(
      format!(
        "AI response was not JSON ({error}): {}",
        truncate(&text, MAX_ERROR_BODY_CHARS)
      ),
      key.as_deref(),
    )
  })
}

// ----------------------------------------------------------------- streaming

/// In-flight streamed runs, keyed by the WebView's run id. `ai_chat_stream`
/// registers its reader task; `ai_chat_cancel` aborts it, which drops the
/// upstream `Response` and so aborts the connection. A `std` mutex is enough:
/// every operation is a constant-time map touch, and the run's `Drop` guard
/// (which must run on the sync path) needs a sync lock.
static STREAM_RUNS: LazyLock<Mutex<HashMap<String, Box<dyn RunAbort>>>> =
  LazyLock::new(|| Mutex::new(HashMap::new()));

fn stream_runs() -> MutexGuard<'static, HashMap<String, Box<dyn RunAbort>>> {
  // A poisoned lock still holds a valid map; a cancelled reader must never
  // wedge the registry for every later run.
  STREAM_RUNS
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One thing `ai_chat_cancel` can do to an in-flight run. The trait is the seam
/// the registry tests use without spawning a task or touching the network.
trait RunAbort: Send + Sync {
  fn abort_run(&self);
}

impl RunAbort for tauri::async_runtime::JoinHandle<()> {
  fn abort_run(&self) {
    self.abort();
  }
}

/// Removes (and aborts) a run when the command returns for any reason —
/// success, error and cancel all drop this guard.
struct RunGuard(String);

impl Drop for RunGuard {
  fn drop(&mut self) {
    if let Some(abort) = stream_runs().remove(&self.0) {
      abort.abort_run();
    }
  }
}

/// Claims a run id. A duplicate id that is still in flight is refused, and the
/// caller's freshly spawned reader is aborted rather than leaked.
fn register_run(run_id: &str, abort: Box<dyn RunAbort>) -> Result<(), String> {
  let mut runs = stream_runs();
  if runs.contains_key(run_id) {
    abort.abort_run();
    return Err(format!(
      "an AI stream is already running for run id {run_id}"
    ));
  }
  runs.insert(run_id.to_string(), abort);
  Ok(())
}

/// Fires the cancellation for one run. Returns whether a run was in flight; the
/// run itself then resolves with `Err("cancelled")` and removes its own entry.
fn cancel_run(run_id: &str) -> bool {
  match stream_runs().get(run_id) {
    Some(abort) => {
      abort.abort_run();
      true
    }
    None => false,
  }
}

/// One event from the reader task to the command.
enum StreamPart {
  /// Raw upstream body bytes, forwarded verbatim. A chunk may split a UTF-8
  /// character; the command's decoder carries the tail into the next chunk.
  Chunk(Vec<u8>),
  /// The upstream body ended cleanly.
  End,
  /// The stream failed; the message is already redacted.
  Failed(String),
}

/// The streaming variant of `chat_body`: byte-for-byte the same object plus
/// `"stream": true`. The flag is added here so the non-streaming `ai_chat`
/// output is untouched and every streaming caller requests the same body.
fn stream_chat_body(
  model: &str,
  messages: &[Value],
  tools: Option<&[Value]>,
  disable_thinking: bool,
) -> Value {
  let mut body = chat_body(model, messages, tools, disable_thinking);
  body["stream"] = json!(true);
  body
}

/// The one non-2xx message shape, shared by `ai_chat` and `ai_chat_stream` so
/// the two cannot drift: the status, the endpoint, and 300 chars of the body,
/// with the key stripped.
fn upstream_error(
  status: reqwest::StatusCode,
  endpoint: &str,
  text: &str,
  key: Option<&str>,
) -> String {
  redact(
    format!(
      "AI request failed with {status} at {endpoint}: {}",
      truncate(text, MAX_ERROR_BODY_CHARS)
    ),
    key,
  )
}

/// The reader half of a streamed completion. It never parses SSE and never
/// buffers the body; the only accounting is the 4 MiB forwarded-byte cap. Every
/// outcome — clean end, upstream error, cap breach — is reported to the command
/// as a `StreamPart`, and a dropped receiver stops it (dropping the response
/// aborts the connection).
async fn pump_stream(
  request: reqwest::RequestBuilder,
  endpoint: String,
  key: Option<String>,
  tx: tauri::async_runtime::Sender<StreamPart>,
) {
  let response = match request.send().await {
    Ok(response) => response,
    Err(error) => {
      let _ = tx
        .send(StreamPart::Failed(redact(
          format!("AI request failed: {error}"),
          key.as_deref(),
        )))
        .await;
      return;
    }
  };
  let status = response.status();
  if !status.is_success() {
    // Non-2xx is not a stream: clip it exactly like `ai_chat` and send no
    // chunks.
    let message = match read_capped_body(response, MAX_RESPONSE_BODY_BYTES).await {
      Ok(bytes) => upstream_error(
        status,
        &endpoint,
        &String::from_utf8_lossy(&bytes),
        key.as_deref(),
      ),
      Err(error) => redact(error, key.as_deref()),
    };
    let _ = tx.send(StreamPart::Failed(message)).await;
    return;
  }
  let mut response = response;
  let mut forwarded = 0usize;
  loop {
    match response.chunk().await {
      Ok(Some(chunk)) => {
        if let Err(error) = ensure_within_cap(
          "AI response body",
          forwarded.saturating_add(chunk.len()),
          MAX_STREAM_RESPONSE_BODY_BYTES,
        ) {
          let _ = tx.send(StreamPart::Failed(error)).await;
          return;
        }
        forwarded += chunk.len();
        if tx.send(StreamPart::Chunk(chunk.to_vec())).await.is_err() {
          // The command is gone; dropping the response aborts the connection.
          return;
        }
      }
      Ok(None) => {
        let _ = tx.send(StreamPart::End).await;
        return;
      }
      Err(error) => {
        let message = redact(
          format!("AI response could not be read: {error}"),
          key.as_deref(),
        );
        let _ = tx.send(StreamPart::Failed(message)).await;
        return;
      }
    }
  }
}

/// Carries a UTF-8 character that was split across two body chunks into the
/// next decode: the tail bytes are deferred, never lossy-replaced, so the
/// caller only ever sees whole characters. A genuinely invalid byte sequence is
/// replaced at once instead of stalling the stream.
struct ChunkDecoder {
  pending: Vec<u8>,
}

impl ChunkDecoder {
  fn new() -> Self {
    Self {
      pending: Vec::new(),
    }
  }

  /// Decodes one body chunk, returning the text that is safe to forward now.
  fn push(&mut self, bytes: &[u8]) -> String {
    if self.pending.is_empty() {
      // Fast path: a self-contained chunk, the common case.
      if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
      }
    }
    self.pending.extend_from_slice(bytes);
    match std::str::from_utf8(&self.pending) {
      Ok(text) => {
        let out = text.to_string();
        self.pending.clear();
        out
      }
      Err(error) => {
        let valid = error.valid_up_to();
        let out = String::from_utf8_lossy(&self.pending[..valid]).into_owned();
        match error.error_len() {
          // The tail is a truncated character: keep it for the next chunk.
          None => {
            self.pending.drain(..valid);
            out
          }
          // The tail is a real error: replace it now, never defer it.
          Some(_) => {
            let tail = String::from_utf8_lossy(&self.pending[valid..]).into_owned();
            self.pending.clear();
            out + &tail
          }
        }
      }
    }
  }

  /// Flushes a trailing truncated character (lossy) when the body ends.
  fn finish(&mut self) -> Option<String> {
    if self.pending.is_empty() {
      return None;
    }
    let text = String::from_utf8_lossy(&self.pending).into_owned();
    self.pending.clear();
    Some(text)
  }
}

/// Forwards one streamed chat completion. Every upstream body chunk crosses
/// `on_chunk` as a UTF-8 string and the WebView assembles deltas and tool calls
/// (`@space-lens/ai-advisor`'s stream parser). The body is capped at 1 MiB and
/// the forwarded bytes at 4 MiB; a non-2xx upstream produces the same redacted
/// error as `ai_chat` and no chunks. Cancelling the run id aborts the upstream
/// request and resolves with `Err("cancelled")`.
#[tauri::command]
pub async fn ai_chat_stream(
  app: AppHandle,
  messages: Vec<Value>,
  tools: Option<Vec<Value>>,
  run_id: String,
  on_chunk: tauri::ipc::Channel<String>,
) -> Result<(), String> {
  let config = load_config(&config_path(&app)?);
  if config.model.trim().is_empty() {
    return Err("AI model is not set".to_string());
  }
  if messages.is_empty() {
    return Err("ai_chat_stream needs at least one message".to_string());
  }
  if run_id.trim().is_empty() {
    return Err("ai_chat_stream needs a run id".to_string());
  }
  let endpoint = chat_endpoint(&config.base_url)?;
  let key = config.api_key.filter(|key| !key.is_empty());
  // Serialized once and checked before the key is attached, exactly like
  // `ai_chat`: the 1 MiB cap refuses an oversized transcript before a request.
  let body = encode_chat_body(&stream_chat_body(
    &config.model,
    &messages,
    tools.as_deref(),
    config.disable_thinking,
  ))?;
  let mut request = build_client()?
    .post(&endpoint)
    .header(reqwest::header::CONTENT_TYPE, "application/json")
    .body(body);
  if let Some(key) = key.as_deref() {
    request = request.bearer_auth(key);
  }

  let (tx, mut rx) = tauri::async_runtime::channel::<StreamPart>(STREAM_CHANNEL_CAPACITY);
  let reader = tauri::async_runtime::spawn(pump_stream(request, endpoint, key, tx));
  // Rejects a duplicate in-flight id; on refusal the freshly spawned reader is
  // aborted inside `register_run` rather than leaked.
  register_run(&run_id, Box::new(reader))?;
  // Removes (and aborts) the run on every exit path below.
  let _guard = RunGuard(run_id.clone());

  let mut decoder = ChunkDecoder::new();
  loop {
    match rx.recv().await {
      Some(StreamPart::Chunk(bytes)) => {
        let text = decoder.push(&bytes);
        if !text.is_empty() {
          on_chunk
            .send(text)
            .map_err(|error| format!("AI stream chunk could not be delivered: {error}"))?;
        }
      }
      Some(StreamPart::End) => {
        if let Some(text) = decoder.finish() {
          on_chunk
            .send(text)
            .map_err(|error| format!("AI stream chunk could not be delivered: {error}"))?;
        }
        return Ok(());
      }
      Some(StreamPart::Failed(message)) => return Err(message),
      // The reader was aborted (cancel) or dropped without an `End`.
      None => return Err("cancelled".to_string()),
    }
  }
}

/// Cancels an in-flight `ai_chat_stream` by its run id. Returns whether a run
/// was found; that run then resolves with `Err("cancelled")`.
#[tauri::command]
pub fn ai_chat_cancel(run_id: String) -> bool {
  cancel_run(&run_id)
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
  use std::sync::Arc;

  static TEMP_SEQ: AtomicU32 = AtomicU32::new(0);

  /// A fresh directory per test — no temp-file crate, and the caller removes
  /// it so repeated runs stay clean.
  fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
      "sl-ai-{label}-{}-{}",
      std::process::id(),
      TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
  }

  #[test]
  fn a_pasted_full_endpoint_is_not_suffixed_twice() {
    assert_eq!(
      chat_endpoint("https://api.example.com/v1/chat/completions/").unwrap(),
      "https://api.example.com/v1/chat/completions"
    );
  }

  #[test]
  fn https_base_urls_build_one_chat_completions_endpoint() {
    assert_eq!(
      chat_endpoint("https://api.openai.com/v1").unwrap(),
      "https://api.openai.com/v1/chat/completions"
    );
    // Trailing slashes and surrounding whitespace never double up the path.
    assert_eq!(
      chat_endpoint("  https://api.example.com/v1//  ").unwrap(),
      "https://api.example.com/v1/chat/completions"
    );
    assert_eq!(
      chat_endpoint("https://api.example.com").unwrap(),
      "https://api.example.com/chat/completions"
    );
  }

  #[test]
  fn plaintext_http_is_allowed_only_for_loopback() {
    for base in [
      "http://localhost:11434/v1",
      "http://127.0.0.1:8080/v1",
      "http://[::1]:8080/v1",
    ] {
      assert!(chat_endpoint(base).is_ok(), "{base} should be accepted");
    }
    assert_eq!(
      chat_endpoint("http://[::1]:8080/v1").unwrap(),
      "http://[::1]:8080/v1/chat/completions"
    );
  }

  #[test]
  fn remote_http_userinfo_and_junk_are_rejected() {
    assert_eq!(
      chat_endpoint("http://api.example.com/v1").unwrap_err(),
      "AI base URL must use https unless the host is localhost"
    );
    // Localhost-ish names that are not loopback hosts must not pass.
    assert!(chat_endpoint("http://localhost.example.com/v1").is_err());
    assert!(chat_endpoint("https://user@example.com/v1").is_err());
    assert!(chat_endpoint("https://user:secret@example.com/v1").is_err());
    assert!(chat_endpoint("file:///etc/passwd").is_err());
    assert!(chat_endpoint("api.openai.com/v1").is_err());
    assert!(chat_endpoint("").is_err());
    assert!(chat_endpoint("   ").is_err());
  }

  #[test]
  fn config_round_trips_through_the_file_and_defaults_when_absent() {
    let dir = temp_dir("round-trip");
    let path = dir.join(CONFIG_FILE_NAME);

    // No file yet: defaults, not an error.
    let missing = load_config(&path);
    assert_eq!(missing, StoredConfig::default());
    assert_eq!(missing.base_url, "https://api.openai.com/v1");
    assert_eq!(missing.model, "");
    assert!(!missing.has_key());

    let config = StoredConfig {
      base_url: "https://api.example.com/v1".to_string(),
      model: "glm-4.6".to_string(),
      api_key: Some("sk-secret".to_string()),
      disable_thinking: true,
    };
    save_config(&path, &config).unwrap();
    assert_eq!(load_config(&path), config);
    assert!(load_config(&path).has_key());

    // The key is stored (the file is the only place it lives) and no temp file
    // survives the rename.
    let raw = fs::read_to_string(&path).unwrap();
    assert!(raw.contains("sk-secret"));
    assert!(!dir.join(format!("{CONFIG_FILE_NAME}.tmp")).exists());

    // A cleared key and a hand-edited partial file both keep working.
    save_config(&path, &StoredConfig::default()).unwrap();
    assert!(!load_config(&path).has_key());
    fs::write(&path, "{}").unwrap();
    assert_eq!(load_config(&path), StoredConfig::default());
    fs::write(&path, "{ not json").unwrap();
    assert_eq!(load_config(&path), StoredConfig::default());

    fs::remove_dir_all(dir).unwrap();
  }

  #[cfg(unix)]
  #[test]
  fn saved_config_is_owner_only() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = temp_dir("mode");
    let path = dir.join(CONFIG_FILE_NAME);
    save_config(&path, &StoredConfig::default()).unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "the key file must stay owner-readable only");
    // Overwriting an existing file also lands 0600.
    save_config(&path, &StoredConfig::default()).unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    fs::remove_dir_all(dir).unwrap();
  }

  #[test]
  fn get_config_view_never_carries_the_key() {
    let config = StoredConfig {
      base_url: "https://api.example.com/v1".to_string(),
      model: "glm-4.6".to_string(),
      api_key: Some("sk-secret-value".to_string()),
      disable_thinking: false,
    };
    let view = config_view(&config);
    assert_eq!(
      view,
      json!({
        "baseUrl": "https://api.example.com/v1",
        "model": "glm-4.6",
        "hasKey": true,
        "disableThinking": false,
      })
    );
    assert!(view.get("apiKey").is_none());
    assert!(!view.to_string().contains("sk-secret-value"));
  }

  #[test]
  fn chat_body_disables_thinking_only_when_asked() {
    let messages = vec![json!({ "role": "user", "content": "hi" })];
    assert!(chat_body("m", &messages, None, false)
      .get("thinking")
      .is_none());
    assert_eq!(
      chat_body("m", &messages, None, true)["thinking"],
      json!({ "type": "disabled" })
    );
  }

  #[test]
  fn chat_body_carries_tools_only_when_non_empty() {
    let messages = vec![json!({ "role": "user", "content": "hi" })];
    let without = chat_body("glm-4.6", &messages, None, false);
    assert_eq!(without["model"], "glm-4.6");
    assert_eq!(without["messages"], json!(messages));
    assert!(without.get("tools").is_none());
    // An empty list is not a tools request.
    assert!(chat_body("m", &messages, Some(&[]), false)
      .get("tools")
      .is_none());
    let tools = vec![json!({ "type": "function" })];
    assert_eq!(
      chat_body("m", &messages, Some(&tools), false)["tools"],
      json!(tools)
    );
  }

  #[test]
  fn the_chat_client_builds_with_the_installed_crypto_provider() {
    // reqwest panics at construction when rustls has no process provider, so
    // this locks `install_crypto_provider` without touching the network.
    assert!(build_client().is_ok());
  }

  #[test]
  fn the_two_caps_are_the_ones_the_plan_sets() {
    // "Context and memory budget" item 5: 1 MiB up, 2 MiB down.
    assert_eq!(MAX_REQUEST_BODY_BYTES, 1024 * 1024);
    assert_eq!(MAX_RESPONSE_BODY_BYTES, 2 * 1024 * 1024);
  }

  #[test]
  fn request_body_cap_is_enforced_at_the_exact_byte() {
    // The boundary itself passes; one byte more does not.
    assert!(ensure_within_cap(
      "AI request body",
      MAX_REQUEST_BODY_BYTES,
      MAX_REQUEST_BODY_BYTES
    )
    .is_ok());
    let error = ensure_within_cap(
      "AI request body",
      MAX_REQUEST_BODY_BYTES + 1,
      MAX_REQUEST_BODY_BYTES,
    )
    .unwrap_err();
    assert!(error.contains("1048576"), "{error}");

    // A body under the cap serializes to exactly the bytes that would be sent.
    let small = json!({ "model": "m", "messages": [{ "role": "user", "content": "hi" }] });
    let bytes = encode_chat_body(&small).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), small);
    assert!(bytes.len() <= MAX_REQUEST_BODY_BYTES);

    // A body over the cap is refused before any request can be built.
    let huge = json!({
      "model": "m",
      "messages": [{ "role": "user", "content": "x".repeat(MAX_REQUEST_BODY_BYTES) }],
    });
    let error = encode_chat_body(&huge).unwrap_err();
    assert!(error.contains("too large"), "{error}");
    assert!(
      !error.contains(&"x".repeat(64)),
      "the body must not be echoed"
    );
  }

  #[test]
  fn response_body_cap_refuses_the_chunk_that_would_cross_it() {
    let cap = 8;
    let mut buffer = Vec::new();
    append_capped(&mut buffer, b"12345", cap).unwrap();
    // Exactly at the cap still fits.
    append_capped(&mut buffer, b"678", cap).unwrap();
    assert_eq!(buffer, b"12345678");
    // One byte over is refused, and none of the offending chunk is buffered.
    let error = append_capped(&mut buffer, b"9", cap).unwrap_err();
    assert!(error.contains("too large"), "{error}");
    assert_eq!(buffer, b"12345678");
    // A single chunk larger than the whole cap never lands in memory.
    let mut fresh = Vec::new();
    let oversized = vec![b'x'; cap + 1];
    assert!(append_capped(&mut fresh, &oversized, cap).is_err());
    assert!(fresh.is_empty());
  }

  /// The streaming read is the one cap that cannot be proven by a pure helper
  /// alone, so this drives it through a real socket: a one-shot local server
  /// answers with 4 MiB against the 2 MiB cap. The endpoint guard already
  /// allows plaintext `http` for loopback, so the rule that makes a local model
  /// server usable also makes this test possible.
  #[test]
  fn a_streamed_response_past_the_cap_is_aborted_instead_of_buffered() {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
      let Ok((mut socket, _)) = listener.accept() else {
        return;
      };
      let mut request = [0u8; 1024];
      let _ = socket.read(&mut request);
      let body = vec![b'x'; 4 * 1024 * 1024];
      // Write errors are expected once the client aborts, so they are ignored:
      // this thread only exists to feed the cap.
      let _ = write!(
        socket,
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n",
        body.len()
      );
      let _ = socket.write_all(&body);
    });

    let url = format!("http://127.0.0.1:{port}/chat/completions");
    let outcome = tauri::async_runtime::block_on(async move {
      let response = build_client()
        .unwrap()
        .post(&url)
        .body("{}")
        .send()
        .await
        .unwrap();
      read_capped_body(response, MAX_RESPONSE_BODY_BYTES).await
    });

    let error = outcome.unwrap_err();
    assert!(error.contains("too large"), "{error}");
    let _ = server.join();
  }

  #[test]
  fn error_text_strips_the_key_and_clips_a_long_body() {
    assert_eq!(
      redact("rejected key sk-secret".to_string(), Some("sk-secret")),
      "rejected key [redacted]"
    );
    assert_eq!(redact("plain".to_string(), None), "plain");
    assert_eq!(redact("plain".to_string(), Some("")), "plain");

    let long = "x".repeat(MAX_ERROR_BODY_CHARS + 50);
    let clipped = truncate(&long, MAX_ERROR_BODY_CHARS);
    assert_eq!(clipped.chars().count(), MAX_ERROR_BODY_CHARS + 1);
    assert_eq!(truncate("short", MAX_ERROR_BODY_CHARS), "short");
  }

  // ------------------------------------------------------------- streaming

  #[test]
  fn stream_body_adds_the_stream_flag_and_keeps_thinking_and_tools() {
    let messages = vec![json!({ "role": "user", "content": "hi" })];
    let streaming = stream_chat_body("glm-4.6", &messages, None, false);
    assert_eq!(streaming["stream"], json!(true));
    assert_eq!(streaming["model"], "glm-4.6");
    assert_eq!(streaming["messages"], json!(messages));

    // `thinking` and `tools` behave exactly as the non-streaming builder's.
    assert_eq!(
      stream_chat_body("m", &messages, None, true)["thinking"],
      json!({ "type": "disabled" })
    );
    assert!(stream_chat_body("m", &messages, None, false)
      .get("thinking")
      .is_none());
    assert!(stream_chat_body("m", &messages, Some(&[]), false)
      .get("tools")
      .is_none());
    let tools = vec![json!({ "type": "function" })];
    assert_eq!(
      stream_chat_body("m", &messages, Some(&tools), false)["tools"],
      json!(tools)
    );

    // The non-streaming body is untouched: no `stream` key.
    assert!(chat_body("m", &messages, None, false)
      .get("stream")
      .is_none());
  }

  #[test]
  fn stream_cap_is_the_four_mebibytes_the_design_sets() {
    assert_eq!(MAX_STREAM_RESPONSE_BODY_BYTES, 4 * 1024 * 1024);
  }

  #[test]
  fn chunk_decoder_carries_a_split_multibyte_character() {
    let mut decoder = ChunkDecoder::new();
    // "🙂" is F0 9F 99 82; split it after the first byte.
    assert_eq!(
      decoder.push(&[b'a', 0xF0]),
      "a",
      "the incomplete character must be deferred, not replaced"
    );
    assert_eq!(decoder.push(&[0x9F, 0x99, 0x82, b'b']), "🙂b");
    assert_eq!(decoder.finish(), None);

    // A 3-byte character split at every byte still reassembles exactly.
    let text = "日本語";
    let mut decoder = ChunkDecoder::new();
    let mut out = String::new();
    for byte in text.as_bytes() {
      out.push_str(&decoder.push(std::slice::from_ref(byte)));
    }
    out.push_str(&decoder.finish().unwrap_or_default());
    assert_eq!(out, text);
  }

  #[test]
  fn chunk_decoder_replaces_invalid_bytes_without_stalling() {
    // A lone 0xFF is invalid, not truncated: replace it now.
    let mut decoder = ChunkDecoder::new();
    assert_eq!(decoder.push(&[b'a', 0xFF, b'b']), "a\u{fffd}b");
    assert_eq!(decoder.finish(), None);

    // An invalid lead byte does not swallow the valid character after it.
    let mut decoder = ChunkDecoder::new();
    assert_eq!(decoder.push(&[0xF0, 0x41]), "\u{fffd}A");

    // A truncated tail at end-of-body is flushed lossily, exactly once.
    let mut decoder = ChunkDecoder::new();
    assert_eq!(decoder.push(&[0xF0, 0x9F]), "");
    assert_eq!(decoder.finish(), Some("\u{fffd}".to_string()));
    assert_eq!(decoder.finish(), None);
  }

  /// A registry probe: records the abort without spawning anything.
  struct ProbeAbort(Arc<AtomicBool>);

  impl RunAbort for ProbeAbort {
    fn abort_run(&self) {
      self.0.store(true, Ordering::SeqCst);
    }
  }

  #[test]
  fn stream_registry_registers_rejects_duplicates_and_cancels() {
    let run_id = format!("run-{}-registry", std::process::id());
    let aborted = Arc::new(AtomicBool::new(false));
    register_run(&run_id, Box::new(ProbeAbort(aborted.clone()))).unwrap();
    assert!(
      !aborted.load(Ordering::SeqCst),
      "registration is not a cancel"
    );

    // A duplicate in-flight id is refused, and the caller's handle is aborted
    // rather than leaked.
    let duplicate = Arc::new(AtomicBool::new(false));
    let error = register_run(&run_id, Box::new(ProbeAbort(duplicate.clone()))).unwrap_err();
    assert!(error.contains("already running"), "{error}");
    assert!(duplicate.load(Ordering::SeqCst));

    // Cancel fires the registered run's abort.
    assert!(cancel_run(&run_id));
    assert!(aborted.load(Ordering::SeqCst));

    // The entry survives cancel until the run's guard removes it.
    drop(RunGuard(run_id.clone()));
    assert!(
      !cancel_run(&run_id),
      "a removed run is no longer cancellable"
    );
  }

  #[test]
  fn stream_registry_guard_cleans_up_the_entry() {
    let run_id = format!("run-{}-cleanup", std::process::id());
    register_run(
      &run_id,
      Box::new(ProbeAbort(Arc::new(AtomicBool::new(false)))),
    )
    .unwrap();
    assert!(cancel_run(&run_id));
    {
      let _guard = RunGuard(run_id.clone());
    }
    // Dropping the guard removed the entry, so the id is free and a later
    // cancel is a no-op.
    assert!(!cancel_run(&run_id));
    // The id can be registered again.
    register_run(
      &run_id,
      Box::new(ProbeAbort(Arc::new(AtomicBool::new(false)))),
    )
    .unwrap();
    drop(RunGuard(run_id.clone()));
  }
}
