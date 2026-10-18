use anyhow::Result;
use base64::Engine as _;
use clap::{Args, Parser, Subcommand, ValueEnum};
use memory_cmd::MemoryCommand;
use space_lens::cloud::{
  build_eviction_plan, execute_eviction_plan, inspect_item, EvictionOutcome, EvictionPlan,
  NativeICloudBackend, ScanOptions as CloudScanOptions,
};
use space_lens::{
  build_removal_plan, execute_removal_plan, find_candidates, find_dirty_git_repos, scan_directory,
  scan_local_directory_with_progress, CandidateOptions, CleanupPreset, DirtyGitRepoOptions,
  IgnoredMode, LocalScanCancellation, LocalScanProgress, LocalScanReport, RemovalPlan, ScanNode,
  ScanOptions,
};
use spacelens_app_audit::{audit_installed_apps, AppAuditOptions};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

#[cfg(feature = "mcp")]
mod mcp;

mod memory_cmd;

#[derive(Debug, Parser)]
#[command(
  name = "spacelens",
  version,
  about = "Scan disk usage and cleanup candidates."
)]
struct Cli {
  #[command(subcommand)]
  command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
  Scan(ScanArgs),
  Candidates(CandidateArgs),
  Clean(CleanArgs),
  #[command(name = "dirty-git")]
  DirtyGit(DirtyGitArgs),
  #[command(name = "icloud")]
  ICloud(ICloudArgs),
  #[command(name = "app-audit")]
  AppAudit(AppAuditArgs),
  #[command(name = "app-icon")]
  AppIcon(AppIconArgs),
  #[command(subcommand)]
  Memory(MemoryCommand),
  #[cfg(feature = "mcp")]
  Mcp,
}

#[derive(Debug, Args)]
struct ICloudArgs {
  #[command(subcommand)]
  command: ICloudCommand,
}

#[derive(Debug, Subcommand)]
enum ICloudCommand {
  Inspect(ICloudInspectArgs),
  Plan(ICloudPlanArgs),
  Evict(ICloudEvictArgs),
}

#[derive(Debug, Args)]
struct ICloudInspectArgs {
  #[arg(value_name = "PATH")]
  path: PathBuf,
  #[arg(long)]
  json: bool,
}

#[derive(Debug, Args)]
struct ICloudPlanArgs {
  #[arg(value_name = "PATH")]
  path: PathBuf,
  #[arg(long)]
  json: bool,
}

#[derive(Debug, Args)]
struct ICloudEvictArgs {
  #[arg(value_name = "PATH")]
  path: PathBuf,
  #[arg(long)]
  execute: bool,
  #[arg(long)]
  json: bool,
}

#[derive(Debug, Args)]
struct ScanArgs {
  #[arg(value_name = "PATH", default_value = ".")]
  paths: Vec<PathBuf>,
  #[arg(long)]
  json: bool,
  #[arg(long)]
  ignore_hidden: bool,
  #[arg(long)]
  full_path: bool,
  #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
  respect_gitignore: bool,
  #[arg(long, value_enum, default_value_t = IgnoredModeArg::Summarize)]
  ignored_mode: IgnoredModeArg,
  #[arg(long, help = "Descend into symlinked directories")]
  follow_symlinks: bool,
  #[arg(
    long,
    help = "Protected local scan: skip cloud domains, dataless items, mounts and symlinks"
  )]
  local_only: bool,
  #[arg(long, requires_all = ["local_only", "json"], help = "Emit progress and terminal report as NDJSON")]
  progress_json: bool,
  #[arg(long, requires_all = ["local_only", "progress_json"], help = "Emit each node separately before the terminal coverage report")]
  stream_nodes: bool,
}

#[derive(Debug, Args)]
struct CandidateArgs {
  #[arg(value_name = "PATH", default_value = ".")]
  paths: Vec<PathBuf>,
  #[arg(long, value_enum)]
  preset: Vec<PresetArg>,
  #[arg(long)]
  json: bool,
  #[arg(long)]
  ignore_hidden: bool,
  #[arg(long, help = "Descend into symlinked directories")]
  follow_symlinks: bool,
}

