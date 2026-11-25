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

use crate::remote_interactive::LineReader as _;
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
  /// Trash specific folders (venvs, build outputs, models) — dry run by default.
  Trash(RemoteTrashArgs),
  /// Connect and print a browser URL for the remote workbench.
  Open {
    /// An alias from ~/.ssh/config, or `user@host[:port]`.
    #[arg(value_name = "TARGET")]
    target: String,
  },
}

#[derive(Debug, Args)]
pub struct RemoteScanArgs {
  /// An alias from ~/.ssh/config, or `user@host[:port]`; omit to pick from a list.
  #[arg(value_name = "TARGET")]
  pub target: Option<String>,
  /// Folder on the remote; omit to be asked (Enter = home, `b` = browse by size).
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
pub struct RemoteTrashArgs {
  /// An alias from ~/.ssh/config, or `user@host[:port]`.
  #[arg(value_name = "TARGET")]
  pub target: String,
  /// Folder(s) to move to the remote trash. Repeatable.
  #[arg(long = "path", value_name = "PATH", required = true)]
  pub paths: Vec<String>,
  /// Actually move them; without this the plan is printed only.
  #[arg(long)]
  pub execute: bool,
  /// Machine-readable plan/outcome.
  #[arg(long)]
  pub json: bool,
}

#[derive(Debug, Args)]
pub struct RemoteCleanupArgs {
  /// An alias from ~/.ssh/config, or `user@host[:port]`; omit to pick from a list.
  #[arg(value_name = "TARGET")]
  pub target: Option<String>,
  /// Folder on the remote to scan for cleanup candidates; omit to be asked.
  #[arg(long, value_name = "PATH")]
  pub path: Option<String>,
  /// Where candidates come from; `caches` finds node_modules, Cargo target
  /// and friends, `gitignored` finds ignored paths.
  #[arg(long, value_enum, default_value_t = DiscoveryKindArg::Caches)]
  pub kind: DiscoveryKindArg,
  /// Move the planned nodes to the remote trash. Without this flag the
  /// command only prints the plan.
  #[arg(long)]
  pub execute: bool,
  /// Full plan/outcome as JSON instead of a table.
  #[arg(long)]
  pub json: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum DiscoveryKindArg {
  #[value(name = "large-files")]
  LargeFiles,
  #[default]
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
    RemoteCommand::Trash(args) => trash(args),
    RemoteCommand::Open { target } => open(&target),
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
  let interactive = crate::remote_interactive::stdin_is_interactive();
  let Some(target) = resolve_target(args.target.as_deref(), interactive)? else {
    println!("cancelled.");
    return Ok(());
  };
  let policy = RemotePolicy::read_only();
  let remote = connect(&target, policy)?;
  let _guard = StopOnDrop(remote.clone());

  let path = match &args.path {
    Some(path) => path.clone(),
    None => {
      let roots = host(remote.roots())?;
      let home = roots["roots"]
        .get(0)
        .and_then(|root| root.get("path"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("the remote host reported no scan roots"))?;
      if !interactive || args.json {
        home
      } else {
        match ask_path(&remote, &home, interactive)? {
          Some(chosen) => chosen,
          None => {
            println!("cancelled.");
            return Ok(());
          }
        }
      }
    }
  };
  if !args.json {
    eprintln!("scanning {path} on {target} …");
  }
  let scan = host(remote.scan_start(&ScanStart {
    paths: vec![path.clone()],
    ..ScanStart::default()
  }))?;
  let scan_id = text(&scan, "scanId")?.to_string();
  wait_ready(&remote, &scan_id, args.json)?;

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
  println!("{target} — largest entries under {path}");
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
}

/// Polls one scan until it leaves the `scanning` state, printing progress
/// unless quiet. Returns the terminal status.
fn wait_ready(remote: &Arc<RemoteHost>, scan_id: &str, quiet: bool) -> Result<Value> {
  let mut last_state = String::new();
  loop {
    let status = host(remote.scan_status(scan_id))?;
    let state = status["state"].as_str().unwrap_or_default().to_string();
    if state != last_state {
      last_state = state.clone();
      if !quiet {
        eprintln!("scan {scan_id}: {state}");
      }
    }
    match state.as_str() {
      "ready" | "failed" | "cancelled" => return Ok(status),
      _ => {
        if !quiet {
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
  // Planning is a read-only computation, but the host gates even `cleanup/plan`
  // behind serve-time `--allow-cleanup`, so this one-shot session grants it.
  // The destructive half stays behind the operator's `--execute`.
  let interactive = crate::remote_interactive::stdin_is_interactive();
  let Some(target) = resolve_target(args.target.as_deref(), interactive)? else {
    println!("cancelled.");
    return Ok(());
  };
  let mut execute = args.execute;
  let policy = RemotePolicy {
    allow_cleanup: true,
  };
  let remote = connect(&target, policy)?;
  let _guard = StopOnDrop(remote.clone());

  // A scan session lives and dies with the remote host process, so the plan is
  // derived in this one session: scan, discover, plan, optionally execute.
  let path = match &args.path {
    Some(path) => path.clone(),
    None => {
      let roots = host(remote.roots())?;
      let home = roots["roots"]
        .get(0)
        .and_then(|root| root.get("path"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("the remote host reported no scan roots"))?;
      if interactive {
        match ask_path(&remote, &home, interactive)? {
          Some(chosen) => chosen,
          None => {
            println!("cancelled.");
            return Ok(());
          }
        }
      } else {
        home
      }
    }
  };
  if !args.json {
    eprintln!("scanning {path} on {target} …");
  }
  let scan = host(remote.scan_start(&ScanStart {
    paths: vec![path.clone()],
    ..ScanStart::default()
  }))?;
  let scan_id = text(&scan, "scanId")?.to_string();
  wait_ready(&remote, &scan_id, args.json)?;

  let page = host(remote.discovery(&scan_id, args.kind.wire(), 0, 0, 1000))?;
  let items = page["items"].as_array().cloned().unwrap_or_default();
  let node_ids: Vec<String> = items
    .iter()
    .filter_map(|item| item["node"]["id"].as_str().map(str::to_string))
    .collect();
  if node_ids.is_empty() {
    println!(
      "no {} candidates under {path}; nothing to plan",
      args.kind.wire()
    );
    return Ok(());
  }

  let plan = host(remote.cleanup_plan(&scan_id, &node_ids))?;
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
  if interactive && !execute && !args.json {
    println!("move these to the remote trash? (y/N)");
    let mut input = crate::remote_interactive::StdinLines;
    if let Some(answer) = input.next_line() {
      execute =
        answer.trim().eq_ignore_ascii_case("y") || answer.trim().eq_ignore_ascii_case("yes");
    }
    if !execute {
      println!("dry run only; rerun and confirm (or pass --execute) to move them.");
      return Ok(());
    }
  } else if !args.execute {
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

// ------------------------------------------------------------------- trash

/// Splits an absolute folder path into the folder to scan and the entry to
/// look for in it. Both separators are honoured because a Windows remote sends
/// `C:\Users\…` while the POSIX ones send `/`.
fn split_parent(path: &str) -> Option<(String, String)> {
  let trimmed = path.trim_end_matches(['/', '\\']);
  // Only absolute paths: a relative one has no folder the host serves, and the
  // remote command would otherwise scan the remote process's cwd by accident.
  let absolute = trimmed.starts_with('/') || trimmed.as_bytes().get(1) == Some(&b':');
  if !absolute {
    return None;
  }
  let index = trimmed.rfind(['/', '\\'])?;
  let (parent, name) = (&trimmed[..index], &trimmed[index + 1..]);
  if parent.is_empty() || name.is_empty() {
    return None;
  }
  Some((parent.to_string(), name.to_string()))
}

/**
 * Moves named folders (a venv, a build output, a model cache) to the remote
 * trash. The host can only plan against a folder it has in a scan, so each
 * target's parent is scanned, the entry is resolved by name, and the plan runs
 * on that node — the same path the workbench takes. Dry run by default.
 *
 * Targets should be leaf folders: the parent is scanned with the ordinary
 * engine, so pointing at a folder inside a huge tree costs that tree's walk.
 */
fn trash(args: RemoteTrashArgs) -> Result<()> {
  let policy = RemotePolicy {
    allow_cleanup: true,
  };
  let remote = connect(&args.target, policy)?;
  let _guard = StopOnDrop(remote.clone());

  let mut by_parent: std::collections::BTreeMap<String, Vec<String>> =
    std::collections::BTreeMap::new();
  for path in &args.paths {
    let (parent, name) =
      split_parent(path).with_context(|| format!("{path} is not an absolute folder path"))?;
    by_parent.entry(parent).or_default().push(name);
  }

  let mut plans: Vec<(String, Value)> = Vec::new();
  let mut plan_lines: Vec<Value> = Vec::new();
  let mut outcomes: Vec<Value> = Vec::new();
  let mut freed = 0u64;
  let mut trashed = 0usize;
  let mut failures = 0usize;
  for (parent, names) in &by_parent {
    if !args.json {
      eprintln!("scanning {parent} on {} …", args.target);
    }
    let scan = host(remote.scan_start(&ScanStart {
      paths: vec![parent.clone()],
      ..ScanStart::default()
    }))?;
    let scan_id = text(&scan, "scanId")?.to_string();
    let status = wait_ready(&remote, &scan_id, true)?;
    let root_id = status["rootIds"]
      .get(0)
      .and_then(Value::as_str)
      .context("the ready scan reported no root id")?
      .to_string();
    let page = host(remote.tree_children(&scan_id, &root_id, 0, 1000, "size"))?;
    let items = page["items"].as_array().cloned().unwrap_or_default();
    let mut node_ids: Vec<String> = Vec::new();
    for name in names {
      match items
        .iter()
        .find(|item| item["name"].as_str() == Some(name.as_str()))
      {
        Some(item) => node_ids.push(item["id"].as_str().unwrap_or_default().to_string()),
        None => eprintln!("  {name} is not in {parent}; skipped"),
      }
    }
    if node_ids.is_empty() {
      continue;
    }
    let plan = host(remote.cleanup_plan(&scan_id, &node_ids))?;
    plan_lines.push(plan.clone());
    // Execute inside the loop: the host keeps at most 8 scan sessions, so a
    // batch that planned everything first would hit the limit and lose the
    // work it had already done.
    if args.execute {
      let outcome =
        host(remote.cleanup_execute(plan["planId"].as_str().context("plan returned no planId")?))?;
      freed += outcome["bytesFreed"].as_u64().unwrap_or(0);
      trashed += outcome["trashed"].as_array().map(Vec::len).unwrap_or(0);
      failures += outcome["failed"].as_array().map(Vec::len).unwrap_or(0);
      outcomes.push(outcome);
    }
    plans.push((scan_id, plan));
  }

  if plan_lines.is_empty() {
    println!("nothing to plan");
    return Ok(());
  }

  let total_entries: usize = plan_lines
    .iter()
    .map(|plan| plan["entries"].as_array().map(Vec::len).unwrap_or(0))
    .sum();
  let total_bytes: u64 = plan_lines
    .iter()
    .map(|plan| plan["totalSize"].as_u64().unwrap_or(0))
    .sum();

  if args.json {
    print_json(&json!({
      "plans": plan_lines,
      "outcomes": outcomes,
      "freed": freed,
      "trashed": trashed,
      "failed": failures,
    }))?;
    return Ok(());
  }

  for plan in &plan_lines {
    println!(
      "plan {} — {} entries, {}",
      plan["planId"].as_str().unwrap_or("?"),
      plan["entries"].as_array().map(Vec::len).unwrap_or(0),
      format_bytes(plan["totalSize"].as_u64().unwrap_or(0)),
    );
    for entry in plan["entries"].as_array().into_iter().flatten() {
      println!(
        "{:>10}  {}",
        format_bytes(entry["size"].as_u64().unwrap_or(0)),
        entry["path"].as_str().unwrap_or("?")
      );
    }
  }

  if !args.execute {
    println!(
      "dry run: {total_entries} entries, {}; pass --execute to move them to the remote trash.",
      format_bytes(total_bytes)
    );
    return Ok(());
  }
  println!(
    "trashed {trashed} paths, freed {}{}",
    format_bytes(freed),
    if failures == 0 {
      String::new()
    } else {
      format!(" ({failures} failed)")
    }
  );
  Ok(())
}

// ------------------------------------------------------------------- open

/// Connects one device and prints the URL that opens its workbench in a
/// browser through the tunnel. The session stays alive until interrupted;
/// a ticket minted for the tunnel's own loopback origin is what lets the
/// browser pair even though the host gates every other origin.
fn open(spec: &str) -> Result<()> {
  let remote = connect(spec, RemotePolicy::read_only())?;
  let origin = format!("http://127.0.0.1:{}", remote.local_port());
  let ticket = host(remote.mint_browser_ticket(&origin))?;
  let url = match ticket.get("url").and_then(Value::as_str) {
    Some(url) => url.to_string(),
    None => format!("{origin}/?pair={}", text(&ticket, "ticket")?),
  };
  // The remote host serves local sessions (loopback bind, no password), so
  // the plain URL opens and reopens without a pairing code: the ticket is only
  // what admits the tunnel's origin, and a fallback if a host refuses.
  println!("Open this URL in a browser — no pairing code needed:");
  println!();
  println!("  {origin}/");
  println!();
  println!("One-time ticket, if this host still asks for one:\n\n  {url}");
  println!();
  println!(
    "This window holds the SSH tunnel. Close it (Ctrl-C) when done; the\n\
     remote host is reaped automatically on the next connect."
  );
  // stdout is block-buffered away from a terminal; the URL must be visible
  // the moment it exists, not when the next buffer fills.
  use std::io::Write as _;
  std::io::stdout().flush().ok();
  // Park until the operator closes the window; the process dying with the
  // terminal is the expected end, and connect reaps the remote pid next time.
  loop {
    std::thread::sleep(Duration::from_secs(3600));
  }
}

// ---------------------------------------------------------------- plumbing

/// The ssh target: an explicit argument, an interactive pick, or an error
/// that explains both. `None` means the operator cancelled the picker.
fn resolve_target(explicit: Option<&str>, interactive: bool) -> Result<Option<String>> {
  match explicit {
    Some(target) => Ok(Some(target.to_string())),
    None if interactive => {
      let mut input = crate::remote_interactive::StdinLines;
      crate::remote_interactive::pick_host(&mut input)
    }
    None => Err(anyhow!(
      "pass an ssh target (alias or user@host[:port]) — or run from a terminal to pick one"
    )),
  }
}

/// Asks for the folder to work on: Enter accepts the reported home, `b`
/// starts a scan of it and drills by size, anything else is a typed path the
/// host validates on use. `None` cancels. Browsing a huge root loads its
/// whole tree on the remote host and can exhaust its memory, so the
/// reported root size is shown and a very large browse asks twice.
fn ask_path(remote: &Arc<RemoteHost>, home: &str, interactive: bool) -> Result<Option<String>> {
  if !interactive {
    return Ok(Some(home.to_string()));
  }
  let roots = host(remote.roots())?;
  let home_size = roots["roots"]
    .as_array()
    .and_then(|roots| roots.first())
    .and_then(|root| root.get("size"))
    .and_then(Value::as_u64)
    .unwrap_or(0);
  const BROWSE_WARN_BYTES: u64 = 50 * 1024 * 1024 * 1024;
  let size_note = if home_size > 0 {
    format!(" ({})", format_bytes(home_size))
  } else {
    String::new()
  };
  println!(
    "Which folder? Enter = {home}{size_note}, `b` = browse sizes first, or type a path (q cancels)"
  );
  let mut input = crate::remote_interactive::StdinLines;
  let Some(line) = input.next_line() else {
    return Ok(None);
  };
  let line = line.trim();
  match line {
    "" => Ok(Some(home.to_string())),
    "q" | "quit" => Ok(None),
    "b" | "browse" => {
      if home_size > BROWSE_WARN_BYTES {
        println!(
          "{} is {}; browsing scans it first and loads the whole tree on the remote host, which can exhaust its memory.",
          home,
          format_bytes(home_size),
        );
        println!("browse anyway? (y/N)");
        match input.next_line() {
          Some(answer) if answer.trim().eq_ignore_ascii_case("y") => {}
          _ => return Ok(None),
        }
      }
      eprintln!("scanning {home} to browse its sizes …");
      let scan = host(remote.scan_start(&ScanStart {
        paths: vec![home.to_string()],
        ..ScanStart::default()
      }))?;
      let scan_id = text(&scan, "scanId")?.to_string();
      let status = wait_ready(remote, &scan_id, false)?;
      let root_id = status["rootIds"]
        .get(0)
        .and_then(Value::as_str)
        .context("the ready scan reported no root id")?
        .to_string();
      crate::remote_interactive::browse_path(remote, &scan_id, &root_id, home, &mut input)
    }
    typed => Ok(Some(typed.to_string())),
  }
}

/// Connects one SSH device with the pinned manifest and the CLI's agent
/// cache. Shared with the MCP adapter, which keeps sessions alive across
/// tool calls instead of stopping after one command.
pub(crate) fn connect(spec: &str, policy: RemotePolicy) -> Result<Arc<RemoteHost>> {
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

/// The ssh-config listing for the devices tool: alias + resolved destination.
#[cfg_attr(not(feature = "mcp"), allow(dead_code))]
pub(crate) fn ssh_hosts() -> Vec<(String, String)> {
  let Ok(home) = std::env::var("HOME") else {
    return Vec::new();
  };
  spacelens_remote::ssh_config::load(&PathBuf::from(home))
    .hosts
    .into_iter()
    .map(|host| {
      let destination = match (&host.user, &host.host_name, host.port) {
        (Some(user), Some(name), Some(port)) => format!("{user}@{name}:{port}"),
        (Some(user), Some(name), None) => format!("{user}@{name}"),
        (_, _, Some(port)) => format!("{}:{port}", host.alias),
        (_, _, None) => host.alias.clone(),
      };
      (host.alias, destination)
    })
    .collect()
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

#[cfg(test)]
mod trash_tests {
  use super::split_parent;

  #[test]
  fn parents_split_on_both_separators() {
    assert_eq!(
      split_parent("/home/hk/dev/2048AI/models"),
      Some(("/home/hk/dev/2048AI".to_string(), "models".to_string()))
    );
    // A Windows remote sends backslashes; the parent must not become `C:`.
    assert_eq!(
      split_parent(r"C:\Users\shenh\proj\node_modules"),
      Some((
        r"C:\Users\shenh\proj".to_string(),
        "node_modules".to_string()
      ))
    );
    // Trailing separators are not part of the name.
    assert_eq!(
      split_parent("/home/hk/dev/videos/.venvs/"),
      Some(("/home/hk/dev/videos".to_string(), ".venvs".to_string()))
    );
  }

  #[test]
  fn relative_and_root_paths_are_refused() {
    assert_eq!(
      split_parent("dev/node_modules"),
      None,
      "a relative path has no folder to serve"
    );
    assert_eq!(split_parent("/"), None);
    assert_eq!(
      split_parent("/home"),
      None,
      "the root itself has no parent to scan"
    );
  }
}
