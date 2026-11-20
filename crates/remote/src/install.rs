//! Provisioning vocabulary shared by the desktop runtime and its tests.
//!
//! Stages are closed and ordered; a caller publishes them verbatim, so the UI
//! never has to translate a free-form message into "where did it stop?".

use serde::{Deserialize, Serialize};

use crate::error::RemoteError;
use crate::server::RemoteShell;

pub const AGENT_DIR: &str = ".space-lens/agents";
pub const HOST_DIR: &str = ".space-lens/host";
pub const REMOTE_DIR: &str = ".space-lens/remote";

/// The agent binary's file name on one platform. Windows ships `spacelens.exe`
/// in its release archive, and the install layout must land on that exact
/// name or the smoke test would look for a binary that was never there.
pub fn agent_binary_name(shell: RemoteShell) -> &'static str {
  match shell {
    RemoteShell::Posix => "spacelens",
    RemoteShell::WindowsPwsh => "spacelens.exe",
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallStage {
  Probe,
  Resolve,
  Download,
  Verify,
  AgentInstall,
  HostCheck,
  ServeStart,
  Tunnel,
  Pair,
  Ready,
}

impl std::fmt::Display for InstallStage {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(self.code())
  }
}

impl InstallStage {
  pub fn code(self) -> &'static str {
    match self {
      Self::Probe => "probe",
      Self::Resolve => "resolve",
      Self::Download => "download",
      Self::Verify => "verify",
      Self::AgentInstall => "agent-install",
      Self::HostCheck => "host-check",
      Self::ServeStart => "serve-start",
      Self::Tunnel => "tunnel",
      Self::Pair => "pair",
      Self::Ready => "ready",
    }
  }

  /// Display uses the same closed code the UI receives.
  pub fn as_str(self) -> &'static str {
    self.code()
  }

  /// Whether repeating the same action can plausibly succeed. A hash mismatch
  /// is not retryable: the same bytes will fail the same check.
  pub fn retryable(self) -> bool {
    matches!(
      self,
      Self::Probe | Self::Download | Self::HostCheck | Self::ServeStart | Self::Tunnel | Self::Pair
    )
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgress {
  pub stage: InstallStage,
  pub code: &'static str,
  pub retryable: bool,
  pub message: String,
}

impl InstallProgress {
  pub fn new(stage: InstallStage, message: impl Into<String>) -> Self {
    Self {
      stage,
      code: stage.code(),
      retryable: stage.retryable(),
      message: message.into(),
    }
  }
}

/// Absolute remote paths for one agent version. The shape is per-platform:
/// POSIX homes are `/`-rooted and the binary is `spacelens`; Windows homes are
/// canonical drive paths (`C:/Users/…`) and the binary is `spacelens.exe`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLayout {
  pub home: String,
  pub version: String,
  pub version_dir: String,
  pub binary: String,
  pub staging_dir: String,
}

impl AgentLayout {
  /// POSIX layout: `/`-rooted home and the `spacelens` binary.
  pub fn new(home: &str, version: &str, staging_suffix: &str) -> Result<Self, RemoteError> {
    Self::for_shell(home, version, staging_suffix, RemoteShell::Posix)
  }

  /// Windows layout: canonical `C:/` home and the `spacelens.exe` binary.
  pub fn windows(home: &str, version: &str, staging_suffix: &str) -> Result<Self, RemoteError> {
    Self::for_shell(home, version, staging_suffix, RemoteShell::WindowsPwsh)
  }

  /// Builds the layout for one platform's path rules and binary name.
  pub fn for_shell(
    home: &str,
    version: &str,
    staging_suffix: &str,
    shell: RemoteShell,
  ) -> Result<Self, RemoteError> {
    shell.validate_path(home, "home")?;
    validate_component("version", version)?;
    validate_component("staging suffix", staging_suffix)?;
    let version_dir = format!("{home}/{AGENT_DIR}/{version}");
    Ok(Self {
      home: home.to_string(),
      version: version.to_string(),
      binary: format!("{version_dir}/{}", agent_binary_name(shell)),
      staging_dir: format!("{home}/{AGENT_DIR}/.staging-{staging_suffix}"),
      version_dir,
    })
  }
}

/// Absolute remote paths for one host-package version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostLayout {
  pub home: String,
  pub version: String,
  pub prefix: String,
  pub entry: String,
  pub pid_file: String,
}

impl HostLayout {
  /// POSIX layout: `/`-rooted home.
  pub fn new(home: &str, version: &str) -> Result<Self, RemoteError> {
    Self::for_shell(home, version, RemoteShell::Posix)
  }

  /// Windows layout: canonical `C:/` home.
  pub fn windows(home: &str, version: &str) -> Result<Self, RemoteError> {
    Self::for_shell(home, version, RemoteShell::WindowsPwsh)
  }

