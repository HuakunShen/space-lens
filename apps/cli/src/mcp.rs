use serde_json::{json, Map, Value};
use space_lens::cloud::{
  build_eviction_plan, NativeICloudBackend, ScanOptions as CloudScanOptions,
};
use space_lens::{
  build_removal_plan, execute_removal_plan, find_candidates, scan_directory, CandidateOptions,
  CleanupPreset, IgnoredMode, PlatformCapabilities, RemovalEntry, RemovalPlan, ScanNode,
  ScanOptions, SnapshotEnvelope,
};
use spacelens_app_audit::{audit_installed_apps, AppAuditOptions};
use spacelens_discovery::{run_discovery, DiscoveryKind, DiscoveryOptions};
use spacelens_remote::runtime::RemoteHost;
use spacelens_remote::RemotePolicy;
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use crate::remote_cmd;

const PROTOCOL_VERSION: &str = "2025-11-25";

/// Set once by `spacelens mcp --allow-cleanup`; read on every tool call.
static ALLOW_CLEANUP: AtomicBool = AtomicBool::new(false);

/// Live SSH sessions, keyed by destination spec. One connection per device
/// outlives every tool call; all are stopped when stdin closes.
static REMOTES: OnceLock<Mutex<HashMap<String, Arc<RemoteHost>>>> = OnceLock::new();

/// Local background scans, keyed by scan id. The scan runs in a thread; the
/// tools only ever read cheap state or one paginated level of the tree.
static LOCAL_SCANS: OnceLock<Mutex<HashMap<String, LocalScan>>> = OnceLock::new();

static NEXT_SCAN: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
struct LocalScan {
  state: Arc<Mutex<LocalScanState>>,
  started: Instant,
}

enum LocalScanState {
  Scanning,
  Ready(Vec<ScanNode>),
}

pub fn run(allow_cleanup: bool) -> anyhow::Result<()> {
  ALLOW_CLEANUP.store(allow_cleanup, Ordering::SeqCst);
  let stdin = io::stdin();
  let mut stdout = io::BufWriter::new(io::stdout().lock());

  for line in stdin.lock().lines() {
    let line = line?;
    if line.trim().is_empty() {
      continue;
    }

    let response = match serde_json::from_str::<Value>(&line) {
      Ok(request) => handle_request(request),
      Err(error) => Some(error_response(
        Value::Null,
        -32700,
        format!("invalid JSON: {error}"),
      )),
    };

    if let Some(response) = response {
      serde_json::to_writer(&mut stdout, &response)?;
      stdout.write_all(b"\n")?;
      stdout.flush()?;
    }
  }

  shutdown_remotes();
  Ok(())
}

/// Stops every live SSH session so remote serve processes and their pid files
/// never outlive the MCP adapter.
fn shutdown_remotes() {
  if let Some(remotes) = REMOTES.get() {
    let sessions = remotes.lock().unwrap().drain().collect::<Vec<_>>();
    for (_, remote) in sessions {
      remote.stop();
    }
  }
}

pub fn handle_request(request: Value) -> Option<Value> {
  let id = request.get("id").cloned();
  let method = request.get("method").and_then(Value::as_str);
  let Some(method) = method else {
    return id.map(|id| error_response(id, -32600, "request must contain a method"));
  };

  // MCP notifications, including notifications/initialized, have no reply.
  let id = id?;

  match method {
    "initialize" => Some(success_response(id, initialize_result())),
    "ping" => Some(success_response(id, json!({}))),
    "tools/list" => Some(success_response(id, json!({ "tools": tool_definitions() }))),
    "tools/call" => Some(success_response(id, call_tool(request.get("params")))),
    _ => Some(error_response(
      id,
      -32601,
      format!("method not found: {method}"),
    )),
  }
}

fn initialize_result() -> Value {
  let cleanup_allowed = ALLOW_CLEANUP.load(Ordering::SeqCst);
  json!({
    "protocolVersion": PROTOCOL_VERSION,
    "capabilities": { "tools": { "listChanged": false } },
    "serverInfo": {
      "name": "space-lens",
      "version": env!("CARGO_PKG_VERSION")
    },
    "instructions": if cleanup_allowed {
      "Space Lens MCP inspects filesystems on this machine and on SSH hosts. Pass `device` as an ~/.ssh/config alias or `user@host[:port]` to target a remote machine; omit it (or use \"local\") for this machine. Long scans never block a tool call: space_lens_scan returns a scanId immediately, space_lens_scan_status polls progress, space_lens_ls lists one folder level sorted by size. Cleanup tools are live: remote hosts move planned nodes to their trash (never permanent deletion); local cleanup removes the listed paths. Process control and iCloud eviction are never available through MCP."
    } else {
      "Space Lens MCP is read-only. It inspects filesystems on this machine and on SSH hosts (pass `device` as an ~/.ssh/config alias or `user@host[:port]`; omit it for this machine). Long scans never block a tool call: space_lens_scan returns a scanId immediately, space_lens_scan_status polls progress, space_lens_ls lists one folder level sorted by size. It can report cleanup candidates and plans, but it never deletes files, moves items to Trash, requests iCloud downloads, or evicts iCloud local copies. Start with `spacelens mcp --allow-cleanup` to unlock the cleanup tools."
    }
  })
}

