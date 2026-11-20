//! Local `ssh` argv, the remote serve command, and readiness parsing.
//!
//! Only this module turns a [`SshTarget`] into a process. The local argv stops
//! at `ssh`: the target is one argument, and the remote command is one already
//! quoted string. Nothing here starts a local shell.

use serde::Deserialize;

use crate::error::RemoteError;
use crate::install::HostLayout;
use crate::probe::{validate_absolute_path, PROBE_SCRIPT};
use crate::quote::{quote_posix, shell_script};
use crate::target::SshTarget;

/// The host resolves its Rust sidecar from this variable; without it the
/// installed npm package would search build directories that exist only in a
/// checkout, and protected scans, memory, and app audits would fail.
pub const LOCAL_SCAN_BIN_ENV: &str = "SPACELENS_LOCAL_SCAN_BIN";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshCommand {
  pub program: String,
  pub args: Vec<String>,
}

/// Options fixed by the execution policy. Every entry is a decision:
///
/// - `BatchMode=yes` and `NumberOfPasswordPrompts=0`: no prompt can ever appear
///   in a GUI-less child; a key/agent miss fails instead of hanging.
/// - `StrictHostKeyChecking=yes`: the app never accepts a key on the user's
///   behalf. The UI explains the one-time `ssh <target>` step.
/// - `ClearAllForwardings=yes`: an exec command opens no port, even when the
///   user's config asks for one. Tunnel commands omit it because it would also
///   clear the `-L` we are about to request.
/// - `PermitLocalCommand=no`: a `LocalCommand` in `~/.ssh/config` must not run.
const EXEC_OPTIONS: &[&str] = &[
  "-T",
  "-o",
  "BatchMode=yes",
  "-o",
  "NumberOfPasswordPrompts=0",
  "-o",
  "StrictHostKeyChecking=yes",
  "-o",
  "ConnectTimeout=15",
  "-o",
  "ServerAliveInterval=15",
  "-o",
  "ServerAliveCountMax=3",
  "-o",
  "ClearAllForwardings=yes",
  "-o",
  "ForwardAgent=no",
  "-o",
  "ForwardX11=no",
  "-o",
  "PermitLocalCommand=no",
];

/// Tunnel options deliberately exclude `ClearAllForwardings`.
const TUNNEL_OPTIONS: &[&str] = &[
  "-N",
  "-T",
  "-o",
  "BatchMode=yes",
  "-o",
  "NumberOfPasswordPrompts=0",
  "-o",
  "StrictHostKeyChecking=yes",
  "-o",
  "ConnectTimeout=15",
  "-o",
  "ServerAliveInterval=15",
  "-o",
  "ServerAliveCountMax=3",
  "-o",
  "ForwardAgent=no",
  "-o",
  "ForwardX11=no",
  "-o",
  "PermitLocalCommand=no",
  "-o",
  "ExitOnForwardFailure=yes",
];

fn base(target: &SshTarget, options: &[&str]) -> Result<SshCommand, RemoteError> {
  target.validate()?;
  let mut args: Vec<String> = options.iter().map(|value| (*value).to_string()).collect();
  if let Some(port) = target.port() {
    args.push("-p".into());
    args.push(port.to_string());
  }
  args.push(target.destination());
  Ok(SshCommand {
    program: "ssh".into(),
    args,
  })
}

/// One remote command, executed without a PTY.
pub fn ssh_exec(target: &SshTarget, remote_command: &str) -> Result<SshCommand, RemoteError> {
  let mut command = base(target, EXEC_OPTIONS)?;
  command.args.push(remote_command.to_string());
  Ok(command)
}

/// One SSH local-forward session: `127.0.0.1:local` → `127.0.0.1:remote`.
///
/// The local side is loopback only, and the remote side is the host's own
/// loopback. No port is opened on any external interface.
pub fn ssh_tunnel(
  target: &SshTarget,
  local_port: u16,
  remote_port: u16,
) -> Result<SshCommand, RemoteError> {
  if local_port == 0 || remote_port == 0 {
    return Err(RemoteError::InvalidTarget(
      "tunnel ports must be 1-65535".into(),
    ));
  }
  let mut command = base(target, TUNNEL_OPTIONS)?;
  command.args.push("-L".into());
  command
    .args
    .push(format!("127.0.0.1:{local_port}:127.0.0.1:{remote_port}"));
  Ok(command)
}

