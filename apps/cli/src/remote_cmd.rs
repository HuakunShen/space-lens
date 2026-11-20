//! `spacelens remote <subcommand>`: one-shot operations against a machine
//! reached over the user's own OpenSSH.
//!
//! Provisioning is the same pipeline the desktop runs: probe, install the
//! pinned agent and host package under the remote home without sudo, start
//! `spacelens serve` on the remote loopback, tunnel, pair. Every command ends
//! by stopping the remote server, so its pid file never outlives the run.
//!
//! Local artifacts come from the pinned manifest
//! (`crates/remote/agents/manifest.json`); before a release exists,
//! `SPACLENS_REMOTE_MANIFEST`, `SPACELENS_REMOTE_AGENT` and
//! `SPACELENS_REMOTE_HOST_TARBALL` point the same pipeline at local builds
//! and an `npm pack` tarball.

use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use serde_json::{json, Value};
use spacelens_remote::install::InstallProgress;
use spacelens_remote::manifest::AgentManifest;
use spacelens_remote::runtime::{self, RemoteHost, ScanStart};
use spacelens_remote::{RemotePolicy, SshTarget};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::{format_bytes, print_json};

const MANIFEST_JSON: &str = include_str!("../../../crates/remote/agents/manifest.json");
const STATUS_POLL: Duration = Duration::from_secs(2);

#[derive(Debug, Subcommand)]
pub enum RemoteCommand {
  /// List concrete hosts from ~/.ssh/config.
  List,
  /// Probe a target: os, arch, shell, node, npm, and the agent it resolves to.
  Probe {
    /// An alias from ~/.ssh/config, or `user@host[:port]`.
    #[arg(value_name = "TARGET")]
    target: String,
  },
  /// Scan a folder on the target and print the largest entries.
  Scan(RemoteScanArgs),
  /// Show discovery views (large files, caches, gitignored) for one scan.
  Discovery(RemoteDiscoveryArgs),
  /// Plan (and optionally execute) a trash-only cleanup for remote nodes.
  Cleanup(RemoteCleanupArgs),
}

#[derive(Debug, Args)]
pub struct RemoteScanArgs {
  /// An alias from ~/.ssh/config, or `user@host[:port]`.
  #[arg(value_name = "TARGET")]
  pub target: String,
  /// Folder on the remote; defaults to the first root the host reports.
  #[arg(long, value_name = "PATH")]
  pub path: Option<String>,
  /// How many entries to print, largest first.
  #[arg(long, default_value_t = 20)]
  pub limit: usize,
  /// Full scan status as JSON instead of a table.
  #[arg(long)]
  pub json: bool,
}

#[derive(Debug, Args)]
pub struct RemoteDiscoveryArgs {
  /// An alias from ~/.ssh/config, or `user@host[:port]`.
  #[arg(value_name = "TARGET")]
  pub target: String,
  /// The scan id from `remote scan`.
  #[arg(long, value_name = "ID")]
  pub scan_id: String,
  /// Views to fetch; defaults to all three.
  #[arg(long, value_enum)]
  pub kind: Vec<DiscoveryKindArg>,
  /// Drop items smaller than this many bytes.
  #[arg(long, default_value_t = 0)]
  pub min_size: u64,
  /// Maximum items per view, largest first.
  #[arg(long, default_value_t = 50)]
  pub limit: usize,
  /// Full discovery pages as JSON instead of a table.
  #[arg(long)]
  pub json: bool,
}

#[derive(Debug, Args)]
pub struct RemoteCleanupArgs {
  /// An alias from ~/.ssh/config, or `user@host[:port]`.
  #[arg(value_name = "TARGET")]
  pub target: String,
  /// The scan id from `remote scan`.
  #[arg(long, value_name = "ID")]
  pub scan_id: String,
  /// Nodes to clean, as printed by `remote scan --json` (field `id`).
  #[arg(long = "node-id", value_name = "ID")]
  pub node_ids: Vec<String>,
  /// Move the planned nodes to the remote trash. Without this flag the
  /// command only prints the plan.
  #[arg(long)]
  pub execute: bool,
  /// Full plan/outcome as JSON instead of a table.
  #[arg(long)]
  pub json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DiscoveryKindArg {
  #[value(name = "large-files")]
  LargeFiles,
  #[value(name = "caches")]
  Caches,
  #[value(name = "gitignored")]
  Gitignored,
}

impl DiscoveryKindArg {
  fn wire(self) -> &'static str {
    match self {
      Self::LargeFiles => "large-files",
      Self::Caches => "caches",
      Self::Gitignored => "gitignored",
    }
  }
}