#[derive(Debug, Args)]
struct CleanArgs {
  #[arg(value_name = "PATH", default_value = ".")]
  paths: Vec<PathBuf>,
  #[arg(long, value_enum)]
  preset: Vec<PresetArg>,
  #[arg(long)]
  json: bool,
  #[arg(long)]
  ignore_hidden: bool,
  #[arg(long, help = "Descend into symlinked directories")]
  follow_symlinks: bool,
  #[arg(long)]
  execute: bool,
}

#[derive(Debug, Args)]
struct DirtyGitArgs {
  #[arg(value_name = "PATH", default_value = ".")]
  paths: Vec<PathBuf>,
  #[arg(long)]
  json: bool,
  #[arg(long)]
  ignore_hidden: bool,
  #[arg(long, help = "Descend into symlinked directories")]
  follow_symlinks: bool,
}

#[derive(Debug, Args)]
struct AppAuditArgs {
  #[arg(long = "min-size", default_value_t = 0)]
  min_size: u64,
  #[arg(
    long,
    conflicts_with = "no_heuristic",
    help = "Include apps without curated catalog entries (default on)"
  )]
  all: bool,
  #[arg(
    long = "no-heuristic",
    conflicts_with = "all",
    help = "Exclude apps without curated catalog entries"
  )]
  no_heuristic: bool,
  #[arg(long, default_value_t = 500)]
  limit: usize,
  #[arg(long, value_name = "PATH")]
  home: Option<PathBuf>,
  #[arg(long)]
  json: bool,
}

