use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};
use space_lens::cloud::{
  build_eviction_plan, execute_eviction_plan, inspect_item, EvictionOutcome, EvictionPlan,
  NativeICloudBackend, ScanOptions as CloudScanOptions,
};
use space_lens::{
  build_removal_plan, execute_removal_plan, find_candidates, find_dirty_git_repos, scan_directory,
  CandidateOptions, CleanupPreset, DirtyGitRepoOptions, IgnoredMode, RemovalPlan, ScanNode,
  ScanOptions,
};
use std::io::{self, BufRead};
use std::path::PathBuf;

#[cfg(feature = "mcp")]
mod mcp;

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
  let tree = scan_directory(ScanOptions {
    directories: args.paths,
    ignore_hidden: args.ignore_hidden,
    full_path: args.full_path,
    respect_gitignore: args.respect_gitignore,
    ignored_mode: args.ignored_mode.into(),
    follow_symlinks: args.follow_symlinks,
  });

  if args.json {
    print_json(&tree)?;
  } else {
    for node in &tree {
      print_tree(node, 0);
    }
  }

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
      println!("{}\t{} changed files", repo.path.display(), repo.dirty_entries);
    }
  }

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
  use super::{format_bytes, Cli, Command, ICloudCommand};
  use clap::Parser;
  use std::path::Path;

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
}