fn tool_definitions() -> Vec<Value> {
  vec![
    json!({
      "name": "space_lens_devices",
      "description": "List the machines this adapter can reach: this machine plus the concrete hosts in ~/.ssh/config. The `device` value every other tool accepts is the `device` field here.",
      "inputSchema": { "type": "object", "additionalProperties": false }
    }),
    json!({
      "name": "space_lens_scan",
      "description": "Start a scan and return immediately with a scanId. The scan runs in the background (in this process for local, on the remote host for SSH devices); poll space_lens_scan_status until state is \"ready\", then navigate with space_lens_ls. This call never blocks for the scan itself.",
      "inputSchema": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "path": { "type": "string", "description": "Folder to scan, in the target machine's own path format." },
          "device": { "type": "string", "description": "An ~/.ssh/config alias or user@host[:port]; omit for this machine." }
        },
        "required": ["path"]
      }
    }),
    json!({
      "name": "space_lens_scan_status",
      "description": "Poll one scan: state (scanning/ready/failed/cancelled), progress, and the bytes/entries counted so far. Cheap by design; call it repeatedly.",
      "inputSchema": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "scanId": { "type": "string" },
          "device": { "type": "string", "description": "The device the scan runs on; omit for this machine." }
        },
        "required": ["scanId"]
      }
    }),
    json!({
      "name": "space_lens_ls",
      "description": "List one folder level of a finished scan, largest first — the equivalent of `ls` where every row carries the subtree size. Give either a node id (remote scans) or a folder path (local and remote); omit both to list the scan root. Paginated: one call never returns a whole disk.",
      "inputSchema": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "scanId": { "type": "string" },
          "path": { "type": "string", "description": "Folder path as reported by earlier ls rows." },
          "nodeId": { "type": "string", "description": "Node id from a previous ls row (remote scans)." },
          "sort": { "type": "string", "enum": ["size", "name", "path"], "default": "size" },
          "limit": { "type": "integer", "minimum": 1, "maximum": 1000, "default": 50 },
          "offset": { "type": "integer", "minimum": 0, "default": 0 },
          "device": { "type": "string", "description": "The device the scan runs on; omit for this machine." }
        },
        "required": ["scanId"]
      }
    }),
    json!({
      "name": "space_lens_cleanup_execute",
      "description": "Remove the listed paths on this machine (local only). Requires the adapter to run with --allow-cleanup; without it every call is refused. For remote machines use space_lens_remote_cleanup_plan and space_lens_remote_cleanup_execute, which trash instead of delete.",
      "inputSchema": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "paths": { "type": "array", "items": { "type": "string", "minLength": 1 }, "minItems": 1 }
        },
        "required": ["paths"]
      }
    }),
    json!({
      "name": "space_lens_remote_cleanup_plan",
      "description": "Plan a trash-only cleanup for remote scan nodes (from space_lens_ls or space_lens_discovery rows). Requires the adapter to run with --allow-cleanup. Returns a planId for space_lens_remote_cleanup_execute; nothing is moved yet.",
      "inputSchema": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "device": { "type": "string", "description": "An ~/.ssh/config alias or user@host[:port]." },
          "scanId": { "type": "string" },
          "nodeIds": { "type": "array", "items": { "type": "string", "minLength": 1 }, "minItems": 1 }
        },
        "required": ["device", "scanId", "nodeIds"]
      }
    }),
    json!({
      "name": "space_lens_remote_cleanup_execute",
      "description": "Move a planned cleanup into the remote machine's trash. Requires the adapter to run with --allow-cleanup; the remote host refuses permanent deletion by construction.",
      "inputSchema": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "device": { "type": "string" },
          "planId": { "type": "string" }
        },
        "required": ["device", "planId"]
      }
    }),
    json!({
      "name": "space_lens_scan_snapshot",
      "description": "Scan one explicitly supplied folder and return a portable flat filesystem snapshot. The scan does not modify files or request cloud downloads.",
      "inputSchema": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "path": { "type": "string", "description": "Existing folder path to inspect." },
          "ignoreHidden": { "type": "boolean", "default": false },
          "respectGitignore": { "type": "boolean", "default": true },
          "ignoredMode": { "type": "string", "enum": ["exclude", "summarize"], "default": "summarize" }
        },
        "required": ["path"]
      }
    }),
    json!({
      "name": "space_lens_cleanup_candidates",
      "description": "Find known cleanup candidates such as node_modules, Cargo target, and gitignored paths. This is a dry-run query and never removes anything.",
      "inputSchema": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "path": { "type": "string", "description": "Existing folder path to inspect." },
          "presets": { "type": "array", "items": { "type": "string", "enum": ["node", "rust", "gitignored"] } },
          "ignoreHidden": { "type": "boolean", "default": false }
        },
        "required": ["path"]
      }
    }),
    json!({
      "name": "space_lens_discovery",
      "description": "Discover large files, disposable developer caches (node_modules, Cargo target, __pycache__, and similar), and gitignored paths under one explicitly supplied folder. Read-only: it never removes, moves, or trashes anything.",
      "inputSchema": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "path": { "type": "string", "description": "Existing folder path to inspect." },
          "kinds": {
            "type": "array",
            "items": { "type": "string", "enum": ["large-files", "caches", "gitignored"] },
            "description": "Views to return; defaults to all three."
          },
          "minSize": { "type": "integer", "default": 0, "description": "Drop items smaller than this many bytes from every view." },
          "limit": { "type": "integer", "default": 200, "description": "Maximum items per view, largest first." },
          "ignoreHidden": { "type": "boolean", "default": false }
        },
        "required": ["path"]
      }
    }),
    json!({
      "name": "space_lens_icloud_plan",
      "description": "Inspect an iCloud item and return a read-only eviction plan. It never downloads, evicts, or deletes anything.",
      "inputSchema": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "path": { "type": "string", "description": "Existing iCloud Drive file or folder path." }
        },
        "required": ["path"]
      }
    }),
    json!({
      "name": "space_lens_app_audit",
      "description": "Audit how much disk space installed applications and known CLI tools keep in scattered folders (Application Support, Caches, dot-directories). Returns per-app totals and per-location classifications (cache, versions, logs, re-downloadable, user-data) with explanations. Read-only: it never removes, moves, or trashes anything.",
      "inputSchema": {
        "type": "object", "additionalProperties": false,
        "properties": {
          "minSize": { "type": "integer", "minimum": 0, "default": 0, "description": "Drop locations smaller than this many bytes." },
          "includeHeuristic": { "type": "boolean", "default": true, "description": "Include apps without curated catalog entries." },
          "limit": { "type": "integer", "minimum": 1, "maximum": 1000, "default": 500, "description": "Maximum apps returned, largest total first." }
        }
      }
    }),
    json!({
      "name": "space_lens_capabilities",
      "description": "Return the platform capability matrix without scanning or modifying files.",
      "inputSchema": { "type": "object", "additionalProperties": false }
    }),
  ]
}

