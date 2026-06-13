use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};
use space_lens::{
  build_removal_plan, execute_removal_plan, find_candidates, scan_directory, CandidateOptions,
  CleanupPreset, IgnoredMode, RemovalPlan, ScanNode, ScanOptions,
};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
  name = "space-lens",
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
  #[arg(long)]
  execute: bool,
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
  }
}

fn run_scan(args: ScanArgs) -> Result<()> {
  let tree = scan_directory(ScanOptions {
    directories: args.paths,
    ignore_hidden: args.ignore_hidden,
    full_path: args.full_path,
    respect_gitignore: args.respect_gitignore,
    ignored_mode: args.ignored_mode.into(),
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
  use super::format_bytes;

  #[test]
  fn formats_bytes_as_human_readable_values() {
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(512), "512 B");
    assert_eq!(format_bytes(1024), "1.0 KiB (1024 bytes)");
    assert_eq!(format_bytes(1_572_864), "1.5 MiB (1572864 bytes)");
  }
}