/// Runs the fixed probe script.
pub fn ssh_probe(target: &SshTarget) -> Result<SshCommand, RemoteError> {
  ssh_exec(target, &shell_script(PROBE_SCRIPT)?)
}

/// Writes bytes from local stdin to `remote_path` through `cat`.
pub fn ssh_transfer(target: &SshTarget, remote_path: &str) -> Result<SshCommand, RemoteError> {
  validate_absolute_path(remote_path, "transfer path")?;
  // The redirection must sit outside the quoted word: `exec cat > '<path>'`.
  let quoted_path = quote_posix(remote_path)?;
  ssh_exec(target, &format!("exec cat > {quoted_path}"))
}

/// The mutation surface one client is willing to grant a remote host.
///
/// The host binary itself decides what it allows (`--allow-cleanup` absent
/// means read-only); this value is the client's half of the contract, and the
/// runtime enforces it again before any socket is opened.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RemotePolicy {
  /// Grant the paired session trash-only cleanup. The host refuses permanent
  /// deletion by construction; enabling this only unlocks `cleanup/plan` and
  /// `cleanup/execute`.
  pub allow_cleanup: bool,
}

impl RemotePolicy {
  pub fn read_only() -> Self {
    Self {
      allow_cleanup: false,
    }
  }
}

/// The remote script that starts the pinned host package.
///
/// `npx` is deliberately not used at run time: `npm install --prefix` gives a
/// stable entry point and a single node process whose pid is the one written to
/// the pid file. That makes disconnect cleanup exact instead of best-effort
/// process-tree guessing.
pub fn serve_script(
  host: &HostLayout,
  agent_binary: &str,
  node: &str,
  home: &str,
  policy: &RemotePolicy,
) -> Result<String, RemoteError> {
  validate_absolute_path(agent_binary, "agent binary")?;
  validate_absolute_path(node, "node")?;
  validate_absolute_path(home, "home")?;
  let environment = quote_posix(&format!("{LOCAL_SCAN_BIN_ENV}={agent_binary}"))?;
  let cleanup = if policy.allow_cleanup {
    " --allow-cleanup"
  } else {
    ""
  };
  let script = format!(
    "mkdir -p {pid_dir} && printf '%s\\n' $$ > {pid_file} && exec env {environment} {node} {entry} serve --machine --allow-memory{cleanup} --root {root}",
    pid_dir = quote_posix(&format!("{}/{}/{}", host.home, crate::install::REMOTE_DIR, host.version))?,
    pid_file = quote_posix(&host.pid_file)?,
    node = quote_posix(node)?,
    entry = quote_posix(&host.entry)?,
    root = quote_posix(home)?,
  );
  shell_script(&script)
}

/// The remote script that installs the pinned host package without sudo.
pub fn host_install_script(host: &HostLayout, npm: &str) -> Result<String, RemoteError> {
  validate_absolute_path(npm, "npm")?;
  let script = format!(
    "mkdir -p {prefix} && exec {npm} install --prefix {prefix} --no-audit --no-fund --loglevel=error '{package}@{version}'",
    prefix = quote_posix(&host.prefix)?,
    npm = quote_posix(npm)?,
    package = crate::manifest::PACKAGE,
    version = host.version,
  );
  shell_script(&script)
}

/// Installs the host package from a tarball already on the remote filesystem.
///
/// Development and acceptance testing use this with a local `npm pack` result
/// transferred over the same SSH session, so a remote host can be exercised
/// before the package version is published to the registry.
pub fn host_install_tarball_script(
  host: &HostLayout,
  npm: &str,
  tarball: &str,
) -> Result<String, RemoteError> {
  validate_absolute_path(npm, "npm")?;
  validate_absolute_path(tarball, "host tarball")?;
  let script = format!(
    "mkdir -p {prefix} && exec {npm} install --prefix {prefix} --no-audit --no-fund --loglevel=error {tarball}",
    prefix = quote_posix(&host.prefix)?,
    npm = quote_posix(npm)?,
    tarball = quote_posix(tarball)?,
  );
  shell_script(&script)
}

/// A command that reports whether a pid is still alive, then kills its group.
pub fn kill_script(pid_file: &str) -> Result<String, RemoteError> {
  validate_absolute_path(pid_file, "pid file")?;
  let quoted = quote_posix(pid_file)?;
  let script = format!(
    "pid=$(cat {quoted} 2>/dev/null) && [ -n \"$pid\" ] && kill -TERM -\"$pid\" 2>/dev/null || kill -TERM \"$pid\" 2>/dev/null; rm -f {quoted}",
  );
  shell_script(&script)
}