fn call_tool(params: Option<&Value>) -> Value {
  let Some(params) = params else {
    return tool_error("tools/call requires params");
  };
  let Some(name) = params.get("name").and_then(Value::as_str) else {
    return tool_error("tools/call requires params.name");
  };
  let arguments = params
    .get("arguments")
    .and_then(Value::as_object)
    .cloned()
    .unwrap_or_default();

  match name {
    "space_lens_devices" => devices(),
    "space_lens_scan" => device_scan(&arguments),
    "space_lens_scan_status" => device_scan_status(&arguments),
    "space_lens_ls" => device_ls(&arguments),
    "space_lens_cleanup_execute" => local_cleanup_execute(&arguments),
    "space_lens_remote_cleanup_plan" => remote_cleanup_plan(&arguments),
    "space_lens_remote_cleanup_execute" => remote_cleanup_execute(&arguments),
    "space_lens_scan_snapshot" => scan_snapshot(&arguments),
    "space_lens_cleanup_candidates" => cleanup_candidates(&arguments),
    "space_lens_discovery" => discovery(&arguments),
    "space_lens_app_audit" => app_audit(&arguments),
    "space_lens_icloud_plan" => icloud_plan(&arguments),
    "space_lens_capabilities" => tool_success(serde_json::to_value(
      PlatformCapabilities::for_current_platform(),
    )),
    _ => tool_error(format!("unknown tool: {name}")),
  }
}

fn scan_snapshot(arguments: &Map<String, Value>) -> Value {
  let path = match required_path(arguments) {
    Ok(path) => path,
    Err(error) => return tool_error(error),
  };
  let ignored_mode = match arguments
    .get("ignoredMode")
    .and_then(Value::as_str)
    .unwrap_or("summarize")
  {
    "exclude" => IgnoredMode::Exclude,
    "summarize" => IgnoredMode::Summarize,
    value => return tool_error(format!("invalid ignoredMode: {value}")),
  };

  let tree = scan_directory(ScanOptions {
    directories: vec![path],
    ignore_hidden: arguments
      .get("ignoreHidden")
      .and_then(Value::as_bool)
      .unwrap_or(false),
    full_path: false,
    respect_gitignore: arguments
      .get("respectGitignore")
      .and_then(Value::as_bool)
      .unwrap_or(true),
    ignored_mode,
    // ScanOptions' documented default: measure links as link nodes, never
    // wander into symlinked external trees from an MCP-initiated snapshot.
    follow_symlinks: false,
  });
  tool_success(serde_json::to_value(SnapshotEnvelope::from_scan_nodes(
    &tree,
    env!("CARGO_PKG_VERSION"),
  )))
}

fn cleanup_candidates(arguments: &Map<String, Value>) -> Value {
  let path = match required_path(arguments) {
    Ok(path) => path,
    Err(error) => return tool_error(error),
  };
  let presets = match arguments.get("presets") {
    None => Vec::new(),
    Some(Value::Array(values)) => match values
      .iter()
      .map(|value| match value.as_str() {
        Some("node") => Ok(CleanupPreset::Node),
        Some("rust") => Ok(CleanupPreset::Rust),
        Some("gitignored") => Ok(CleanupPreset::Gitignored),
        Some(value) => Err(format!("invalid cleanup preset: {value}")),
        None => Err("cleanup presets must be strings".to_string()),
      })
      .collect::<Result<Vec<_>, _>>()
    {
      Ok(presets) => presets,
      Err(error) => return tool_error(error),
    },
    Some(_) => return tool_error("presets must be an array"),
  };

  let candidates = find_candidates(CandidateOptions {
    roots: vec![path],
    presets,
    ignore_hidden: arguments
      .get("ignoreHidden")
      .and_then(Value::as_bool)
      .unwrap_or(false),
    // CandidateOptions' documented default: never wander into symlinked
    // external projects from an MCP-initiated cleanup scan.
    follow_symlinks: false,
  });
  tool_success(serde_json::to_value(build_removal_plan(candidates)))
}

fn discovery(arguments: &Map<String, Value>) -> Value {
  let path = match required_path(arguments) {
    Ok(path) => path,
    Err(error) => return tool_error(error),
  };
  let kinds = match arguments.get("kinds") {
    None => DiscoveryKind::ALL.to_vec(),
    Some(Value::Array(values)) => match values
      .iter()
      .map(|value| match value.as_str() {
        Some(id) => DiscoveryKind::parse(id).ok_or_else(|| format!("invalid discovery kind: {id}")),
        None => Err("discovery kinds must be strings".to_string()),
      })
      .collect::<Result<Vec<_>, _>>()
    {
      Ok(kinds) if kinds.is_empty() => return tool_error("kinds must not be empty"),
      Ok(kinds) => kinds,
      Err(error) => return tool_error(error),
    },
    Some(_) => return tool_error("kinds must be an array"),
  };
  let min_size = arguments
    .get("minSize")
    .and_then(Value::as_u64)
    .unwrap_or(0);
  let limit = arguments
    .get("limit")
    .and_then(Value::as_u64)
    .unwrap_or(200) as usize;

  let report = run_discovery(DiscoveryOptions {
    roots: vec![path],
    ignore_hidden: arguments
      .get("ignoreHidden")
      .and_then(Value::as_bool)
      .unwrap_or(false),
    min_size,
    limit,
  });
  match report {
    // Only the requested views stay in the response.
    Ok(mut report) => {
      report
        .views
        .retain(|view| kinds.iter().any(|kind| kind.id() == view.kind));
      tool_success(serde_json::to_value(report))
    }
    Err(error) => tool_error(error.to_string()),
  }
}

