//! The one fixed probe command, and the parser for its answer.
//!
//! The probe is a constant POSIX script. No user value is interpolated into it,
//! so there is nothing for a hostile home directory or host name to influence.
//! Every line is prefixed with [`SENTINEL`], which lets a user's shell profile
//! print banners without corrupting the answer.

use std::path::Component;

use serde::Serialize;

use crate::error::RemoteError;
use crate::ssh_config::shell_supported;

pub const SENTINEL: &str = "SPACELENS-PROBE\t";
const MAX_VALUE: usize = 4096;
const MAX_HOME: usize = 4096;

/// Fixed probe script. Kept free of interpolation on purpose.
pub const PROBE_SCRIPT: &str = r#"p() { printf 'SPACELENS-PROBE\t%s=%s\n' "$1" "$2"; }
p os "$(uname -s 2>/dev/null)"
p arch "$(uname -m 2>/dev/null)"
p home "$HOME"
p shell "$SHELL"
p writable "$([ -w "$HOME" ] && printf yes || printf no)"
p node "$(command -v node 2>/dev/null)"
p node_version "$(node --version 2>/dev/null)"
p npm "$(command -v npm 2>/dev/null)"
p npx "$(command -v npx 2>/dev/null)"
p agent "$(ls -1 "$HOME/.space-lens/agents" 2>/dev/null | tail -n 1)"
"#;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProbe {
  pub os: String,
  pub arch: String,
  pub home: String,
  pub shell: String,
  pub writable: bool,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub node: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub node_version: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub npm: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub npx: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub agent_latest: Option<String>,
}