#[derive(Debug, Args)]
struct AppIconArgs {
  #[arg(long, value_name = "PATH")]
  path: PathBuf,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum IgnoredModeArg {
  Exclude,
  Summarize,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum PresetArg {
  Node,
  Rust,
  Gitignored,
}

fn main() -> Result<()> {
  let cli = Cli::parse();

  match cli.command {
    Command::Scan(args) => run_scan(args),
    Command::Candidates(args) => run_candidates(args),
    Command::Clean(args) => run_clean(args),
    Command::DirtyGit(args) => run_dirty_git(args),
    Command::ICloud(args) => run_icloud(args),
    Command::AppAudit(args) => run_app_audit(args),
    Command::AppIcon(args) => run_app_icon(args),
    Command::Memory(command) => memory_cmd::run(command),
    #[cfg(feature = "mcp")]
    Command::Mcp => mcp::run(),
  }
}

fn run_icloud(args: ICloudArgs) -> Result<()> {
  let backend = NativeICloudBackend;

  match args.command {
    ICloudCommand::Inspect(args) => {
      let info = inspect_item(&backend, &args.path)?;
      if args.json {
        print_json(&info)?;
      } else {
        println!("path\t{}", args.path.display());
        println!("kind\t{:?}", info.kind());
        println!("iCloud\t{}", info.is_icloud);
        println!("download state\t{:?}", info.download_state);
        println!(
          "logical bytes\t{}",
          format_bytes(info.fingerprint.logical_bytes)
        );
        println!(
          "local allocation\t{}",
          info
            .allocated_bytes
            .map(|bytes| format_bytes(bytes.0))
            .unwrap_or_else(|| "unknown".to_string())
        );
      }
    }
    ICloudCommand::Plan(args) => {
      let plan = build_eviction_plan(&backend, &args.path, CloudScanOptions::default())?;
      print_icloud_plan(&plan, args.json)?;
    }
    ICloudCommand::Evict(args) => {
      let plan = build_eviction_plan(&backend, &args.path, CloudScanOptions::default())?;
      if !args.execute {
        print_icloud_plan(&plan, args.json)?;
        return Ok(());
      }

      if args.json {
        eprintln!(
          "iCloud plan: {} candidates, {} local allocation estimate",
          plan.candidates.len(),
          format_bytes(plan.total_allocated_bytes())
        );
      } else {
        print_icloud_plan(&plan, false)?;
      }
      confirm_icloud_eviction()?;
      let outcome = execute_eviction_plan(&backend, &plan)?;
      if args.json {
        print_json(&outcome)?;
      } else {
        print_icloud_outcome(&outcome);
      }
    }
  }

  Ok(())
}

fn print_icloud_plan(plan: &EvictionPlan, json: bool) -> Result<()> {
  if json {
    print_json(plan)?;
    return Ok(());
  }

  println!(
    "iCloud plan: {} candidates, {} local allocation estimate",
    plan.candidates.len(),
    format_bytes(plan.total_allocated_bytes())
  );
  println!(
    "logical bytes in candidates\t{}",
    format_bytes(plan.total_logical_bytes())
  );
  println!("cloud-only files\t{}", plan.cloud_only.len());
  println!("skipped items\t{}", plan.skipped.len());
  println!("coverage complete\t{}", plan.coverage_complete);

  for candidate in &plan.candidates {
    println!(
      "candidate\t{}\t{}",
      format_bytes(candidate.allocated_bytes.0),
      candidate.path.display()
    );
  }

  Ok(())
}

fn print_icloud_outcome(outcome: &EvictionOutcome) {
  for result in &outcome.results {
    match &result.error {
      Some(error) => println!("{:?}\t{}\t{}", result.status, result.path.display(), error),
      None => println!("{:?}\t{}", result.status, result.path.display()),
    }
  }
}

fn confirm_icloud_eviction() -> Result<()> {
  eprintln!("This requests removal of local iCloud copies; it does not delete cloud files.");
  eprintln!("Type EVICT-LOCAL-COPIES to continue:");
  let mut input = String::new();
  io::stdin().lock().read_line(&mut input)?;
  if input.trim() != "EVICT-LOCAL-COPIES" {
    anyhow::bail!("eviction cancelled: confirmation phrase did not match");
  }
  Ok(())
}

fn run_scan(args: ScanArgs) -> Result<()> {
  let options = ScanOptions {
    directories: args.paths,
    ignore_hidden: args.ignore_hidden,
    full_path: args.full_path,
    respect_gitignore: args.respect_gitignore,
    ignored_mode: args.ignored_mode.into(),
    follow_symlinks: args.follow_symlinks,
  };

  if args.local_only {
    let cancellation = LocalScanCancellation::default();
    let writer = std::sync::Mutex::new(io::BufWriter::new(io::stdout()));
    let output_error = std::sync::Mutex::new(None::<String>);
    let report = scan_local_directory_with_progress(options, cancellation.clone(), |progress| {
      if args.progress_json {
        if let Err(error) = write_ndjson(
          &mut *writer.lock().unwrap(),
          &ProgressFrame {
            kind: "progress",
            progress,
          },
        ) {
          *output_error.lock().unwrap() = Some(error.to_string());
          cancellation.cancel();
        }
      }
    });
    if let Some(error) = output_error.into_inner().unwrap() {
      anyhow::bail!("progress output failed: {error}");
    }
    let mut report = report?;
    if args.progress_json {
      if args.stream_nodes {
        // Move each subtree through an explicit stack: no tree clone or giant JSON frame.
        let mut pending = std::mem::take(&mut report.nodes);
        pending.reverse();
        while let Some(mut node) = pending.pop() {
          let children = std::mem::take(&mut node.children);
          write_ndjson_buffered(
            &mut *writer.lock().unwrap(),
            &NodeFrame {
              kind: "node",
              node: &node,
            },
          )?;
          pending.extend(children.into_iter().rev());
        }
      }
      write_ndjson(
        &mut *writer.lock().unwrap(),
        &DoneFrame {
          kind: "done",
          report: &report,
        },
      )?;
    } else if args.json {
      print_json(&report)?;
    } else {
      for node in report.nodes {
        print_tree(&ScanNode::from(node), 0);
      }
      eprintln!("Local-only coverage: {} skipped, {} denied, {} issues. Sizes are allocated blocks, not guaranteed reclaimable space.", report.coverage.skipped_count, report.coverage.denied_count, report.coverage.issue_count);
    }
    return Ok(());
  }
  let tree = scan_directory(options);

  if args.json {
    print_json(&tree)?;
  } else {
    for node in &tree {
      print_tree(node, 0);
    }
  }

  Ok(())
}

#[derive(serde::Serialize)]
struct ProgressFrame {
  #[serde(rename = "type")]
  kind: &'static str,
  progress: LocalScanProgress,
}

#[derive(serde::Serialize)]
struct DoneFrame<'a> {
  #[serde(rename = "type")]
  kind: &'static str,
  report: &'a LocalScanReport,
}

#[derive(serde::Serialize)]
struct NodeFrame<'a> {
  #[serde(rename = "type")]
  kind: &'static str,
  node: &'a space_lens::LocalScanNode,
}