fn app_audit(arguments: &Map<String, Value>) -> Value {
  let min_size = match arguments.get("minSize") {
    None => 0,
    Some(Value::Number(size)) if size.as_u64().is_some() => size.as_u64().unwrap_or(0),
    Some(_) => return tool_error("arguments.minSize must be a non-negative integer"),
  };
  let include_heuristic = match arguments.get("includeHeuristic") {
    None => true,
    Some(Value::Bool(include)) => *include,
    Some(_) => return tool_error("arguments.includeHeuristic must be a boolean"),
  };
  let limit = match arguments.get("limit") {
    None => 500,
    Some(Value::Number(limit))
      if limit
        .as_u64()
        .is_some_and(|value| (1..=1000).contains(&value)) =>
    {
      limit.as_u64().unwrap_or(500) as usize
    }
    Some(_) => return tool_error("arguments.limit must be an integer between 1 and 1000"),
  };
  match audit_installed_apps(&AppAuditOptions {
    min_size,
    include_heuristic,
    limit,
    home: None,
  }) {
    Ok(report) => tool_success(serde_json::to_value(report)),
    Err(error) => tool_error(error.to_string()),
  }
}

fn icloud_plan(arguments: &Map<String, Value>) -> Value {
  let path = match required_path(arguments) {
    Ok(path) => path,
    Err(error) => return tool_error(error),
  };
  match build_eviction_plan(&NativeICloudBackend, &path, CloudScanOptions::default()) {
    Ok(plan) => tool_success(serde_json::to_value(plan)),
    Err(error) => tool_error(error.to_string()),
  }
}

fn required_path(arguments: &Map<String, Value>) -> Result<PathBuf, String> {
  let path = arguments
    .get("path")
    .and_then(Value::as_str)
    .ok_or_else(|| "arguments.path must be a string".to_string())?;
  if path.is_empty() {
    return Err("arguments.path must not be empty".to_string());
  }
  Ok(PathBuf::from(path))
}

// --------------------------------------------------------------- device tools

/// Where one tool call runs. `None` and `"local"` mean this machine; anything
/// else is an SSH destination handed to the shared remote runtime.
enum Device<'a> {
  Local,
  Remote(&'a str),
}

fn device_argument(arguments: &Map<String, Value>) -> Result<Device<'_>, String> {
  match arguments.get("device").and_then(Value::as_str) {
    None => Ok(Device::Local),
    Some("local") => Ok(Device::Local),
    Some("") => Err("arguments.device must not be empty".into()),
    Some(spec) => Ok(Device::Remote(spec)),
  }
}

/// The live session for one SSH device, connected on first use. The policy is
/// decided once by `spacelens mcp --allow-cleanup` and holds for the process.
fn remote_session(spec: &str) -> Result<Arc<RemoteHost>, String> {
  let remotes = REMOTES.get_or_init(|| Mutex::new(HashMap::new()));
  if let Some(remote) = remotes.lock().unwrap().get(spec) {
    return Ok(remote.clone());
  }
  let policy = RemotePolicy {
    allow_cleanup: ALLOW_CLEANUP.load(Ordering::SeqCst),
  };
  let remote = remote_cmd::connect(spec, policy).map_err(|error| error.to_string())?;
  remotes
    .lock()
    .unwrap()
    .insert(spec.to_string(), remote.clone());
  Ok(remote)
}

fn host_problem(problem: &Value) -> String {
  spacelens_remote::error::problem_text_value(problem)
}

fn devices() -> Value {
  let connected = |spec: &str| {
    REMOTES
      .get()
      .map(|remotes| remotes.lock().unwrap().contains_key(spec))
      .unwrap_or(false)
  };
  let mut list = vec![json!({ "device": "local", "label": "This machine", "connected": true })];
  for (alias, destination) in remote_cmd::ssh_hosts() {
    list.push(json!({
      "device": alias,
      "destination": destination,
      "connected": connected(&alias),
    }));
  }
  tool_success(serde_json::to_value(list))
}

fn device_scan(arguments: &Map<String, Value>) -> Value {
  let Ok(path) = required_path(arguments) else {
    return tool_error("arguments.path must be a non-empty string");
  };
  let Ok(device) = device_argument(arguments) else {
    return tool_error("arguments.device must be \"local\" or an ssh destination");
  };
  match device {
    Device::Local => {
      if !path.is_dir() {
        return tool_error(format!(
          "no such folder on this machine: {}",
          path.display()
        ));
      }
      let scan_id = format!("scan_mcp_{}", NEXT_SCAN.fetch_add(1, Ordering::SeqCst));
      let state = Arc::new(Mutex::new(LocalScanState::Scanning));
      LOCAL_SCANS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .insert(
          scan_id.clone(),
          LocalScan {
            state: state.clone(),
            started: Instant::now(),
          },
        );
      let path_text = path.to_string_lossy().into_owned();
      std::thread::spawn(move || {
        let trees = scan_directory(ScanOptions {
          directories: vec![PathBuf::from(&path_text)],
          full_path: true,
          respect_gitignore: true,
          ignored_mode: IgnoredMode::Summarize,
          ignore_hidden: false,
          follow_symlinks: false,
        });
        *state.lock().unwrap() = LocalScanState::Ready(trees);
      });
      tool_success(Ok(json!({
        "scanId": scan_id,
        "device": "local",
        "state": "scanning",
        "path": path,
      })))
    }
    Device::Remote(spec) => {
      let Ok(remote) = remote_session(spec) else {
        return tool_error(format!("cannot connect to {spec}"));
      };
      let request = spacelens_remote::runtime::ScanStart {
        paths: vec![path.to_string_lossy().into_owned()],
        ..spacelens_remote::runtime::ScanStart::default()
      };
      match remote.scan_start(&request) {
        Ok(value) => {
          let mut value = value;
          if let Some(object) = value.as_object_mut() {
            object.insert("device".into(), json!(spec));
          }
          tool_success(Ok(value))
        }
        Err(problem) => tool_error(host_problem(&problem)),
      }
    }
  }
}

