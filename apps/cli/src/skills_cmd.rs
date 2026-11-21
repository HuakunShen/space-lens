//! `spacelens skills` — install and update this repository's agent skill.
//!
//! The skill itself lives in the repo (`.agents/skills/space-lens/SKILL.md`),
//! and `skills` (skills.sh) is the ecosystem tool that copies it into each
//! agent's directory and can later update it in place. Rather than
//! re-implementing that, this wraps it: the same `npx skills@latest` calls,
//! with the repository and skill name already filled in, so the operator does
//! not have to remember either.

use anyhow::{anyhow, Result};
use clap::{Args, Subcommand};
use std::process::{Command as Process, Stdio};

/// Where the skill comes from. Kept as flags so a fork or a renamed copy works
/// without rebuilding the binary.
const DEFAULT_REPO: &str = "HuakunShen/space-lens";
const DEFAULT_SKILL: &str = "space-lens";
/// Pinned to `@latest`: the wrapper must follow the ecosystem tool, not a
/// stale copy of it.
const SKILLS_CLI: &str = "skills@latest";

#[derive(Debug, Args)]
pub struct SkillsArgs {
  #[command(subcommand)]
  pub command: SkillsCommand,
}

#[derive(Debug, Subcommand)]
pub enum SkillsCommand {
  /// Add the skill to your agents (global by default).
  Install(InstallArgs),
  /// Update an installed copy to the newest version in the repository.
  Update(ScopeArgs),
  /// Show installed skills.
  List(ListArgs),
  /// Remove the installed skill.
  Remove(RemoveArgs),
}

#[derive(Debug, Args)]
pub struct InstallArgs {
  /// Install into this project's agents instead of your user-level ones.
  #[arg(long)]
  pub project: bool,
  /// Install for these agents only (repeatable; `*` for all).
  #[arg(long, value_name = "AGENT")]
  pub agent: Vec<String>,
  /// Copy the files instead of symlinking them into the agent directories.
  #[arg(long)]
  pub copy: bool,
  /// Skip the confirmation prompt.
  #[arg(long)]
  pub yes: bool,
  /// Repository to install from (`owner/repo`).
  #[arg(long, default_value = DEFAULT_REPO, value_name = "OWNER/REPO")]
  pub repo: String,
  /// Skill to install from that repository.
  #[arg(long, default_value = DEFAULT_SKILL, value_name = "NAME")]
  pub skill: String,
}

#[derive(Debug, Args)]
pub struct ScopeArgs {
  /// Update project-level skills only.
  #[arg(long, conflicts_with = "global")]
  pub project: bool,
  /// Update user-level skills only.
  #[arg(long, conflicts_with = "project")]
  pub global: bool,
  /// The skill to update (defaults to this repository's).
  #[arg(long, default_value = DEFAULT_SKILL, value_name = "NAME")]
  pub skill: String,
  /// Skip the confirmation prompt.
  #[arg(long)]
  pub yes: bool,
}

#[derive(Debug, Args)]
pub struct ListArgs {
  /// List project-level skills instead of user-level ones.
  #[arg(long)]
  pub project: bool,
  /// Machine-readable output.
  #[arg(long)]
  pub json: bool,
}

#[derive(Debug, Args)]
pub struct RemoveArgs {
  /// Remove from your user-level skills instead of this project's.
  #[arg(long)]
  pub global: bool,
  /// The skill to remove (defaults to this repository's).
  #[arg(long, default_value = DEFAULT_SKILL, value_name = "NAME")]
  pub skill: String,
  /// Skip the confirmation prompt.
  #[arg(long)]
  pub yes: bool,
}

/**
 * The `npx` argv for one action. Pure so the mapping is unit-tested without a
 * network call, and visible in the output before it runs.
 */
pub fn skills_argv(command: &SkillsCommand) -> Vec<String> {
  let mut argv: Vec<String> = vec!["-y".into(), SKILLS_CLI.into()];
  let push = |argv: &mut Vec<String>, value: &str| argv.push(value.to_string());
  match command {
    SkillsCommand::Install(args) => {
      argv.extend(["add".into(), args.repo.clone()]);
      argv.push("--skill".into());
      argv.push(args.skill.clone());
      if !args.project {
        argv.push("--global".into());
      }
      for agent in &args.agent {
        argv.push("--agent".into());
        argv.push(agent.clone());
      }
      if args.copy {
        argv.push("--copy".into());
      }
      if args.yes {
        argv.push("--yes".into());
      }
    }
    SkillsCommand::Update(args) => {
      push(&mut argv, "update");
      push(&mut argv, &args.skill);
      if args.project {
        push(&mut argv, "--project");
      }
      if args.global || !args.project {
        push(&mut argv, "--global");
      }
      if args.yes {
        push(&mut argv, "--yes");
      }
    }
    SkillsCommand::List(args) => {
      push(&mut argv, "list");
      if !args.project {
        push(&mut argv, "--global");
      }
      if args.json {
        push(&mut argv, "--json");
      }
    }
    // `skills remove` has no --project: the project is the default scope, so
    // only the global case adds a flag.
    SkillsCommand::Remove(args) => {
      push(&mut argv, "remove");
      push(&mut argv, &args.skill);
      if args.global {
        push(&mut argv, "--global");
      }
      if args.yes {
        push(&mut argv, "--yes");
      }
    }
  }
  argv
}

