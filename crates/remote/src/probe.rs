//! The one fixed probe command, and the parser for its answer.
//!
//! The probe is a constant script. No user value is interpolated into it,
//! so there is nothing for a hostile home directory or host name to influence.
//! Every line is prefixed with [`SENTINEL`], which lets a user's shell profile
//! print banners without corrupting the answer. There is one script per
//! command channel: [`PROBE_SCRIPT`] for POSIX login shells and
//! [`PROBE_SCRIPT_PWSH`] for Windows, whose default ssh shell is cmd.exe.

use std::path::Component;

use serde::Serialize;

use crate::error::RemoteError;
use crate::ssh_config::shell_supported;

pub const SENTINEL: &str = "SPACELENS-PROBE\t";
const MAX_VALUE: usize = 4096;
const MAX_HOME: usize = 4096;

/// Fixed probe script. Kept free of interpolation on purpose.
///
/// `node`/`npm` fall back to the install locations a non-interactive SSH
/// shell misses (nvm, Homebrew, volta, pnpm, user-local trees): remote
/// provisioning must not depend on an interactive PATH the login shell never
/// sourced.
pub const PROBE_SCRIPT: &str = r#"p() { printf 'SPACELENS-PROBE\t%s=%s\n' "$1" "$2"; }
p os "$(uname -s 2>/dev/null)"
p arch "$(uname -m 2>/dev/null)"
p home "$HOME"
p shell "$SHELL"
p writable "$([ -w "$HOME" ] && printf yes || printf no)"
n="$(command -v node 2>/dev/null)"
m="$(command -v npm 2>/dev/null)"
if [ -z "$n" ]; then
  for d in "$HOME"/.nvm/versions/node/*/bin /opt/homebrew/bin /usr/local/bin "$HOME/.local/bin" "$HOME/.local/share/vite-plus/bin" "$HOME/.volta/bin" "$HOME/Library/pnpm"; do
    if [ -x "$d/node" ]; then
      n="$d/node"
      [ -n "$m" ] || m="$d/npm"
      break
    fi
  done
fi
p node "$n"
p node_version "$("$n" --version 2>/dev/null)"
p npm "$m"
p npx "$(command -v npx 2>/dev/null)"
p agent "$(ls -1 "$HOME/.space-lens/agents" 2>/dev/null | tail -n 1)"
"#;

/// Fixed probe script for the Windows command channel. Kept free of
/// interpolation on purpose, exactly like [`PROBE_SCRIPT`].
///
/// Every path this script reports is normalized to the crate's canonical
/// forward-slash form (`C:/Users/shenh`): the runtime, the layouts, and every
/// later remote script share that one form, and node, npm, `[IO.*]` and the
/// PowerShell cmdlets all accept it. `$lensHome` is deliberately not `$home`:
/// PowerShell variable names are case-insensitive, so `$home` would collide
/// with the automatic `$HOME` variable.
pub const PROBE_SCRIPT_PWSH: &str = r#"function p([string]$key, [string]$value) {
  Write-Output "SPACELENS-PROBE`t$key=$value"
}
$lensHome = $env:USERPROFILE.Replace('\', '/')
p os 'Windows'
p arch $env:PROCESSOR_ARCHITECTURE
p home $lensHome
p shell 'powershell'
$probeDir = if ($env:TEMP) { $env:TEMP } else { $env:USERPROFILE }
$probeFile = [IO.Path]::Combine($probeDir, 'space-lens-probe.tmp')
try {
  [IO.File]::WriteAllText($probeFile, 'probe')
  [IO.File]::Delete($probeFile)
  p writable 'yes'
} catch {
  p writable 'no'
}
$node = Get-Command node -ErrorAction SilentlyContinue
$npm = Get-Command npm -ErrorAction SilentlyContinue
$npx = Get-Command npx -ErrorAction SilentlyContinue
$nodePath = if ($node) { $node.Source.Replace('\', '/') } else { '' }
$npmPath = if ($npm) { $npm.Source.Replace('\', '/') } else { '' }
$npxPath = if ($npx) { $npx.Source.Replace('\', '/') } else { '' }
p node $nodePath
p node_version (if ($node) { & $nodePath --version })
p npm $npmPath
p npx $npxPath
$agentsDir = [IO.Path]::Combine($lensHome, '.space-lens/agents')
$agentLatest = if (Test-Path -LiteralPath $agentsDir) { (Get-ChildItem -LiteralPath $agentsDir -Directory | Sort-Object Name -Descending | Select-Object -First 1).Name }
p agent $agentLatest
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
      "windows" => "windows",
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

/// Maps the probe's `os`/`arch` onto a release target triple. Windows is
/// reached only through the PowerShell command channel: the probe that
/// answers `Windows` is the one already wrapped in
/// [`PROBE_SCRIPT_PWSH`](crate::probe::PROBE_SCRIPT_PWSH), so the runtime
/// knows the shell flavor that produced this answer.
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
    ("windows", "x86_64") => "x86_64-pc-windows-msvc",
    ("windows", "aarch64") => "aarch64-pc-windows-msvc",
    _ => {
      return Err(RemoteError::UnsupportedTarget(format!(
        "no Space Lens agent for {os}/{arch}"
      )))
    }
  };
  // The triple match above is exhaustive over linux/darwin/windows, so the
  // fallback arm is reached only for linux.
  let os = match os.as_str() {
    "darwin" => "macos",
    "windows" => "windows",
    _ => "linux",
  };
  Ok(RemoteTarget {
    triple,
    os,
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
  // The home is validated with the rules of the platform the probe itself
  // declared: a Windows home is the canonical `C:/…` form, and a POSIX
  // answer can never claim to be Windows (its script prints `uname`).
  if os.eq_ignore_ascii_case("windows") {
    validate_windows_path(&home, "home")?;
  } else {
    validate_absolute_path(&home, "home")?;
  }

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

/// Lexical validation for a Windows path in the crate's canonical
/// forward-slash form (`C:/Users/shenh`).
///
/// The runtime stores every Windows path this way — node, npm, `[IO.*]` and
/// the PowerShell cmdlets all accept it — so backslashes are refused outright
/// rather than tolerated and half-normalized. Beyond that the rules mirror
/// [`validate_absolute_path`]: a drive-letter root, no `.`/`..`, no empty
/// components, no control characters, and no trailing separator except the
/// drive root `C:/` itself.
pub fn validate_windows_path(path: &str, what: &str) -> Result<(), RemoteError> {
  let invalid = |message: String| Err(RemoteError::InvalidPath(format!("{what} {message}")));
  if path.is_empty() || path.len() > MAX_HOME {
    return invalid(format!("must be 1-{MAX_HOME} bytes"));
  }
  if path.len() < 3 {
    return invalid(format!("is not a drive-absolute path: {path:?}"));
  }
  let bytes = path.as_bytes();
  if !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' || bytes[2] != b'/' {
    return invalid(format!("is not a drive-absolute path: {path:?}"));
  }
  if path.chars().any(char::is_control) {
    return invalid("contains a control character".into());
  }
  if path.contains('\\') {
    return invalid(format!("must use forward slashes only: {path:?}"));
  }
  if path.len() > 3 && path.ends_with('/') {
    return invalid(format!("has a trailing separator: {path:?}"));
  }
  if path.len() > 3
    && path[3..]
      .split('/')
      .any(|component| component.is_empty() || component == "." || component == "..")
  {
    return invalid(format!("has an unsafe component: {path:?}"));
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

  fn windows_output() -> Vec<(&'static str, &'static str)> {
    vec![
      ("os", "Windows"),
      ("arch", "AMD64"),
      ("home", "C:/Users/shenh"),
      ("shell", "powershell"),
      ("writable", "yes"),
      ("node", "C:/Program Files/nodejs/node.exe"),
      ("node_version", "v26.5.0"),
      ("npm", "C:/Program Files/nodejs/npm.cmd"),
      ("npx", "C:/Program Files/nodejs/npx.cmd"),
    ]
  }

  #[test]
  fn parses_a_windows_probe() {
    let probe = parse_probe(&probe_output(&windows_output())).unwrap();
    assert_eq!(probe.platform(), "windows");
    assert_eq!(probe.home, "C:/Users/shenh");
    assert_eq!(probe.node_major(), Some(26));
    assert!(probe.shell_supported());
    assert!(probe.supports_node(20));

    let target = resolve_target(&probe.os, &probe.arch).unwrap();
    assert_eq!(target.triple, "x86_64-pc-windows-msvc");
    assert_eq!(target.os, "windows");
    assert_eq!(target.arch, "x86_64");
  }

  #[test]
  fn windows_probe_lines_tolerate_crlf() {
    // PowerShell 5.1 writes CRLF; the probe answers through it.
    let raw: String = probe_output(&windows_output())
      .lines()
      .map(|line| format!("{line}\r\n"))
      .collect();
    let probe = parse_probe(&raw).unwrap();
    assert_eq!(probe.home, "C:/Users/shenh");
  }

  #[test]
  fn windows_home_validation_is_stricter_than_posix() {
    for invalid in ["C:\\Users\\shenh", "shenh", "/home/alice", "C:/Users/"] {
      let mut rows = windows_output();
      rows.retain(|(key, _)| *key != "home");
      rows.push(("home", invalid));
      assert!(
        parse_probe(&probe_output(&rows)).is_err(),
        "accepted home {invalid:?}"
      );
    }
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
      ("Windows", "AMD64", "x86_64-pc-windows-msvc"),
      ("Windows", "ARM64", "aarch64-pc-windows-msvc"),
    ] {
      assert_eq!(resolve_target(os, arch).unwrap().triple, triple);
      let expected_os = match os {
        "Darwin" => "macos",
        "Windows" => "windows",
        _ => "linux",
      };
      assert_eq!(resolve_target(os, arch).unwrap().os, expected_os);
    }
    assert!(resolve_target("FreeBSD", "x86_64").is_err());
    assert!(resolve_target("Linux", "riscv64").is_err());
    assert!(resolve_target("Windows", "riscv64").is_err());
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
  fn windows_paths_are_lexically_strict() {
    for valid in [
      "C:/",
      "C:/Users/shenh",
      "C:/Users/al ice",
      "D:/x",
      "c:/Users/shenh",
    ] {
      validate_windows_path(valid, "root").unwrap_or_else(|error| panic!("{valid}: {error}"));
    }
    for invalid in [
      "",
      "C:",
      "C",
      "C:\\Users\\shenh",
      "C:/Users//shenh",
      "C:/Users/../Windows",
      "C:/Users/.",
      "C:/Users/",
      "C:/Users/sh\u{0}enh",
      "C://",
      "/home/alice",
      "1:/x",
      "Cä:/x",
    ] {
      assert!(
        validate_windows_path(invalid, "root").is_err(),
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

  #[test]
  fn the_powershell_probe_script_reports_the_windows_contract() {
    // The same constant-script rule as the POSIX probe: no user value is
    // interpolated, and every answer line goes through the sentinel.
    assert!(PROBE_SCRIPT_PWSH.contains("SPACELENS-PROBE"));
    assert!(PROBE_SCRIPT_PWSH.contains("p os 'Windows'"));
    assert!(PROBE_SCRIPT_PWSH.contains("p shell 'powershell'"));
    assert!(PROBE_SCRIPT_PWSH.contains("$env:PROCESSOR_ARCHITECTURE"));
    // Paths are normalized to the canonical forward-slash form at the source.
    assert!(PROBE_SCRIPT_PWSH.contains(".Replace('\\', '/')"));
    assert!(PROBE_SCRIPT_PWSH.contains("'.space-lens/agents'"));
    assert!(!PROBE_SCRIPT_PWSH.contains("{}"));
  }
}