fn device_scan_status(arguments: &Map<String, Value>) -> Value {
  let Some(scan_id) = arguments.get("scanId").and_then(Value::as_str) else {
    return tool_error("arguments.scanId must be a string");
  };
  let Ok(device) = device_argument(arguments) else {
    return tool_error("arguments.device must be \"local\" or an ssh destination");
  };
  match device {
    Device::Local => {
      let scans = LOCAL_SCANS.get_or_init(|| Mutex::new(HashMap::new()));
      let Some(scan) = scans.lock().unwrap().get(scan_id).cloned() else {
        return tool_error(format!("unknown scan {scan_id}"));
      };
      let elapsed = scan.started.elapsed().as_secs();
      let state = scan.state.lock().unwrap();
      match &*state {
        LocalScanState::Scanning => tool_success(Ok(json!({
          "scanId": scan_id,
          "device": "local",
          "state": "scanning",
          "elapsedSeconds": elapsed,
        }))),
        LocalScanState::Ready(trees) => tool_success(Ok(json!({
          "scanId": scan_id,
          "device": "local",
          "state": "ready",
          "roots": trees.iter().map(|tree| tree.path.to_string_lossy()).collect::<Vec<_>>(),
          "elapsedSeconds": elapsed,
        }))),
      }
    }
    Device::Remote(spec) => {
      let Ok(remote) = remote_session(spec) else {
        return tool_error(format!("cannot connect to {spec}"));
      };
      match remote.scan_status(scan_id) {
        Ok(value) => tool_success(Ok(value)),
        Err(problem) => tool_error(host_problem(&problem)),
      }
    }
  }
}

fn paging(arguments: &Map<String, Value>) -> Result<(usize, usize, String), String> {
  let limit = match arguments.get("limit") {
    None => 50,
    Some(value) => value
      .as_u64()
      .filter(|limit| (1..=1000).contains(limit))
      .ok_or("arguments.limit must be an integer between 1 and 1000")? as usize,
  };
  let offset = match arguments.get("offset") {
    None => 0,
    Some(value) => value
      .as_u64()
      .ok_or("arguments.offset must be a non-negative integer")? as usize,
  };
  let sort = match arguments.get("sort").and_then(Value::as_str) {
    None => "size",
    Some(sort @ ("size" | "name" | "path")) => sort,
    Some(_) => return Err("arguments.sort must be one of size, name, path".into()),
  };
  Ok((limit, offset, sort.to_string()))
}

fn device_ls(arguments: &Map<String, Value>) -> Value {
  let Some(scan_id) = arguments.get("scanId").and_then(Value::as_str) else {
    return tool_error("arguments.scanId must be a string");
  };
  let Ok((limit, offset, sort)) = paging(arguments) else {
    return tool_error("arguments.limit, offset and sort are out of range");
  };
  let path = arguments.get("path").and_then(Value::as_str);
  let node_id = arguments.get("nodeId").and_then(Value::as_str);
  let Ok(device) = device_argument(arguments) else {
    return tool_error("arguments.device must be \"local\" or an ssh destination");
  };
  match device {
    Device::Local => {
      if node_id.is_some() {
        return tool_error("local scans navigate by path; nodeId is for remote scans");
      }
      let scans = LOCAL_SCANS.get_or_init(|| Mutex::new(HashMap::new()));
      let Some(scan) = scans.lock().unwrap().get(scan_id).cloned() else {
        return tool_error(format!("unknown scan {scan_id}"));
      };
      let state = scan.state.lock().unwrap();
      let LocalScanState::Ready(trees) = &*state else {
        return tool_success(Ok(json!({ "scanId": scan_id, "state": "scanning" })));
      };
      let roots: Vec<&ScanNode> = trees.iter().collect();
      let listing = match path {
        None => {
          if roots.len() == 1 {
            children_page(&roots[0].children, limit, offset, &sort)
          } else {
            Value::Array(roots.iter().map(|root| node_summary(root)).collect())
          }
        }
        Some(path) => match descend_by_path(&roots, Path::new(path)) {
          Ok(node) => children_page(&node.children, limit, offset, &sort),
          Err(message) => return tool_error(message),
        },
      };
      tool_success(Ok(
        json!({ "scanId": scan_id, "path": path, "children": listing }),
      ))
    }
    Device::Remote(spec) => {
      let Ok(remote) = remote_session(spec) else {
        return tool_error(format!("cannot connect to {spec}"));
      };
      // Resolve the node to list: an explicit id, a path descent, or the root.
      let target: Result<String, String> = (|| {
        if let Some(node_id) = node_id {
          return Ok(node_id.to_string());
        }
        let status = remote
          .scan_status(scan_id)
          .map_err(|problem| host_problem(&problem))?;
        let root_id = status["rootIds"]
          .get(0)
          .and_then(Value::as_str)
          .ok_or("the scan reported no root id")?
          .to_string();
        let Some(path) = path else {
          return Ok(root_id);
        };
        descend_remote_by_path(&remote, scan_id, &root_id, path)
      })();
      let Ok(node_id) = target else {
        return tool_error(target.unwrap_err());
      };
      match remote.tree_children(scan_id, &node_id, offset, limit, &sort) {
        Ok(page) => {
          let items = page["items"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|item| {
              json!({
                "id": item["id"],
                "name": item["name"],
                "path": item["path"],
                "size": item["size"],
                "childCount": item["childCount"],
                "ignored": item["ignored"],
              })
            })
            .collect::<Vec<_>>();
          tool_success(Ok(json!({
            "scanId": scan_id,
            "nodeId": node_id,
            "path": path,
            "total": page["total"],
            "offset": page["offset"],
            "limit": page["limit"],
            "children": items,
          })))
        }
        Err(problem) => tool_error(host_problem(&problem)),
      }
    }
  }
}

