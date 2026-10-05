use serde_json::{json, Map, Value};
use space_lens::cloud::{
  build_eviction_plan, NativeICloudBackend, ScanOptions as CloudScanOptions,
};
use space_lens::{
  build_removal_plan, find_candidates, scan_directory, CandidateOptions, CleanupPreset,
  IgnoredMode, PlatformCapabilities, ScanOptions, SnapshotEnvelope,
};
use spacelens_discovery::{run_discovery, DiscoveryKind, DiscoveryOptions};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

const PROTOCOL_VERSION: &str = "2025-11-25";

pub fn run() -> anyhow::Result<()> {
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

  Ok(())
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
  json!({
    "protocolVersion": PROTOCOL_VERSION,
    "capabilities": { "tools": { "listChanged": false } },
    "serverInfo": {
      "name": "space-lens",
      "version": env!("CARGO_PKG_VERSION")
    },
    "instructions": "Space Lens MCP is read-only. It can inspect filesystem snapshots, discover large files, developer caches, and gitignored paths, report cleanup candidates, and inspect iCloud eviction plans. It never deletes files, moves items to Trash, requests iCloud downloads, or evicts iCloud local copies through MCP."
  })
}

fn tool_definitions() -> Vec<Value> {
  vec![
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
    "space_lens_scan_snapshot" => scan_snapshot(&arguments),
    "space_lens_cleanup_candidates" => cleanup_candidates(&arguments),
    "space_lens_discovery" => discovery(&arguments),
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
    std::fs::write(root.join("project/build/heavy.bin"), [0u8; 2 * 1024 * 1024]).unwrap();
    root
  }

  fn discovery_call(path: &PathBuf, arguments: Value) -> Value {
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
}