/// Runs the wrapper. `npx` is the distribution channel the skills ecosystem
/// documents, and it keeps the tool current without a second package to
/// install here.
pub fn run(args: SkillsArgs) -> Result<()> {
  let argv = skills_argv(&args.command);
  eprintln!("running: npx {}", argv.join(" "));
  let status = Process::new("npx")
    .args(&argv)
    .stdin(Stdio::inherit())
    .stdout(Stdio::inherit())
    .stderr(Stdio::inherit())
    .status()
    .map_err(|error| {
      if error.kind() == std::io::ErrorKind::NotFound {
        anyhow!("`npx` was not found; install Node.js 20+ (npm ships npx) and try again")
      } else {
        anyhow!("could not run npx: {error}")
      }
    })?;
  if !status.success() {
    return Err(anyhow!(
      "skills exited with {} (read the messages above). One agent refusing a global \
       install — e.g. one that only supports project scope — fails the run while the \
       others succeed: rerun with --agent <name> (repeatable) to name the agents you \
       want, or --project to install into this directory only",
      status
        .code()
        .map_or_else(|| "a signal".to_string(), |code| code.to_string())
    ));
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn install(args: InstallArgs) -> SkillsCommand {
    SkillsCommand::Install(args)
  }

  fn base_install() -> InstallArgs {
    InstallArgs {
      project: false,
      agent: vec![],
      copy: false,
      yes: false,
      repo: DEFAULT_REPO.into(),
      skill: DEFAULT_SKILL.into(),
    }
  }

  #[test]
  fn install_defaults_to_the_repo_skill_for_the_user() {
    let argv = skills_argv(&install(base_install()));
    assert_eq!(
      argv,
      vec![
        "-y",
        SKILLS_CLI,
        "add",
        "HuakunShen/space-lens",
        "--skill",
        "space-lens",
        "--global"
      ]
    );
  }

  #[test]
  fn install_carries_agents_copy_and_yes() {
    let mut args = base_install();
    args.project = true;
    args.agent = vec!["claude-code".into(), "opencode".into()];
    args.copy = true;
    args.yes = true;
    let argv = skills_argv(&install(args));
    assert!(
      !argv.contains(&"--global".to_string()),
      "project scope wins"
    );
    assert_eq!(
      argv.iter().filter(|value| *value == "--agent").count(),
      2,
      "one --agent per flag"
    );
    assert!(argv.contains(&"--copy".to_string()));
    assert!(argv.contains(&"--yes".to_string()));
  }

  #[test]
  fn update_is_global_by_default_and_can_target_a_project() {
    let global = skills_argv(&SkillsCommand::Update(ScopeArgs {
      project: false,
      global: false,
      skill: DEFAULT_SKILL.into(),
      yes: true,
    }));
    assert_eq!(
      global,
      vec![
        "-y",
        SKILLS_CLI,
        "update",
        "space-lens",
        "--global",
        "--yes"
      ]
    );

    let project = skills_argv(&SkillsCommand::Update(ScopeArgs {
      project: true,
      global: false,
      skill: DEFAULT_SKILL.into(),
      yes: false,
    }));
    assert!(project.contains(&"--project".to_string()));
    assert!(!project.contains(&"--global".to_string()));
  }

  #[test]
  fn list_and_remove_map_their_scope_and_json() {
    let list = skills_argv(&SkillsCommand::List(ListArgs {
      project: false,
      json: true,
    }));
    assert_eq!(list, vec!["-y", SKILLS_CLI, "list", "--global", "--json"]);

    let project = skills_argv(&SkillsCommand::Remove(RemoveArgs {
      global: false,
      skill: DEFAULT_SKILL.into(),
      yes: true,
    }));
    assert_eq!(
      project,
      vec!["-y", SKILLS_CLI, "remove", "space-lens", "--yes"]
    );

    let global = skills_argv(&SkillsCommand::Remove(RemoveArgs {
      global: true,
      skill: DEFAULT_SKILL.into(),
      yes: false,
    }));
    assert!(global.contains(&"--global".to_string()));
  }
}