fn write_ndjson<W: Write, T: serde::Serialize>(writer: &mut W, value: &T) -> Result<()> {
  write_ndjson_buffered(writer, value)?;
  writer.flush()?;
  Ok(())
}

fn write_ndjson_buffered<W: Write, T: serde::Serialize>(writer: &mut W, value: &T) -> Result<()> {
  serde_json::to_writer(&mut *writer, value)?;
  writer.write_all(b"\n")?;
  Ok(())
}

fn run_candidates(args: CandidateArgs) -> Result<()> {
  let candidates = find_candidates(CandidateOptions {
    roots: args.paths,
    presets: args.preset.into_iter().map(CleanupPreset::from).collect(),
    ignore_hidden: args.ignore_hidden,
    follow_symlinks: args.follow_symlinks,
  });

  if args.json {
    print_json(&candidates)?;
  } else {
    for candidate in &candidates {
      println!(
        "{}\t{}\t{}\t{}",
        format_bytes(candidate.size),
        preset_name(candidate.preset),
        candidate.path.display(),
        candidate.reason
      );
    }
  }

  Ok(())
}

fn run_clean(args: CleanArgs) -> Result<()> {
  let candidates = find_candidates(CandidateOptions {
    roots: args.paths,
    presets: args.preset.into_iter().map(CleanupPreset::from).collect(),
    ignore_hidden: args.ignore_hidden,
    follow_symlinks: args.follow_symlinks,
  });
  let plan = build_removal_plan(candidates);

  if args.execute {
    let outcome = execute_removal_plan(&plan);
    if args.json {
      print_json(&outcome)?;
    } else {
      println!(
        "removed {} paths, {}",
        outcome.removed.len(),
        format_bytes(outcome.bytes_removed)
      );
      for error in &outcome.errors {
        eprintln!("error: {error}");
      }
    }
  } else if args.json {
    print_json(&plan)?;
  } else {
    print_plan(&plan);
  }

  Ok(())
}

fn run_dirty_git(args: DirtyGitArgs) -> Result<()> {
  let repos = find_dirty_git_repos(DirtyGitRepoOptions {
    roots: args.paths,
    ignore_hidden: args.ignore_hidden,
    follow_symlinks: args.follow_symlinks,
  });

  if args.json {
    print_json(&repos)?;
  } else if repos.is_empty() {
    println!("No dirty git repositories found.");
  } else {
    for repo in &repos {
      println!(
        "{}\t{} changed files",
        repo.path.display(),
        repo.dirty_entries
      );
    }
  }

  Ok(())
}

fn new_audit_id() -> String {
  use std::collections::hash_map::DefaultHasher;
  use std::hash::{Hash, Hasher};
  use std::time::{SystemTime, UNIX_EPOCH};
  static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
  let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
  let now = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default();
  let pid = std::process::id();
  // `audit_` + 24 hex chars (hex is alphanumeric, satisfying the contract
  // regex `audit_[0-9a-zA-Z]{8,64}`); the counter keeps ids unique even when
  // two audits share a timestamp. Mirrors the desktop host's generator.
  let mut first = DefaultHasher::new();
  (now.as_secs(), now.subsec_nanos(), pid, counter).hash(&mut first);
  let mut second = DefaultHasher::new();
  (counter, pid, now.subsec_nanos(), now.as_secs()).hash(&mut second);
  format!(
    "audit_{:016x}{:08x}",
    first.finish(),
    second.finish() & 0xFFFF_FFFF
  )
}