// --------------------------------------------------------------- readiness

/// The readiness JSON `serve --machine` writes to stdout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessLine {
  pub service_instance_id: String,
  pub port: u16,
  pub url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawReadiness {
  service_instance_id: String,
  port: u16,
  #[serde(default)]
  url: String,
}

/// Extracts readiness from one stdout line. Anything that is not the readiness
/// object (npm notices, shell banners) is ignored.
pub fn parse_readiness_line(line: &str) -> Option<ReadinessLine> {
  let line = line.trim();
  if !line.starts_with('{') {
    return None;
  }
  let raw: RawReadiness = serde_json::from_str(line).ok()?;
  if raw.service_instance_id.is_empty() || raw.port == 0 {
    return None;
  }
  Some(ReadinessLine {
    service_instance_id: raw.service_instance_id,
    port: raw.port,
    url: raw.url,
  })
}

/// Finds the single-use ticket in a pairing URL or the line that prints it.
pub fn parse_ticket(text: &str) -> Option<String> {
  for marker in [
    "?pair=", "&pair=", "#pair=", "?ticket=", "&ticket=", "#ticket=",
  ] {
    if let Some(index) = text.find(marker) {
      let rest = &text[index + marker.len()..];
      let ticket: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~'))
        .collect();
      if !ticket.is_empty() {
        return Some(ticket);
      }
    }
  }
  None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeReadiness {
  pub service_instance_id: String,
  pub port: u16,
  pub ticket: String,
}

/// Combines the stdout readiness object and the stderr pairing URL.
pub fn combine_readiness(line: ReadinessLine, stderr: &str) -> Result<ServeReadiness, RemoteError> {
  let ticket = parse_ticket(stderr).ok_or_else(|| {
    RemoteError::HostUnavailable("serve reported readiness but printed no pairing ticket".into())
  })?;
  Ok(ServeReadiness {
    service_instance_id: line.service_instance_id,
    port: line.port,
    ticket,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  fn alias() -> SshTarget {
    SshTarget::Alias {
      alias: "refyard".into(),
    }
  }

  fn host_target() -> SshTarget {
    SshTarget::Host {
      user: "alice".into(),
      host: "[::1]".into(),
      port: Some(2222),
    }
  }

  #[test]
  fn exec_argv_is_fixed_and_has_no_local_shell() {
    let command = ssh_exec(&alias(), "exec 'echo' 'hi'").unwrap();
    assert_eq!(command.program, "ssh");
    assert_eq!(
      command.args,
      [
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "NumberOfPasswordPrompts=0",
        "-o",
        "StrictHostKeyChecking=yes",
        "-o",
        "ConnectTimeout=15",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=3",
        "-o",
        "ClearAllForwardings=yes",
        "-o",
        "ForwardAgent=no",
        "-o",
        "ForwardX11=no",
        "-o",
        "PermitLocalCommand=no",
        "refyard",
        "exec 'echo' 'hi'",
      ]
    );
  }

  #[test]
  fn concrete_hosts_carry_user_host_and_port() {
    let command = ssh_tunnel(&host_target(), 49152, 9400).unwrap();
    assert_eq!(command.args[command.args.len() - 5], "-p");
    assert_eq!(command.args[command.args.len() - 4], "2222");
    assert_eq!(command.args[command.args.len() - 3], "alice@[::1]");
    assert_eq!(command.args[command.args.len() - 2], "-L");
    assert_eq!(
      command.args[command.args.len() - 1],
      "127.0.0.1:49152:127.0.0.1:9400"
    );
  }

  #[test]
  fn a_tunnel_never_clears_its_own_forwarding() {
    let command = ssh_tunnel(&alias(), 49152, 9400).unwrap();
    assert!(!command
      .args
      .iter()
      .any(|arg| arg == "ClearAllForwardings=yes"));
    assert!(command
      .args
      .iter()
      .any(|arg| arg == "ExitOnForwardFailure=yes"));
    assert!(command.args.iter().any(|arg| arg == "-N"));
  }

  #[test]
  fn tunnel_ports_are_bounded() {
    assert!(ssh_tunnel(&alias(), 0, 1).is_err());
    assert!(ssh_tunnel(&alias(), 1, 0).is_err());
  }

  #[test]
  fn transfer_quotes_the_destination() {
    let command = ssh_transfer(&alias(), "/home/al ice/agent").unwrap();
    assert_eq!(
      command.args.last().unwrap(),
      "exec cat > '/home/al ice/agent'"
    );
  }

  #[test]
  fn serve_script_pins_the_agent_and_host_entry() {
    let host = HostLayout::new("/home/alice", "0.4.0").unwrap();
    let agent = "/home/alice/.space-lens/agents/0.4.0/spacelens";
    let script = serve_script(
      &host,
      agent,
      "/usr/bin/node",
      "/home/alice",
      &RemotePolicy::read_only(),
    )
    .unwrap();
    assert!(script.starts_with("exec 'sh' '-c' "));
    assert!(script.contains("--machine"));
    assert!(script.contains("--allow-memory"));
    assert!(script.contains("--root"));
    assert!(!script.contains("--allow-cleanup"));
    assert!(!script.contains("--allow-process-control"));
    assert!(script.contains("dist/cli.mjs"));
    // The npm host cannot discover the installed sidecar on its own.
    assert!(script.contains(&format!("SPACELENS_LOCAL_SCAN_BIN={agent}")));
  }

  #[test]
  fn serve_script_grants_cleanup_only_when_policy_allows_it() {
    let host = HostLayout::new("/home/alice", "0.4.0").unwrap();
    let script = serve_script(
      &host,
      "/home/alice/.space-lens/agents/0.4.0/spacelens",
      "/usr/bin/node",
      "/home/alice",
      &RemotePolicy {
        allow_cleanup: true,
      },
    )
    .unwrap();
    assert!(script.contains("--allow-cleanup"));
    assert!(!script.contains("--allow-process-control"));
  }

  #[test]
  fn serve_script_quotes_an_agent_path_with_spaces() {
    let host = HostLayout::new("/home/al ice", "0.4.0").unwrap();
    let script = serve_script(
      &host,
      "/home/al ice/.space-lens/agents/0.4.0/spacelens",
      "/usr/bin/node",
      "/home/al ice",
      &RemotePolicy::read_only(),
    )
    .unwrap();
    // One quoted assignment word: the space must not split it into two.
    assert!(
      script.contains("SPACELENS_LOCAL_SCAN_BIN=/home/al ice/.space-lens/agents/0.4.0/spacelens")
    );
  }

  #[test]
  fn probe_command_carries_a_multi_line_script() {
    let command = ssh_probe(&alias()).unwrap();
    let script = command.args.last().unwrap();
    assert!(script.contains("SPACELENS-PROBE"));
    // The whole probe is one shell word; its newlines are script syntax.
    assert_eq!(
      script.matches('\n').count(),
      PROBE_SCRIPT.matches('\n').count()
    );
  }

  #[test]
  fn readiness_ignores_noise_and_finds_the_ticket() {
    let line = parse_readiness_line(
      r#"{"serviceInstanceId":"inst_abc","port":39421,"url":"http://127.0.0.1:39421","ticketSingleUse":true}"#,
    )
    .unwrap();
    assert_eq!(line.port, 39421);
    assert!(parse_readiness_line("npm notice something").is_none());
    assert!(parse_readiness_line(r#"{"port":0}"#).is_none());

    let ready = combine_readiness(
      line,
      "pairing URL (single use): http://127.0.0.1:39421/?pair=abc_DEF-123\n",
    )
    .unwrap();
    assert_eq!(ready.ticket, "abc_DEF-123");

    assert!(combine_readiness(
      ReadinessLine {
        service_instance_id: "x".into(),
        port: 1,
        url: String::new(),
      },
      "no ticket here",
    )
    .is_err());
  }

  #[test]
  fn ticket_parsing_accepts_fragment_and_api_urls() {
    assert_eq!(
      parse_ticket("http://127.0.0.1:1/?pair=t1&api=x").as_deref(),
      Some("t1")
    );
    assert_eq!(
      parse_ticket("http://127.0.0.1:1/#ticket=t2").as_deref(),
      Some("t2")
    );
    assert_eq!(parse_ticket("nothing"), None);
  }

  #[test]
  fn host_install_is_pinned_and_prefix_local() {
    let host = HostLayout::new("/home/alice", "0.4.0").unwrap();
    let script = host_install_script(&host, "/usr/bin/npm").unwrap();
    assert!(script.contains("install --prefix"));
    assert!(script.contains("'@space-lens/cli@0.4.0'"));
    assert!(!script.contains("sudo"));
  }
}
