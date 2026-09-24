use serde_json::{json, Map, Value};
use space_lens::cloud::{
  build_eviction_plan, NativeICloudBackend, ScanOptions as CloudScanOptions,
};
use space_lens::{
  build_removal_plan, find_candidates, scan_directory, CandidateOptions, CleanupPreset,
  IgnoredMode, PlatformCapabilities, ScanOptions, SnapshotEnvelope,
};
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
    "instructions": "Space Lens MCP is read-only. It can inspect filesystem snapshots, report cleanup candidates, and inspect iCloud eviction plans. It never deletes files, moves items to Trash, requests iCloud downloads, or evicts iCloud local copies through MCP."
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
  });
  tool_success(serde_json::to_value(build_removal_plan(candidates)))
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
}