fn run_app_audit(args: AppAuditArgs) -> Result<()> {
  if args.limit == 0 || args.limit > 1000 {
    anyhow::bail!("app-audit limit must be between 1 and 1000");
  }
  let options = AppAuditOptions {
    min_size: args.min_size,
    include_heuristic: !args.no_heuristic,
    limit: args.limit,
    home: args.home,
  };
  let report =
    audit_installed_apps(&options).map_err(|error| anyhow::anyhow!("app audit failed: {error}"))?;
  let audit_id = new_audit_id();
  let mut value = serde_json::to_value(&report)?;
  value
    .as_object_mut()
    .ok_or_else(|| anyhow::anyhow!("audit report did not serialize to an object"))?
    .insert(
      "auditId".to_string(),
      serde_json::Value::String(audit_id.clone()),
    );
  if args.json {
    print_json(&value)?;
  } else {
    let apps = value
      .get("apps")
      .and_then(|apps| apps.as_array())
      .cloned()
      .unwrap_or_default();
    println!(
      "app audit {}: {} apps (showing {})",
      audit_id,
      value.get("totalApps").and_then(|v| v.as_u64()).unwrap_or(0),
      apps.len()
    );
    for app in &apps {
      println!(
        "{}\t{}\t{}",
        app.get("totalSize").and_then(|v| v.as_u64()).unwrap_or(0),
        app.get("name").and_then(|v| v.as_str()).unwrap_or("?"),
        app.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
      );
    }
  }
  Ok(())
}

fn run_app_icon(args: AppIconArgs) -> Result<()> {
  let png = spacelens_app_audit::app_icon_png(&args.path);
  let Some(png) = png else {
    anyhow::bail!("no icon available for this app: {}", args.path.display());
  };
  let data_url = format!(
    "data:image/png;base64,{}",
    base64::engine::general_purpose::STANDARD.encode(&png)
  );
  print_json(&serde_json::json!({ "dataUrl": data_url }))?;
  Ok(())
}

fn print_tree(node: &ScanNode, indent: usize) {
  let prefix = "  ".repeat(indent);
  println!("{prefix}{}\t{}", node.name, format_bytes(node.size));
  for child in &node.children {
    print_tree(child, indent + 1);
  }
}

fn print_plan(plan: &RemovalPlan) {
  println!(
    "dry run: {} paths, {} would be removed",
    plan.entries.len(),
    format_bytes(plan.total_size)
  );
  for entry in &plan.entries {
    println!(
      "{}\t{}\t{}\t{}",
      format_bytes(entry.size),
      preset_name(entry.preset),
      entry.path.display(),
      entry.reason
    );
  }
}

fn format_bytes(bytes: u64) -> String {
  const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];

  if bytes < 1024 {
    return format!("{bytes} B");
  }

  let mut value = bytes as f64;
  let mut unit = 0;

  while value >= 1024.0 && unit < UNITS.len() - 1 {
    value /= 1024.0;
    unit += 1;
  }

  format!("{value:.1} {} ({bytes} bytes)", UNITS[unit])
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
  serde_json::to_writer_pretty(std::io::stdout(), value)?;
  println!();
  Ok(())
}

fn preset_name(preset: CleanupPreset) -> &'static str {
  match preset {
    CleanupPreset::Node => "node",
    CleanupPreset::Rust => "rust",
    CleanupPreset::Gitignored => "gitignored",
  }
}

impl From<IgnoredModeArg> for IgnoredMode {
  fn from(mode: IgnoredModeArg) -> Self {
    match mode {
      IgnoredModeArg::Exclude => IgnoredMode::Exclude,
      IgnoredModeArg::Summarize => IgnoredMode::Summarize,
    }
  }
}