  /// Builds the layout for one platform's path rules.
  pub fn for_shell(home: &str, version: &str, shell: RemoteShell) -> Result<Self, RemoteError> {
    shell.validate_path(home, "home")?;
    validate_component("host version", version)?;
    let prefix = format!("{home}/{HOST_DIR}/{version}");
    Ok(Self {
      home: home.to_string(),
      version: version.to_string(),
      entry: format!("{prefix}/node_modules/@space-lens/cli/dist/cli.mjs"),
      pid_file: format!("{home}/{REMOTE_DIR}/{version}/server.pid"),
      prefix,
    })
  }
}

fn validate_component(what: &str, value: &str) -> Result<(), RemoteError> {
  if value.is_empty()
    || value.len() > 64
    || !value
      .chars()
      .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    || value.starts_with('.')
  {
    return Err(RemoteError::InvalidPath(format!(
      "{what} must be a safe path component: {value:?}"
    )));
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn stages_are_closed_and_ordered() {
    let stages = [
      InstallStage::Probe,
      InstallStage::Resolve,
      InstallStage::Download,
      InstallStage::Verify,
      InstallStage::AgentInstall,
      InstallStage::HostCheck,
      InstallStage::ServeStart,
      InstallStage::Tunnel,
      InstallStage::Pair,
      InstallStage::Ready,
    ];
    let codes: Vec<&str> = stages.iter().map(|stage| stage.code()).collect();
    assert_eq!(
      codes,
      vec![
        "probe",
        "resolve",
        "download",
        "verify",
        "agent-install",
        "host-check",
        "serve-start",
        "tunnel",
        "pair",
        "ready"
      ]
    );
    let json = serde_json::to_string(&InstallProgress::new(InstallStage::Tunnel, "x")).unwrap();
    assert!(json.contains("\"stage\":\"tunnel\""));
  }

  #[test]
  fn layouts_are_absolute_and_versioned() {
    let agent = AgentLayout::new("/home/alice", "0.4.0", "abc123").unwrap();
    assert_eq!(
      agent.binary,
      "/home/alice/.space-lens/agents/0.4.0/spacelens"
    );
    assert_eq!(
      agent.staging_dir,
      "/home/alice/.space-lens/agents/.staging-abc123"
    );
    let host = HostLayout::new("/home/alice", "0.4.0").unwrap();
    assert_eq!(
      host.entry,
      "/home/alice/.space-lens/host/0.4.0/node_modules/@space-lens/cli/dist/cli.mjs"
    );
    assert_eq!(
      host.pid_file,
      "/home/alice/.space-lens/remote/0.4.0/server.pid"
    );
  }

  #[test]
  fn layout_components_cannot_escape_the_home() {
    assert!(AgentLayout::new("/home/alice", "../../etc", "x").is_err());
    assert!(AgentLayout::new("/home/alice", "0.4.0", "../x").is_err());
    assert!(HostLayout::new("/home/alice", ".hidden").is_err());
    assert!(AgentLayout::new("relative", "0.4.0", "x").is_err());
  }

  #[test]
  fn windows_layouts_land_on_spacelens_exe() {
    let agent = AgentLayout::windows("C:/Users/shenh", "0.4.0", "abc123").unwrap();
    assert_eq!(
      agent.binary,
      "C:/Users/shenh/.space-lens/agents/0.4.0/spacelens.exe"
    );
    assert_eq!(
      agent.staging_dir,
      "C:/Users/shenh/.space-lens/agents/.staging-abc123"
    );
    let host = HostLayout::windows("C:/Users/shenh", "0.4.0").unwrap();
    assert_eq!(
      host.entry,
      "C:/Users/shenh/.space-lens/host/0.4.0/node_modules/@space-lens/cli/dist/cli.mjs"
    );
    assert_eq!(
      host.pid_file,
      "C:/Users/shenh/.space-lens/remote/0.4.0/server.pid"
    );
  }

  #[test]
  fn windows_layouts_refuse_posix_paths_and_backslashes() {
    assert!(AgentLayout::windows("C:\\Users\\shenh", "0.4.0", "x").is_err());
    assert!(AgentLayout::windows("/home/alice", "0.4.0", "x").is_err());
    assert!(HostLayout::windows("C:\\Users\\shenh", "0.4.0").is_err());
    assert!(HostLayout::windows("shenh", "0.4.0").is_err());
    // The dispatch constructor enforces the same split.
    assert!(AgentLayout::for_shell("/home/alice", "0.4.0", "x", RemoteShell::WindowsPwsh).is_err());
    assert!(HostLayout::for_shell("C:/Users/shenh", "0.4.0", RemoteShell::Posix).is_err());
    // Component rules are platform-independent.
    assert!(AgentLayout::windows("C:/Users/shenh", "../../etc", "x").is_err());
    assert!(HostLayout::windows("C:/Users/shenh", ".hidden").is_err());
  }
}