pub fn run(command: RemoteCommand) -> Result<()> {
  match command {
    RemoteCommand::List => list(),
    RemoteCommand::Probe { target } => probe(&target),
    RemoteCommand::Scan(args) => scan(args),
    RemoteCommand::Discovery(args) => discovery(args),
    RemoteCommand::Cleanup(args) => cleanup(args),
  }
}

// -------------------------------------------------------------------- list

fn list() -> Result<()> {
  let home = std::env::var("HOME")
    .map(PathBuf::from)
    .context("HOME is not set; cannot read ~/.ssh/config")?;
  let listing = spacelens_remote::ssh_config::load(&home);
  if listing.hosts.is_empty() {
    if listing.complete {
      println!("~/.ssh/config defines no concrete hosts.");
    } else {
      println!(
        "~/.ssh/config could not be read completely:{}",
        listing
          .problems
          .iter()
          .map(|problem| format!("\n  {problem}"))
          .collect::<String>()
      );
    }
    return Ok(());
  }
  for host in &listing.hosts {
    let destination = match (&host.user, &host.host_name, host.port) {
      (Some(user), Some(name), Some(port)) => format!("{user}@{name}:{port}"),
      (Some(user), Some(name), None) => format!("{user}@{name}"),
      (_, _, port) => match port {
        Some(port) => format!("{}:{port}", host.alias),
        None => host.alias.clone(),
      },
    };
    println!("{:<24} {}", host.alias, destination);
  }
  if !listing.complete {
    for problem in &listing.problems {
      eprintln!("warning: {problem}");
    }
  }
  Ok(())
}

// ------------------------------------------------------------------- probe

fn probe(spec: &str) -> Result<()> {
  let target = SshTarget::parse(spec)?;
  let probe = runtime::probe(&target).map_err(|error| anyhow!("{error}"))?;
  let resolved = spacelens_remote::probe::resolve_target(&probe.os, &probe.arch);
  println!("target        {}", target);
  println!("os            {} ({})", probe.os, probe.arch);
  println!("home          {}", probe.home);
  println!("shell         {}", probe.shell);
  println!(
    "writable      {}",
    if probe.writable { "yes" } else { "no" }
  );
  println!(
    "node          {}",
    probe
      .node
      .as_deref()
      .unwrap_or("(not found; Linux/macOS remotes need Node.js >= 20)")
  );
  println!(
    "node version  {}",
    probe.node_version.as_deref().unwrap_or("-")
  );
  println!("npm           {}", probe.npm.as_deref().unwrap_or("-"));
  match resolved {
    Ok(resolved) => println!("agent triple  {}", resolved.triple),
    Err(error) => println!("agent triple  unresolved: {error}"),
  }
  Ok(())
}

// -------------------------------------------------------------------- scan

/// Stops the remote server when the command ends, whatever path it takes.
struct StopOnDrop(Arc<RemoteHost>);

impl Drop for StopOnDrop {
  fn drop(&mut self) {
    self.0.stop();
  }
}