impl RemoteProbe {
  pub fn platform(&self) -> &'static str {
    match self.os.to_ascii_lowercase().as_str() {
      "linux" => "linux",
      "darwin" => "macos",
      _ => "unknown",
    }
  }

  pub fn node_major(&self) -> Option<u32> {
    let version = self.node_version.as_deref()?;
    let version = version.trim().trim_start_matches('v');
    version.split('.').next()?.parse().ok()
  }

  pub fn supports_node(&self, minimum: u32) -> bool {
    self.node.is_some()
      && self.npm.is_some()
      && self.node_major().is_some_and(|major| major >= minimum)
  }

  pub fn shell_supported(&self) -> bool {
    shell_supported(&self.shell)
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoteTarget {
  pub triple: &'static str,
  pub os: &'static str,
  pub arch: String,
}

/// Maps `uname -s` / `uname -m` onto a release target triple. Windows is
/// deliberately absent: Windows OpenSSH defaults to `cmd.exe`, so the POSIX
/// command contract cannot hold there.
pub fn resolve_target(os: &str, arch: &str) -> Result<RemoteTarget, RemoteError> {
  let os = os.trim().to_ascii_lowercase();
  let arch = arch.trim().to_ascii_lowercase();
  let arch = match arch.as_str() {
    "x86_64" | "amd64" | "x64" => "x86_64",
    "aarch64" | "arm64" => "aarch64",
    "armv7l" | "armv7" => "armv7",
    other => other,
  };
  let triple = match (os.as_str(), arch) {
    ("linux", "x86_64") => "x86_64-unknown-linux-musl",
    ("linux", "aarch64") => "aarch64-unknown-linux-musl",
    ("linux", "armv7") => "armv7-unknown-linux-musleabihf",
    ("darwin", "x86_64") => "x86_64-apple-darwin",
    ("darwin", "aarch64") => "aarch64-apple-darwin",
    _ => {
      return Err(RemoteError::UnsupportedTarget(format!(
        "no Space Lens agent for {os}/{arch}"
      )))
    }
  };
  Ok(RemoteTarget {
    triple,
    os: if os == "darwin" { "macos" } else { "linux" },
    arch: arch.to_string(),
  })
}

pub fn parse_probe(stdout: &str) -> Result<RemoteProbe, RemoteError> {
  let mut values = std::collections::HashMap::new();
  let mut lines = 0usize;
  for line in stdout.lines() {
    let Some(rest) = line.strip_prefix(SENTINEL) else {
      continue;
    };
    lines += 1;
    if lines > 64 {
      return Err(RemoteError::Probe(
        "probe printed more than 64 fields".into(),
      ));
    }
    let Some((key, value)) = rest.split_once('=') else {
      continue;
    };
    let value = value.trim_end_matches('\r');
    if value.len() > MAX_VALUE {
      return Err(RemoteError::Probe(format!("probe field {key} is too long")));
    }
    if value.contains('\0') {
      return Err(RemoteError::Probe(format!(
        "probe field {key} contains NUL"
      )));
    }
    values
      .entry(key.to_string())
      .or_insert_with(|| value.to_string());
  }

  let required = |key: &str| -> Result<String, RemoteError> {
    values
      .get(key)
      .filter(|value| !value.is_empty())
      .cloned()
      .ok_or_else(|| RemoteError::Probe(format!("probe did not report {key}")))
  };
  let optional =
    |key: &str| -> Option<String> { values.get(key).filter(|value| !value.is_empty()).cloned() };

  let os = required("os")?;
  let arch = required("arch")?;
  let home = required("home")?;
  let shell = required("shell")?;
  let writable = match required("writable")?.as_str() {
    "yes" => true,
    "no" => false,
    other => {
      return Err(RemoteError::Probe(format!(
        "probe reported an unknown writable value: {other:?}"
      )))
    }
  };
  validate_absolute_path(&home, "home")?;

  Ok(RemoteProbe {
    os,
    arch,
    home,
    shell,
    writable,
    node: optional("node"),
    node_version: optional("node_version"),
    npm: optional("npm"),
    npx: optional("npx"),
    agent_latest: optional("agent"),
  })
}

/// Lexical validation for any absolute path that will be quoted into a
/// remote command or used to build the install layout. Deliberately stricter
/// than Linux: no `.`/`..`, no empty components, no control characters, and no
/// trailing separator except `/` itself.
pub fn validate_absolute_path(path: &str, what: &str) -> Result<(), RemoteError> {
  if path.is_empty() || path.len() > MAX_HOME {
    return Err(RemoteError::InvalidPath(format!(
      "{what} must be 1-{MAX_HOME} bytes"
    )));
  }
  if path.chars().any(char::is_control) {
    return Err(RemoteError::InvalidPath(format!(
      "{what} contains a control character"
    )));
  }
  if !path.starts_with('/') {
    return Err(RemoteError::InvalidPath(format!(
      "{what} is not absolute: {path:?}"
    )));
  }
  if path.len() > 1 && path.ends_with('/') {
    return Err(RemoteError::InvalidPath(format!(
      "{what} has a trailing separator: {path:?}"
    )));
  }
  if path.contains("//") {
    return Err(RemoteError::InvalidPath(format!(
      "{what} has an empty component: {path:?}"
    )));
  }
  if path
    .split('/')
    .any(|component| component == "." || component == "..")
  {
    return Err(RemoteError::InvalidPath(format!(
      "{what} contains . or ..: {path:?}"
    )));
  }
  let parsed = std::path::Path::new(path);
  for component in parsed.components() {
    match component {
      Component::RootDir => {}
      Component::Normal(value) => {
        if value.is_empty() {
          return Err(RemoteError::InvalidPath(format!(
            "{what} has an empty component"
          )));
        }
      }
      Component::CurDir | Component::ParentDir => {
        return Err(RemoteError::InvalidPath(format!(
          "{what} contains . or ..: {path:?}"
        )))
      }
      _ => {
        return Err(RemoteError::InvalidPath(format!(
          "{what} has an unsupported component: {path:?}"
        )))
      }
    }
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn probe_output(lines: &[(&str, &str)]) -> String {
    lines
      .iter()
      .map(|(key, value)| format!("{SENTINEL}{key}={value}\n"))
      .collect()
  }

  fn base() -> Vec<(&'static str, &'static str)> {
    vec![
      ("os", "Linux"),
      ("arch", "x86_64"),
      ("home", "/home/alice"),
      ("shell", "/bin/bash"),
      ("writable", "yes"),
      ("node", "/usr/bin/node"),
      ("node_version", "v20.11.1"),
      ("npm", "/usr/bin/npm"),
      ("npx", "/usr/bin/npx"),
    ]
  }

  #[test]
  fn parses_a_linux_probe() {
    let probe = parse_probe(&probe_output(&base())).unwrap();
    assert_eq!(probe.platform(), "linux");
    assert_eq!(probe.home, "/home/alice");
    assert_eq!(probe.node_major(), Some(20));
    assert!(probe.supports_node(20));
    assert!(!probe.supports_node(22));
    assert!(probe.shell_supported());
  }

  #[test]
  fn profile_noise_is_ignored() {
    let noisy = format!("Welcome to Ubuntu!\n{}last line\n", probe_output(&base()));
    assert!(parse_probe(&noisy).is_ok());
  }

  #[test]
  fn missing_or_malformed_fields_fail_closed() {
    let mut missing = base();
    missing.retain(|(key, _)| *key != "home");
    assert!(parse_probe(&probe_output(&missing)).is_err());

    let mut bad_writable = base();
    bad_writable.retain(|(key, _)| *key != "writable");
    bad_writable.push(("writable", "maybe"));
    assert!(parse_probe(&probe_output(&bad_writable)).is_err());

    let mut relative_home = base();
    relative_home.retain(|(key, _)| *key != "home");
    relative_home.push(("home", "alice"));
    assert!(parse_probe(&probe_output(&relative_home)).is_err());
  }

  #[test]
  fn resolves_release_targets() {
    for (os, arch, triple) in [
      ("Linux", "x86_64", "x86_64-unknown-linux-musl"),
      ("Linux", "amd64", "x86_64-unknown-linux-musl"),
      ("Linux", "aarch64", "aarch64-unknown-linux-musl"),
      ("Linux", "arm64", "aarch64-unknown-linux-musl"),
      ("Darwin", "arm64", "aarch64-apple-darwin"),
      ("Darwin", "x86_64", "x86_64-apple-darwin"),
    ] {
      assert_eq!(resolve_target(os, arch).unwrap().triple, triple);
    }
    assert!(resolve_target("FreeBSD", "x86_64").is_err());
    assert!(resolve_target("Linux", "riscv64").is_err());
  }

  #[test]
  fn absolute_paths_are_lexically_strict() {
    for valid in ["/", "/home/alice", "/home/al ice", "/home/it's"] {
      validate_absolute_path(valid, "root").unwrap_or_else(|error| panic!("{valid}: {error}"));
    }
    for invalid in [
      "",
      "home/alice",
      "/home//alice",
      "/home/../etc",
      "/home/.",
      "/home/",
      "a\u{0}b",
    ] {
      assert!(
        validate_absolute_path(invalid, "root").is_err(),
        "accepted {invalid:?}"
      );
    }
  }

  #[test]
  fn the_probe_script_has_no_interpolation_points() {
    assert!(PROBE_SCRIPT.contains(r"SPACELENS-PROBE\t"));
    assert!(!PROBE_SCRIPT.contains("{}"));
    assert!(!PROBE_SCRIPT.contains("$1="));
  }
}