impl From<PresetArg> for CleanupPreset {
  fn from(preset: PresetArg) -> Self {
    match preset {
      PresetArg::Node => CleanupPreset::Node,
      PresetArg::Rust => CleanupPreset::Rust,
      PresetArg::Gitignored => CleanupPreset::Gitignored,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::{format_bytes, new_audit_id, Cli, Command, ICloudCommand};
  use clap::Parser;
  use std::path::Path;

  #[test]
  fn parses_protected_scan_protocol_and_rejects_progress_on_legacy_scans() {
    assert!(Cli::try_parse_from([
      "spacelens",
      "scan",
      "/tmp/local-fixture",
      "--local-only",
      "--json",
      "--progress-json"
    ])
    .is_ok());
    assert!(
      Cli::try_parse_from(["spacelens", "scan", "/tmp/local-fixture", "--progress-json"]).is_err()
    );
  }

  #[test]
  fn formats_bytes_as_human_readable_values() {
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(512), "512 B");
    assert_eq!(format_bytes(1024), "1.0 KiB (1024 bytes)");
    assert_eq!(format_bytes(1_572_864), "1.5 MiB (1572864 bytes)");
  }

  #[test]
  fn parses_icloud_plan_as_a_read_only_command() {
    let cli = Cli::try_parse_from(["spacelens", "icloud", "plan", "/tmp/test"]).unwrap();

    assert!(matches!(
      cli.command,
      Command::ICloud(super::ICloudArgs {
        command: ICloudCommand::Plan(args),
      }) if args.path == Path::new("/tmp/test")
    ));
  }

  #[test]
  fn evict_is_dry_run_without_execute_flag() {
    let cli = Cli::try_parse_from(["spacelens", "icloud", "evict", "/tmp/test"]).unwrap();

    assert!(matches!(
      cli.command,
      Command::ICloud(super::ICloudArgs {
        command: ICloudCommand::Evict(args),
      }) if !args.execute
    ));
  }

  #[test]
  fn parses_app_audit_defaults_and_heuristic_switches() {
    let cli = Cli::try_parse_from(["spacelens", "app-audit", "--json"]).unwrap();
    assert!(matches!(
      cli.command,
      Command::AppAudit(args) if args.min_size == 0 && !args.no_heuristic && !args.all && args.limit == 500 && args.json && args.home.is_none()
    ));
    let cli = Cli::try_parse_from([
      "spacelens",
      "app-audit",
      "--min-size",
      "1024",
      "--no-heuristic",
      "--limit",
      "10",
      "--json",
    ])
    .unwrap();
    assert!(matches!(
      cli.command,
      Command::AppAudit(args) if args.min_size == 1024 && args.no_heuristic && args.limit == 10 && args.json
    ));
    let cli = Cli::try_parse_from(["spacelens", "app-audit", "--all", "--json"]).unwrap();
    assert!(matches!(
      cli.command,
      Command::AppAudit(args) if args.all && !args.no_heuristic
    ));
    // --all and --no-heuristic conflict.
    assert!(Cli::try_parse_from(["spacelens", "app-audit", "--all", "--no-heuristic"]).is_err());
  }

  #[test]
  fn parses_app_icon_path() {
    let cli =
      Cli::try_parse_from(["spacelens", "app-icon", "--path", "/Applications/Foo.app"]).unwrap();
    assert!(matches!(
      cli.command,
      Command::AppIcon(args) if args.path == Path::new("/Applications/Foo.app")
    ));
    assert!(Cli::try_parse_from(["spacelens", "app-icon"]).is_err());
  }

  #[test]
  fn audit_ids_match_the_contract_shape() {
    for _ in 0..16 {
      let id = new_audit_id();
      assert!(id.starts_with("audit_"), "id: {id}");
      let suffix = &id["audit_".len()..];
      assert!(
        (8..=64).contains(&suffix.len()) && suffix.chars().all(|c| c.is_ascii_alphanumeric()),
        "id: {id}"
      );
    }
    assert_ne!(new_audit_id(), new_audit_id());
  }
}