fn scan(args: RemoteScanArgs) -> Result<()> {
  let policy = RemotePolicy::read_only();
  let remote = connect(&args.target, policy)?;
  let _guard = StopOnDrop(remote.clone());

  let path = match &args.path {
    Some(path) => path.clone(),
    None => {
      let roots = host(remote.roots())?;
      roots["roots"]
        .get(0)
        .and_then(|root| root.get("path"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("the remote host reported no scan roots"))?
    }
  };
  if !args.json {
    eprintln!("scanning {path} on {} …", args.target);
  }
  let scan = host(remote.scan_start(&ScanStart {
    paths: vec![path.clone()],
    ..ScanStart::default()
  }))?;
  let scan_id = text(&scan, "scanId")?.to_string();

  let mut last_state = String::new();
  loop {
    let status = host(remote.scan_status(&scan_id))?;
    let state = status["state"].as_str().unwrap_or_default().to_string();
    if state != last_state {
      last_state = state.clone();
      if !args.json {
        eprintln!("scan {scan_id}: {state}");
      }
    }
    match state.as_str() {
      "ready" | "failed" | "cancelled" => break,
      _ => {
        if !args.json {
          eprintln!(
            "  {} in {} entries",
            format_bytes(status["bytesScanned"].as_u64().unwrap_or(0)),
            status["entriesScanned"].as_u64().unwrap_or(0)
          );
        }
        std::thread::sleep(STATUS_POLL);
      }
    }
  }

  if args.json {
    let children = host(remote.tree_children(&scan_id, "root", 0, args.limit, "size"))?;
    print_json(&json!({
      "scan": status_of(&remote, &scan_id)?,
      "path": path,
      "children": children,
    }))?;
    return Ok(());
  }

  let status = status_of(&remote, &scan_id)?;
  if status["state"].as_str() != Some("ready") {
    return Err(anyhow!(
      "scan ended in {}: {}",
      status["state"].as_str().unwrap_or("?"),
      status["message"].as_str().unwrap_or("no message")
    ));
  }
  let root_id = status["rootIds"]
    .get(0)
    .and_then(Value::as_str)
    .context("the ready scan reported no root id")?;

  let children = host(remote.tree_children(&scan_id, root_id, 0, args.limit, "size"))?;
  println!("{} — largest entries under {path}", args.target);
  println!("{:>10}  path", "size");
  for item in children["items"].as_array().unwrap_or(&Vec::new()) {
    let size = item["size"].as_u64().unwrap_or(0);
    let name = item["name"].as_str().unwrap_or("?");
    println!("{:>10}  {}", format_bytes(size), name);
  }
  let total = children["total"].as_u64().unwrap_or(0);
  if total > args.limit as u64 {
    println!("… and {} more entries", total - args.limit as u64);
  }
  Ok(())
}

fn status_of(remote: &Arc<RemoteHost>, scan_id: &str) -> Result<Value> {
  host(remote.scan_status(scan_id))
} // --------------------------------------------------------------- discovery

fn discovery(args: RemoteDiscoveryArgs) -> Result<()> {
  let kinds = if args.kind.is_empty() {
    vec![
      DiscoveryKindArg::LargeFiles,
      DiscoveryKindArg::Caches,
      DiscoveryKindArg::Gitignored,
    ]
  } else {
    args.kind.clone()
  };
  let remote = connect(&args.target, RemotePolicy::read_only())?;
  let _guard = StopOnDrop(remote.clone());

  let mut pages: Vec<Value> = Vec::new();
  for kind in kinds {
    let page = host(remote.discovery(&args.scan_id, kind.wire(), args.min_size, 0, args.limit))?;
    pages.push(page);
  }

  if args.json {
    print_json(&Value::Array(pages))?;
    return Ok(());
  }
  for page in &pages {
    println!(
      "== {} ({} items, {})",
      page["kind"].as_str().unwrap_or("?"),
      page["total"].as_u64().unwrap_or(0),
      format_bytes(page["totalSize"].as_u64().unwrap_or(0))
    );
    for item in page["items"].as_array().unwrap_or(&Vec::new()) {
      let node = &item["node"];
      println!(
        "{:>10}  {}",
        format_bytes(node["size"].as_u64().unwrap_or(0)),
        node["path"].as_str().unwrap_or("?")
      );
    }
  }
  Ok(())
}

// ------------------------------------------------------------------ cleanup

fn cleanup(args: RemoteCleanupArgs) -> Result<()> {
  if args.node_ids.is_empty() {
    return Err(anyhow!(
      "cleanup needs at least one --node-id; take ids from `remote scan --json`"
    ));
  }
  // The policy is granted per command, not per config file: the serve process
  // only ever sees --allow-cleanup when the operator typed --execute.
  let policy = RemotePolicy {
    allow_cleanup: args.execute,
  };
  let remote = connect(&args.target, policy)?;
  let _guard = StopOnDrop(remote.clone());

  let plan = host(remote.cleanup_plan(&args.scan_id, &args.node_ids))?;
  if args.json {
    if args.execute {
      let outcome =
        host(remote.cleanup_execute(plan["planId"].as_str().context("plan returned no planId")?))?;
      print_json(&json!({ "plan": plan, "outcome": outcome }))?;
    } else {
      print_json(&plan)?;
    }
    return Ok(());
  }

  let entries = plan["entries"].as_array().cloned().unwrap_or_default();
  println!(
    "plan {} — {} entries, {}",
    plan["planId"].as_str().unwrap_or("?"),
    entries.len(),
    format_bytes(plan["totalSize"].as_u64().unwrap_or(0))
  );
  for entry in &entries {
    println!(
      "{:>10}  {}",
      format_bytes(entry["size"].as_u64().unwrap_or(0)),
      entry["path"].as_str().unwrap_or("?")
    );
  }
  if !args.execute {
    println!("dry run only; pass --execute to move these to the remote trash.");
    return Ok(());
  }
  let outcome =
    host(remote.cleanup_execute(plan["planId"].as_str().context("plan returned no planId")?))?;
  println!(
    "trashed {} paths, freed {}",
    outcome["trashed"].as_array().map(Vec::len).unwrap_or(0),
    format_bytes(outcome["bytesFreed"].as_u64().unwrap_or(0))
  );
  for failure in outcome["failed"].as_array().cloned().unwrap_or_default() {
    eprintln!("failed: {}", failure);
  }
  Ok(())
}

// ---------------------------------------------------------------- plumbing

fn connect(spec: &str, policy: RemotePolicy) -> Result<Arc<RemoteHost>> {
  let target = SshTarget::parse(spec)?;
  let manifest = load_manifest()?;
  let mut progress = |stage: InstallProgress| {
    eprintln!("[remote:{}] {}", stage.code, stage.message);
  };
  let mut source =
    |manifest: &AgentManifest, triple: &str, progress: &mut dyn FnMut(InstallProgress)| {
      let cache = agent_cache_dir(manifest, triple);
      runtime::download_agent(manifest, triple, cache.as_deref(), progress)
    };
  let remote = RemoteHost::connect_with(target, &manifest, policy, &mut source, &mut progress)
    .map_err(|error| anyhow!(spacelens_remote::error::problem_text(&error)))?;
  Ok(remote)
}

fn load_manifest() -> Result<AgentManifest> {
  if let Ok(path) = std::env::var("SPACLENS_REMOTE_MANIFEST") {
    let text = std::fs::read_to_string(&path).with_context(|| format!("cannot read {path}"))?;
    return Ok(AgentManifest::parse(&text)?);
  }
  Ok(AgentManifest::parse(MANIFEST_JSON)?)
}

/// `$XDG_CACHE_HOME` (or `~/.cache`) `/space-lens/remote-agents/<v>/<triple>`.
fn agent_cache_dir(manifest: &AgentManifest, triple: &str) -> Option<PathBuf> {
  let base = match std::env::var("XDG_CACHE_HOME") {
    Ok(value) => PathBuf::from(value),
    Err(_) => PathBuf::from(std::env::var("HOME").ok()?).join(".cache"),
  };
  Some(
    base
      .join("space-lens")
      .join("remote-agents")
      .join(&manifest.agent.version)
      .join(triple),
  )
}

/// Maps a host refusal (`Err(problem json)`) onto a printable error.
fn host(result: Result<Value, Value>) -> Result<Value> {
  result.map_err(|problem| anyhow!(spacelens_remote::error::problem_text_value(&problem)))
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
  value
    .get(key)
    .and_then(Value::as_str)
    .with_context(|| format!("the host response has no string field {key:?}"))
}