/// Walks one local tree to the requested folder; paths are the node address.
fn descend_by_path<'a>(roots: &[&'a ScanNode], path: &Path) -> Result<&'a ScanNode, String> {
  let mut current = roots
    .iter()
    .copied()
    .find(|root| path.starts_with(&root.path))
    .ok_or_else(|| format!("{} is outside this scan", path.display()))?;
  let remaining = path
    .strip_prefix(&current.path)
    .map_err(|_| format!("{} is outside this scan", path.display()))?;
  for component in remaining.components() {
    let component = component.as_os_str();
    current = current
      .children
      .iter()
      .find(|child| child.path.ends_with(component))
      .ok_or_else(|| format!("{} was not found in the scan", path.display()))?;
  }
  Ok(current)
}

/// Resolves a folder path to a remote node id by walking the host's tree one
/// level at a time; each step is one paginated children request.
fn descend_remote_by_path(
  remote: &RemoteHost,
  scan_id: &str,
  root_id: &str,
  path: &str,
) -> Result<String, String> {
  let wanted = path.trim_end_matches('/');
  let mut node_id = root_id.to_string();
  loop {
    let page = remote
      .tree_children(scan_id, &node_id, 0, 1000, "size")
      .map_err(|problem| host_problem(&problem))?;
    let items = page["items"].as_array().cloned().unwrap_or_default();
    let Some(item) = items.iter().find(|item| {
      item["path"]
        .as_str()
        .map(|candidate| candidate == wanted || wanted.starts_with(&format!("{candidate}/")))
        == Some(true)
    }) else {
      return Err(format!("{wanted} was not found in the scan"));
    };
    if item["path"].as_str() == Some(wanted) {
      return Ok(item["id"].as_str().unwrap_or_default().to_string());
    }
    node_id = item["id"].as_str().unwrap_or_default().to_string();
  }
}

fn node_summary(node: &ScanNode) -> Value {
  // The scanner fills `name` with the full path when `full_path` is on, so
  // the display name always comes from the path's last component.
  let name = Path::new(&node.path)
    .file_name()
    .map(|name| name.to_string_lossy().into_owned())
    .unwrap_or_else(|| node.name.clone());
  // Children are absent for collapsed and ignored nodes alike, so directory
  // detection reads the filesystem rather than inferring from the tree.
  let is_directory = std::fs::metadata(&node.path)
    .map(|metadata| metadata.is_dir())
    .unwrap_or(false);
  json!({
    "name": name,
    "path": node.path.to_string_lossy(),
    "size": node.size,
    "isDirectory": is_directory,
    "childCount": node.children.len(),
    "ignored": node.ignored,
  })
}

fn children_page(children: &[ScanNode], limit: usize, offset: usize, sort: &str) -> Value {
  let mut items: Vec<&ScanNode> = children.iter().collect();
  match sort {
    "name" => items.sort_by_key(|node| node.name.as_str()),
    "path" => items.sort_by_key(|node| node.path.as_os_str()),
    _ => items.sort_by_key(|node| std::cmp::Reverse(node.size)),
  }
  let total = items.len();
  let items = items
    .into_iter()
    .skip(offset)
    .take(limit)
    .map(node_summary)
    .collect::<Vec<_>>();
  json!({ "total": total, "offset": offset, "limit": limit, "items": items })
}

fn cleanup_guard() -> Option<Value> {
  if ALLOW_CLEANUP.load(Ordering::SeqCst) {
    None
  } else {
    Some(tool_error(
      "cleanup is disabled: restart spacelens mcp with --allow-cleanup",
    ))
  }
}

fn local_cleanup_execute(arguments: &Map<String, Value>) -> Value {
  if let Some(refusal) = cleanup_guard() {
    return refusal;
  }
  let Some(paths) = arguments.get("paths").and_then(Value::as_array) else {
    return tool_error("arguments.paths must be an array of strings");
  };
  let entries: Result<Vec<RemovalEntry>, String> = paths
    .iter()
    .map(|value| {
      let path = value
        .as_str()
        .filter(|path| !path.is_empty())
        .ok_or("arguments.paths must contain non-empty strings")?;
      Ok(RemovalEntry {
        path: PathBuf::from(path),
        size: 0,
        reason: "mcp cleanup_execute".into(),
        preset: CleanupPreset::Node,
      })
    })
    .collect();
  let Ok(entries) = entries else {
    return tool_error(entries.unwrap_err());
  };
  let plan = RemovalPlan {
    entries,
    total_size: 0,
    errors: Vec::new(),
  };
  tool_success(serde_json::to_value(execute_removal_plan(&plan)))
}

fn remote_cleanup_plan(arguments: &Map<String, Value>) -> Value {
  if let Some(refusal) = cleanup_guard() {
    return refusal;
  }
  let Some(spec) = arguments.get("device").and_then(Value::as_str) else {
    return tool_error("arguments.device must be an ssh destination");
  };
  let Some(scan_id) = arguments.get("scanId").and_then(Value::as_str) else {
    return tool_error("arguments.scanId must be a string");
  };
  let Some(node_ids) = arguments.get("nodeIds").and_then(Value::as_array) else {
    return tool_error("arguments.nodeIds must be an array of strings");
  };
  let node_ids: Vec<String> = node_ids
    .iter()
    .filter_map(Value::as_str)
    .map(str::to_string)
    .collect();
  if node_ids.is_empty() {
    return tool_error("arguments.nodeIds must not be empty");
  }
  let Ok(remote) = remote_session(spec) else {
    return tool_error(format!("cannot connect to {spec}"));
  };
  match remote.cleanup_plan(scan_id, &node_ids) {
    Ok(plan) => tool_success(Ok(plan)),
    Err(problem) => tool_error(host_problem(&problem)),
  }
}

fn remote_cleanup_execute(arguments: &Map<String, Value>) -> Value {
  if let Some(refusal) = cleanup_guard() {
    return refusal;
  }
  let Some(spec) = arguments.get("device").and_then(Value::as_str) else {
    return tool_error("arguments.device must be an ssh destination");
  };
  let Some(plan_id) = arguments.get("planId").and_then(Value::as_str) else {
    return tool_error("arguments.planId must be a string");
  };
  let Ok(remote) = remote_session(spec) else {
    return tool_error(format!("cannot connect to {spec}"));
  };
  match remote.cleanup_execute(plan_id) {
    Ok(outcome) => tool_success(Ok(outcome)),
    Err(problem) => tool_error(host_problem(&problem)),
  }
}

fn tool_success(value: Result<Value, serde_json::Error>) -> Value {
  match value {
    Ok(value) => json!({
      "content": [{ "type": "text", "text": serde_json::to_string_pretty(&value).unwrap_or_default() }],
      "structuredContent": value
    }),
    Err(error) => tool_error(format!("could not encode tool result: {error}")),
  }
}

fn tool_error(message: impl Into<String>) -> Value {
  let message = message.into();
  json!({
    "content": [{ "type": "text", "text": message }],
    "isError": true
  })
}

fn success_response(id: Value, result: Value) -> Value {
  json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i64, message: impl Into<String>) -> Value {
  json!({
    "jsonrpc": "2.0",
    "id": id,
    "error": { "code": code, "message": message.into() }
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn initialize_advertises_read_only_tools() {
    let response = handle_request(json!({
      "jsonrpc": "2.0",
      "id": 1,
      "method": "initialize",
      "params": { "protocolVersion": PROTOCOL_VERSION }
    }))
    .expect("response");

    assert_eq!(response["result"]["protocolVersion"], PROTOCOL_VERSION);
    assert!(response["result"]["capabilities"]["tools"].is_object());
    assert!(response["result"]["instructions"]
      .as_str()
      .expect("instructions")
      .contains("read-only"));
  }

  #[test]
  fn tools_list_contains_no_destructive_tool() {
    let response = handle_request(json!({
      "jsonrpc": "2.0",
      "id": "tools",
      "method": "tools/list"
    }))
    .expect("response");
    let tools = response["result"]["tools"].as_array().expect("tools");
    assert!(tools
      .iter()
      .any(|tool| tool["name"] == "space_lens_scan_snapshot"));
    assert!(tools
      .iter()
      .any(|tool| tool["name"] == "space_lens_discovery"));
    assert!(!tools.iter().any(|tool| tool["name"] == "space_lens_delete"));
    assert!(!tools.iter().any(|tool| tool["name"] == "space_lens_evict"));
  }

  #[test]
  fn initialized_notification_has_no_response() {
    assert!(handle_request(json!({
      "jsonrpc": "2.0",
      "method": "notifications/initialized"
    }))
    .is_none());
  }

  fn fixture_project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("spacelens-mcp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("project/node_modules/vendor/node_modules")).unwrap();
    std::fs::write(root.join("project/package.json"), "{}").unwrap();
    std::fs::write(root.join("project/node_modules/vendor/package.json"), "{}").unwrap();
    std::fs::write(root.join("project/.gitignore"), "build\n").unwrap();
    std::fs::create_dir_all(root.join("project/build")).unwrap();
    // Heap-allocated: a 2 MiB array temporary would live on the test thread's
    // stack and overflow it (the CI runners run tests with tight stacks).
    std::fs::write(
      root.join("project/build/heavy.bin"),
      vec![0u8; 2 * 1024 * 1024],
    )
    .unwrap();
    root
  }

  fn discovery_call(path: &Path, arguments: Value) -> Value {
    let mut arguments = arguments;
    arguments["path"] = json!(path.to_string_lossy());
    handle_request(json!({
      "jsonrpc": "2.0",
      "id": 7,
      "method": "tools/call",
      "params": { "name": "space_lens_discovery", "arguments": arguments }
    }))
    .expect("response")["result"]
      .clone()
  }

  #[test]
  fn discovery_returns_caches_and_gitignored_views() {
    let root = fixture_project("views");
    let result = discovery_call(&root, json!({}));
    assert_ne!(result["isError"], true);
    let views = result["structuredContent"]["views"]
      .as_array()
      .expect("views");
    let caches = views
      .iter()
      .find(|view| view["kind"] == "caches")
      .expect("caches view");
    assert!(caches["items"]
      .as_array()
      .unwrap()
      .iter()
      .any(|item| item["path"]
        .as_str()
        .unwrap()
        .ends_with("project/node_modules")));
    assert!(!caches["items"]
      .as_array()
      .unwrap()
      .iter()
      .any(|item| item["path"]
        .as_str()
        .unwrap()
        .contains("vendor/node_modules")));

    let gitignored = views
      .iter()
      .find(|view| view["kind"] == "gitignored")
      .expect("gitignored view");
    assert!(gitignored["items"]
      .as_array()
      .unwrap()
      .iter()
      .any(|item| item["path"].as_str().unwrap().ends_with("project/build")));

    let large = views
      .iter()
      .find(|view| view["kind"] == "large-files")
      .expect("large-files view");
    assert!(large["items"]
      .as_array()
      .unwrap()
      .iter()
      .any(|item| item["path"].as_str().unwrap().ends_with("heavy.bin")));
    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn discovery_honors_kind_selection_and_rejects_unknown_kinds() {
    let root = fixture_project("kinds");
    let result = discovery_call(&root, json!({ "kinds": ["caches"] }));
    let views = result["structuredContent"]["views"]
      .as_array()
      .expect("views");
    assert_eq!(views.len(), 1);
    assert_eq!(views[0]["kind"], "caches");

    let invalid = discovery_call(&root, json!({ "kinds": ["bogus"] }));
    assert_eq!(invalid["isError"], true);
    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn app_audit_tool_is_listed_as_read_only() {
    let response = handle_request(json!({
      "jsonrpc": "2.0",
      "id": "audit",
      "method": "tools/list"
    }))
    .expect("response");
    let tools = response["result"]["tools"].as_array().expect("tools");
    let tool = tools
      .iter()
      .find(|tool| tool["name"] == "space_lens_app_audit")
      .expect("app audit tool");
    assert!(tool["description"]
      .as_str()
      .expect("description")
      .contains("Read-only"));
    // The schema takes only the three documented optional arguments: nothing
    // is required and nothing else is accepted.
    let schema = &tool["inputSchema"];
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["additionalProperties"], false);
    assert!(schema.get("required").is_none());
    let properties = schema["properties"].as_object().expect("properties");
    assert_eq!(properties.len(), 3);
    assert_eq!(properties["minSize"]["type"], "integer");
    assert_eq!(properties["minSize"]["minimum"], 0);
    assert_eq!(properties["minSize"]["default"], 0);
    assert_eq!(properties["includeHeuristic"]["type"], "boolean");
    assert_eq!(properties["includeHeuristic"]["default"], true);
    assert_eq!(properties["limit"]["type"], "integer");
    assert_eq!(properties["limit"]["minimum"], 1);
    assert_eq!(properties["limit"]["maximum"], 1000);
    assert_eq!(properties["limit"]["default"], 500);
  }

  #[test]
  fn app_audit_rejects_invalid_arguments_without_scanning() {
    // Every case fails argument validation before any filesystem scan runs.
    for arguments in [
      json!({ "limit": 0 }),
      json!({ "limit": 1001 }),
      json!({ "limit": -5 }),
      json!({ "limit": "many" }),
      json!({ "limit": 1.5 }),
      json!({ "minSize": -1 }),
      json!({ "minSize": "1024" }),
      json!({ "includeHeuristic": "yes" }),
      json!({ "includeHeuristic": 1 }),
    ] {
      let result = handle_request(json!({
        "jsonrpc": "2.0",
        "id": 9,
        "method": "tools/call",
        "params": { "name": "space_lens_app_audit", "arguments": arguments }
      }))
      .expect("response")["result"]
        .clone();
      assert_eq!(result["isError"], true, "arguments: {arguments}");
    }
  }

  // ---------------------------------------------------------- device tools

  fn call(name: &str, arguments: Value) -> Value {
    handle_request(json!({
      "jsonrpc": "2.0",
      "id": 11,
      "method": "tools/call",
      "params": { "name": name, "arguments": arguments }
    }))
    .expect("response")["result"]
      .clone()
  }

  #[test]
  fn the_device_tools_are_listed_and_describe_their_gating() {
    let response = handle_request(json!({
      "jsonrpc": "2.0",
      "id": "tools",
      "method": "tools/list"
    }))
    .expect("response");
    let tools = response["result"]["tools"].as_array().expect("tools");
    for expected in [
      "space_lens_devices",
      "space_lens_scan",
      "space_lens_scan_status",
      "space_lens_ls",
      "space_lens_cleanup_execute",
      "space_lens_remote_cleanup_plan",
      "space_lens_remote_cleanup_execute",
    ] {
      assert!(
        tools.iter().any(|tool| tool["name"] == expected),
        "missing {expected}"
      );
    }
    let execute = tools
      .iter()
      .find(|tool| tool["name"] == "space_lens_cleanup_execute")
      .expect("cleanup execute");
    assert!(execute["description"]
      .as_str()
      .expect("description")
      .contains("--allow-cleanup"));
  }

  #[test]
  fn a_local_scan_starts_status_and_ls_round_trip() {
    let root = fixture_project("async");
    let project = root.join("project");

    let started = call(
      "space_lens_scan",
      json!({ "path": project.to_string_lossy() }),
    );
    assert_ne!(started["isError"], true, "scan start failed: {started}");
    let scan_id = started["structuredContent"]["scanId"]
      .as_str()
      .expect("scanId")
      .to_string();

    // The scan is genuinely asynchronous: keep polling until it is ready.
    let mut ready = false;
    for _ in 0..200 {
      let status = call("space_lens_scan_status", json!({ "scanId": scan_id }));
      assert_ne!(status["isError"], true, "status failed: {status}");
      if status["structuredContent"]["state"] == "ready" {
        ready = true;
        break;
      }
      std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(ready, "the local scan never became ready");

    let listed = call("space_lens_ls", json!({ "scanId": scan_id, "limit": 10 }));
    assert_ne!(listed["isError"], true, "ls failed: {listed}");
    let children = listed["structuredContent"]["children"]["items"]
      .as_array()
      .expect("children");
    assert!(
      children
        .iter()
        .any(|child| child["name"] == "node_modules" && child["size"].as_u64().unwrap_or(0) > 0),
      "node_modules missing from the root listing: {children:?}"
    );

    let inside = call(
      "space_lens_ls",
      json!({
        "scanId": scan_id,
        "path": project.join("node_modules").to_string_lossy(),
      }),
    );
    assert_ne!(inside["isError"], true, "deep ls failed: {inside}");
    let names = inside["structuredContent"]["children"]["items"]
      .as_array()
      .cloned()
      .unwrap_or_default();
    assert!(names.iter().any(|child| child["name"] == "vendor"));

    let _ = std::fs::remove_dir_all(&root);
  }

  #[test]
  fn cleanup_execute_is_refused_without_the_flag() {
    let root = fixture_project("guard");
    let target = root.join("project/build");

    let refused = call(
      "space_lens_cleanup_execute",
      json!({ "paths": [target.to_string_lossy()] }),
    );
    assert_eq!(refused["isError"], true);
    assert!(refused["content"][0]["text"]
      .as_str()
      .expect("text")
      .contains("--allow-cleanup"));
    // Nothing was removed.
    assert!(target.is_dir());

    let _ = std::fs::remove_dir_all(&root);
  }
}
