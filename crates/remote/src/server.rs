//! Local `ssh` argv, the remote scripts, and readiness parsing.
//!
//! Only this module turns a [`SshTarget`] into a process. The local argv stops
//! at `ssh`: the target is one argument, and the remote command is one
//! already-encoded string. Nothing here starts a local shell. The remote side
//! has two command channels, chosen once from the probe and threaded through
//! every command: POSIX hosts receive `exec 'sh' '-c' '…'`, and Windows hosts
//! — whose OpenSSH default shell is cmd.exe — receive one inert
//! `powershell -EncodedCommand` token ([`RemoteShell`]).

use serde::Deserialize;

use crate::error::RemoteError;
use crate::install::HostLayout;
use crate::probe::{
  validate_absolute_path, validate_windows_path, PROBE_SCRIPT, PROBE_SCRIPT_PWSH,
};
use crate::quote::{pwsh_command, pwsh_literal, quote_posix, shell_script};
use crate::target::SshTarget;

/// The host resolves its Rust sidecar from this variable; without it the
/// installed npm package would search build directories that exist only in a
/// checkout, and protected scans, memory, and app audits would fail.
pub const LOCAL_SCAN_BIN_ENV: &str = "SPACELENS_LOCAL_SCAN_BIN";

/// The command channel one remote platform speaks.
///
/// The flavor is chosen once, from the probe, and threaded through every
/// command built for that host. Mixing channels would hand cmd.exe a POSIX
/// script or `sh` a base64 token, and both fail closed on their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteShell {
  /// The remote login shell is POSIX; scripts travel as `exec 'sh' '-c' '…'`
  /// and every interpolated value is a quoted POSIX word.
  Posix,
  /// Windows OpenSSH defaults to cmd.exe, so scripts travel as one
  /// `powershell -EncodedCommand <base64 UTF-16LE>` token. There is no
  /// quoting grammar on this channel at all; see [`quote::pwsh_command`].
  WindowsPwsh,
}

impl RemoteShell {
  /// The channel a probe's platform implies. Anything unknown is refused
  /// before the first provisioning command is built.
  pub fn for_platform(platform: &str) -> Result<Self, RemoteError> {
    match platform {
      "linux" | "macos" => Ok(Self::Posix),
      "windows" => Ok(Self::WindowsPwsh),
      other => Err(RemoteError::UnsupportedTarget(format!(
        "remote {other} hosts are not supported"
      ))),
    }
  }

  /// Turns one script body into the single command string `ssh` carries.
  ///
  /// This is the only place a body becomes a command, so a script can never
  /// reach the wrong shell by accident.
  pub fn wrap_script(self, script: &str) -> Result<String, RemoteError> {
    match self {
      Self::Posix => shell_script(script),
      Self::WindowsPwsh => pwsh_command(script),
    }
  }

  /// Lexical path validation with the rules of the remote platform.
  pub fn validate_path(self, path: &str, what: &str) -> Result<(), RemoteError> {
    match self {
      Self::Posix => validate_absolute_path(path, what),
      Self::WindowsPwsh => validate_windows_path(path, what),
    }
  }
}

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

/// One remote script, executed without a PTY. `script` is a body in the
/// remote shell's own grammar; `shell` decides how it is carried to the host.
pub fn ssh_exec_shell(
  target: &SshTarget,
  script: &str,
  shell: RemoteShell,
) -> Result<SshCommand, RemoteError> {
  let mut command = base(target, EXEC_OPTIONS)?;
  command.args.push(shell.wrap_script(script)?);
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

/// Runs the fixed probe script for one platform's command channel.
pub fn ssh_probe_shell(target: &SshTarget, shell: RemoteShell) -> Result<SshCommand, RemoteError> {
  let script = match shell {
    RemoteShell::Posix => PROBE_SCRIPT,
    RemoteShell::WindowsPwsh => PROBE_SCRIPT_PWSH,
  };
  ssh_exec_shell(target, script, shell)
}

/// Writes bytes from local stdin to `remote_path` on the remote filesystem.
pub fn ssh_transfer_shell(
  target: &SshTarget,
  remote_path: &str,
  shell: RemoteShell,
) -> Result<SshCommand, RemoteError> {
  shell.validate_path(remote_path, "transfer path")?;
  match shell {
    RemoteShell::Posix => {
      // The redirection must sit outside the quoted word: `exec cat > '<path>'`.
      // The remote login shell performs the redirect itself, so this is a
      // one-liner handed to the login shell, not a wrapped script — the exact
      // form every POSIX host has always received.
      let mut command = base(target, EXEC_OPTIONS)?;
      command
        .args
        .push(format!("exec cat > {}", quote_posix(remote_path)?));
      Ok(command)
    }
    RemoteShell::WindowsPwsh => {
      // cmd.exe cannot carry a raw redirect into PowerShell, so the receiver
      // reads standard input itself and writes the bytes out. `$sshStdin` and
      // `$sshFile` are named deliberately: PowerShell variable names are
      // case-insensitive, so `$in` would collide with the automatic `$input`
      // variable.
      let script = format!(
        "$sshStdin = [Console]::OpenStandardInput()\n\
         $sshFile = [IO.File]::Create({path})\n\
         $sshStdin.CopyTo($sshFile)\n\
         $sshFile.Close()",
        path = pwsh_literal(remote_path)?
      );
      ssh_exec_shell(target, &script, shell)
    }
  }
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

/// The remote script that starts the pinned host package (POSIX body).
///
/// `npx` is deliberately not used at run time: `npm install --prefix` gives a
/// stable entry point and a single node process whose pid is the one written to
/// the pid file. That makes disconnect cleanup exact instead of best-effort
/// process-tree guessing. The caller carries this body through
/// [`ssh_exec_shell`] with [`RemoteShell::Posix`].
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
  Ok(format!(
    "mkdir -p {pid_dir} && printf '%s\\n' $$ > {pid_file} && exec env {environment} {node} {entry} serve --machine --allow-memory{cleanup} --root {root}",
    pid_dir = quote_posix(&format!("{}/{}/{}", host.home, crate::install::REMOTE_DIR, host.version))?,
    pid_file = quote_posix(&host.pid_file)?,
    node = quote_posix(node)?,
    entry = quote_posix(&host.entry)?,
    root = quote_posix(home)?,
  ))
}

/// The remote script that starts the pinned host package on Windows (a
/// PowerShell body; the caller carries it through [`ssh_exec_shell`] with
/// [`RemoteShell::WindowsPwsh`], which wraps it via [`quote::pwsh_command`]).
///
/// Mirrors [`serve_script`] decision for decision: the pid file records the
/// session pid, the sidecar is exported through `SPACELENS_LOCAL_SCAN_BIN`,
/// and `--allow-process-control` is never granted. The trailing
/// `exit $LASTEXITCODE` matters: `&` runs node as a child, so a crashed node
/// would otherwise leave the SSH session's status at PowerShell's 0 and the
/// client would wait out the readiness timeout instead of seeing the exit.
pub fn serve_script_pwsh(
  host: &HostLayout,
  agent_binary: &str,
  node: &str,
  home: &str,
  policy: &RemotePolicy,
) -> Result<String, RemoteError> {
  validate_windows_path(agent_binary, "agent binary")?;
  validate_windows_path(node, "node")?;
  validate_windows_path(home, "home")?;
  let pid_dir = format!(
    "{}/{}/{}",
    host.home,
    crate::install::REMOTE_DIR,
    host.version
  );
  let cleanup = if policy.allow_cleanup {
    " --allow-cleanup"
  } else {
    ""
  };
  Ok(format!(
    "$ErrorActionPreference = 'Stop'\n\
     New-Item -ItemType Directory -Force -Path {pid_dir} | Out-Null\n\
     Set-Content -LiteralPath {pid_file} -Value $PID\n\
     $env:{LOCAL_SCAN_BIN_ENV} = {agent}\n\
     & {node} {entry} serve --machine --allow-memory{cleanup} --root {root}\n\
     exit $LASTEXITCODE",
    pid_dir = pwsh_literal(&pid_dir)?,
    pid_file = pwsh_literal(&host.pid_file)?,
    agent = pwsh_literal(agent_binary)?,
    node = pwsh_literal(node)?,
    entry = pwsh_literal(&host.entry)?,
    root = pwsh_literal(home)?,
  ))
}

/// The remote script that installs the pinned host package without sudo
/// (POSIX body; the caller carries it through [`ssh_exec_shell`]).
///
/// `npm` is a `#!/usr/bin/env node` script, so the node directory found by
/// the probe is prepended to `PATH`: on hosts where node lives in nvm or a
/// user-local tree, the non-interactive SSH session has no node on `PATH`
/// and npm would fail before doing anything.
pub fn host_install_script(
  host: &HostLayout,
  npm: &str,
  node_dir: &str,
) -> Result<String, RemoteError> {
  validate_absolute_path(npm, "npm")?;
  validate_absolute_path(node_dir, "node directory")?;
  Ok(format!(
    "mkdir -p {prefix} && PATH={node_dir}:$PATH exec {npm} install --prefix {prefix} --no-audit --no-fund --loglevel=error '{package}@{version}'",
    prefix = quote_posix(&host.prefix)?,
    npm = quote_posix(npm)?,
    node_dir = quote_posix(node_dir)?,
    package = crate::manifest::PACKAGE,
    version = host.version,
  ))
}

/// The remote script that installs the pinned host package on Windows (a
/// PowerShell body). `npm.cmd` resolves its own node on Windows, but the
/// node directory is prepended to `Path` anyway so the two flavors make the
/// same promise; `exit $LASTEXITCODE` reports npm's own status because `&`
/// runs it as a child PowerShell would otherwise exit 0 under.
pub fn host_install_script_pwsh(
  host: &HostLayout,
  npm: &str,
  node_dir: &str,
) -> Result<String, RemoteError> {
  validate_windows_path(npm, "npm")?;
  validate_windows_path(node_dir, "node directory")?;
  Ok(format!(
    "$ErrorActionPreference = 'Stop'\n\
     $env:Path = {node_dir} + ';' + $env:Path\n\
     & {npm} install --prefix {prefix} --no-audit --no-fund --loglevel=error '{package}@{version}'\n\
     exit $LASTEXITCODE",
    node_dir = pwsh_literal(node_dir)?,
    npm = pwsh_literal(npm)?,
    prefix = pwsh_literal(&host.prefix)?,
    package = crate::manifest::PACKAGE,
    version = host.version,
  ))
}

/// Installs the host package from a tarball already on the remote filesystem
/// (POSIX body).
///
/// Development and acceptance testing use this with a local `npm pack` result
/// transferred over the same SSH session, so a remote host can be exercised
/// before the package version is published to the registry. `node_dir` serves
/// the same `env node` shebang as [`host_install_script`].
pub fn host_install_tarball_script(
  host: &HostLayout,
  npm: &str,
  tarball: &str,
  node_dir: &str,
) -> Result<String, RemoteError> {
  validate_absolute_path(npm, "npm")?;
  validate_absolute_path(tarball, "host tarball")?;
  validate_absolute_path(node_dir, "node directory")?;
  Ok(format!(
    "mkdir -p {prefix} && PATH={node_dir}:$PATH exec {npm} install --prefix {prefix} --no-audit --no-fund --loglevel=error {tarball}",
    prefix = quote_posix(&host.prefix)?,
    npm = quote_posix(npm)?,
    tarball = quote_posix(tarball)?,
    node_dir = quote_posix(node_dir)?,
  ))
}

/// Installs the host package from a tarball already on the remote Windows
/// filesystem (a PowerShell body); see [`host_install_tarball_script`].
pub fn host_install_tarball_script_pwsh(
  host: &HostLayout,
  npm: &str,
  tarball: &str,
  node_dir: &str,
) -> Result<String, RemoteError> {
  validate_windows_path(npm, "npm")?;
  validate_windows_path(tarball, "host tarball")?;
  validate_windows_path(node_dir, "node directory")?;
  Ok(format!(
    "$ErrorActionPreference = 'Stop'\n\
     $env:Path = {node_dir} + ';' + $env:Path\n\
     & {npm} install --prefix {prefix} --no-audit --no-fund --loglevel=error {tarball}\n\
     exit $LASTEXITCODE",
    node_dir = pwsh_literal(node_dir)?,
    npm = pwsh_literal(npm)?,
    prefix = pwsh_literal(&host.prefix)?,
    tarball = pwsh_literal(tarball)?,
  ))
}

/// Stops the recorded serve process and drops its pid file (POSIX body).
pub fn kill_script(pid_file: &str) -> Result<String, RemoteError> {
  validate_absolute_path(pid_file, "pid file")?;
  let quoted = quote_posix(pid_file)?;
  Ok(format!(
    "pid=$(cat {quoted} 2>/dev/null) && [ -n \"$pid\" ] && kill -TERM -\"$pid\" 2>/dev/null || kill -TERM \"$pid\" 2>/dev/null; rm -f {quoted}"
  ))
}

/// Stops the recorded serve process and drops its pid file on Windows (a
/// PowerShell body).
///
/// `$serverPid` is named that way on purpose: PowerShell variable names are
/// case-insensitive, so `$pid` would collide with the read-only automatic
/// `$PID`. A missing or unreadable pid file leaves `$serverPid` at 0 and
/// `Stop-Process -Id 0` fails silently — the same no-op the POSIX script
/// performs for a pid file that is already gone.
pub fn kill_script_pwsh(pid_file: &str) -> Result<String, RemoteError> {
  validate_windows_path(pid_file, "pid file")?;
  Ok(format!(
    "$serverPid = 0\n\
     try {{ $serverPid = [int](Get-Content -LiteralPath {file} -ErrorAction Stop) }} catch {{}}\n\
     Stop-Process -Id $serverPid -ErrorAction SilentlyContinue\n\
     Remove-Item -LiteralPath {file} -ErrorAction SilentlyContinue",
    file = pwsh_literal(pid_file)?
  ))
}

// ------------------------------------------------- provisioning micro-scripts
//
// Small script bodies the runtime chains while provisioning. Each one takes
// its platform's path rules and returns a body, never a command: the caller
// carries them through `ssh_exec_shell`, which is the single place a body
// becomes a command. POSIX bodies are byte-for-byte the one-liners the
// runtime has always sent.

/// Creates a directory remotely (`mkdir -p` semantics).
pub fn mkdir_script(dir: &str, shell: RemoteShell) -> Result<String, RemoteError> {
  shell.validate_path(dir, "directory")?;
  match shell {
    RemoteShell::Posix => Ok(format!("mkdir -p {}", quote_posix(dir)?)),
    RemoteShell::WindowsPwsh => Ok(format!(
      "New-Item -ItemType Directory -Force -Path {} | Out-Null",
      pwsh_literal(dir)?
    )),
  }
}

/// Creates the agent staging directory. POSIX also makes it private (`700`):
/// the agent binary lands there before the remote has proven it. Windows has
/// no permission bit to set; the directory already sits inside the user's own
/// profile, which is the same boundary.
pub fn staging_dir_script(dir: &str, shell: RemoteShell) -> Result<String, RemoteError> {
  shell.validate_path(dir, "staging directory")?;
  match shell {
    RemoteShell::Posix => Ok(format!(
      "mkdir -p {dir} && chmod 700 {dir}",
      dir = quote_posix(dir)?
    )),
    RemoteShell::WindowsPwsh => Ok(format!(
      "New-Item -ItemType Directory -Force -Path {} | Out-Null",
      pwsh_literal(dir)?
    )),
  }
}

/// Reports one file's byte count on stdout as a plain number.
pub fn byte_count_script(path: &str, shell: RemoteShell) -> Result<String, RemoteError> {
  shell.validate_path(path, "byte-count path")?;
  match shell {
    RemoteShell::Posix => Ok(format!("wc -c < {}", quote_posix(path)?)),
    RemoteShell::WindowsPwsh => Ok(format!(
      "(Get-Item -LiteralPath {}).Length",
      pwsh_literal(path)?
    )),
  }
}

/// Runs `agent-info --json` on an already-installed agent and mirrors its
/// exit code, so a stale or broken agent is indistinguishable from none.
pub fn agent_health_script(binary: &str, shell: RemoteShell) -> Result<String, RemoteError> {
  shell.validate_path(binary, "agent binary")?;
  match shell {
    RemoteShell::Posix => Ok(format!("exec {} agent-info --json", quote_posix(binary)?)),
    RemoteShell::WindowsPwsh => agent_info_pwsh(binary),
  }
}

/// The post-transfer smoke test: marks the fresh binary executable where the
/// platform has that bit, then runs the same `agent-info` handshake.
pub fn agent_smoke_script(binary: &str, shell: RemoteShell) -> Result<String, RemoteError> {
  shell.validate_path(binary, "agent binary")?;
  match shell {
    RemoteShell::Posix => Ok(format!(
      "chmod 755 {binary} && exec {binary} agent-info --json",
      binary = quote_posix(binary)?
    )),
    RemoteShell::WindowsPwsh => agent_info_pwsh(binary),
  }
}

/// PowerShell runs the agent as a child process, so a failing agent would
/// otherwise leave the session's exit status at PowerShell's own 0; the
/// trailing exit makes the SSH status tell the truth.
fn agent_info_pwsh(binary: &str) -> Result<String, RemoteError> {
  Ok(format!(
    "& {binary} agent-info --json\nexit $LASTEXITCODE",
    binary = pwsh_literal(binary)?
  ))
}

/// Moves the staged agent into its versioned home, then drops the emptied
/// staging directory.
pub fn activate_agent_script(
  version_dir: &str,
  staged_binary: &str,
  final_binary: &str,
  staging_dir: &str,
  shell: RemoteShell,
) -> Result<String, RemoteError> {
  shell.validate_path(version_dir, "version directory")?;
  shell.validate_path(staged_binary, "staged agent")?;
  shell.validate_path(final_binary, "agent binary")?;
  shell.validate_path(staging_dir, "staging directory")?;
  match shell {
    RemoteShell::Posix => Ok(format!(
      "mkdir -p {version_dir} && mv {staged} {final} && rmdir {staging}",
      version_dir = quote_posix(version_dir)?,
      staged = quote_posix(staged_binary)?,
      final = quote_posix(final_binary)?,
      staging = quote_posix(staging_dir)?,
    )),
    RemoteShell::WindowsPwsh => Ok(format!(
      "$ErrorActionPreference = 'Stop'\n\
       New-Item -ItemType Directory -Force -Path {version_dir} | Out-Null\n\
       Move-Item -LiteralPath {staged} -Destination {final}\n\
       Remove-Item -LiteralPath {staging}",
      version_dir = pwsh_literal(version_dir)?,
      staged = pwsh_literal(staged_binary)?,
      final = pwsh_literal(final_binary)?,
      staging = pwsh_literal(staging_dir)?,
    )),
  }
}

/// Best-effort recursive removal (a failed install's staging tree).
pub fn remove_tree_script(path: &str, shell: RemoteShell) -> Result<String, RemoteError> {
  shell.validate_path(path, "removal target")?;
  match shell {
    RemoteShell::Posix => Ok(format!("rm -rf {}", quote_posix(path)?)),
    RemoteShell::WindowsPwsh => Ok(format!(
      "Remove-Item -LiteralPath {} -Recurse -Force -ErrorAction SilentlyContinue",
      pwsh_literal(path)?
    )),
  }
}

/// Best-effort single-file removal (the transferred host tarball).
pub fn remove_file_script(path: &str, shell: RemoteShell) -> Result<String, RemoteError> {
  shell.validate_path(path, "removal target")?;
  match shell {
    RemoteShell::Posix => Ok(format!("rm -f {}", quote_posix(path)?)),
    RemoteShell::WindowsPwsh => Ok(format!(
      "Remove-Item -LiteralPath {} -Force -ErrorAction SilentlyContinue",
      pwsh_literal(path)?
    )),
  }
}

/// Reports a file's existence through the SSH exit status.
pub fn file_exists_script(path: &str, shell: RemoteShell) -> Result<String, RemoteError> {
  shell.validate_path(path, "checked file")?;
  match shell {
    RemoteShell::Posix => Ok(format!("test -f {}", quote_posix(path)?)),
    RemoteShell::WindowsPwsh => Ok(format!(
      "if (Test-Path -LiteralPath {path} -PathType Leaf) {{ exit 0 }}\nexit 1",
      path = pwsh_literal(path)?
    )),
  }
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
    let command = ssh_exec_shell(&alias(), "exec 'echo' 'hi'", RemoteShell::Posix).unwrap();
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
        // The body is carried exactly as it always was: one wrapped word.
        &shell_script("exec 'echo' 'hi'").unwrap(),
      ]
    );
  }

  #[test]
  fn platforms_map_to_their_command_channel() {
    assert_eq!(
      RemoteShell::for_platform("linux").unwrap(),
      RemoteShell::Posix
    );
    assert_eq!(
      RemoteShell::for_platform("macos").unwrap(),
      RemoteShell::Posix
    );
    assert_eq!(
      RemoteShell::for_platform("windows").unwrap(),
      RemoteShell::WindowsPwsh
    );
    assert!(RemoteShell::for_platform("freebsd").is_err());
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
    let command = ssh_transfer_shell(&alias(), "/home/al ice/agent", RemoteShell::Posix).unwrap();
    assert_eq!(
      command.args.last().unwrap(),
      "exec cat > '/home/al ice/agent'"
    );
  }

  #[test]
  fn windows_transfer_streams_standard_input_into_the_file() {
    let command =
      ssh_transfer_shell(&alias(), "C:/Users/al ice/agent", RemoteShell::WindowsPwsh).unwrap();
    let token = command.args.last().unwrap();
    assert!(token.starts_with(
      "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand "
    ));
    let encoded = token
      .strip_prefix(
        "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand ",
      )
      .unwrap();
    // Base64 alphabet only: no cmd.exe metacharacter can appear.
    assert!(encoded
      .chars()
      .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=')));
    assert_eq!(decode_pwsh_token(encoded), "$sshStdin = [Console]::OpenStandardInput()\n$sshFile = [IO.File]::Create('C:/Users/al ice/agent')\n$sshStdin.CopyTo($sshFile)\n$sshFile.Close()");
    // A backslash path never reaches cmd.exe through this channel.
    assert!(ssh_transfer_shell(&alias(), "C:\\Users\\agent", RemoteShell::WindowsPwsh).is_err());
    assert!(ssh_transfer_shell(&alias(), "/home/agent", RemoteShell::WindowsPwsh).is_err());
  }

  #[test]
  fn serve_script_pins_the_agent_and_host_entry() {
    let host = HostLayout::new("/home/alice", "0.4.0").unwrap();
    let agent = "/home/alice/.space-lens/agents/0.4.0/spacelens";
    let body = serve_script(
      &host,
      agent,
      "/usr/bin/node",
      "/home/alice",
      &RemotePolicy::read_only(),
    )
    .unwrap();
    // Composed onto the POSIX channel, the command is the exact string every
    // POSIX host has always received.
    let sent = ssh_exec_shell(&alias(), &body, RemoteShell::Posix)
      .unwrap()
      .args
      .last()
      .unwrap()
      .clone();
    assert!(sent.starts_with("exec 'sh' '-c' "));
    assert!(sent.contains("--machine"));
    assert!(sent.contains("--allow-memory"));
    assert!(sent.contains("--root"));
    assert!(!sent.contains("--allow-cleanup"));
    assert!(!sent.contains("--allow-process-control"));
    assert!(sent.contains("dist/cli.mjs"));
    // The npm host cannot discover the installed sidecar on its own.
    assert!(sent.contains(&format!("SPACELENS_LOCAL_SCAN_BIN={agent}")));
  }

  #[test]
  fn windows_serve_script_pins_the_agent_and_never_grants_process_control() {
    let host = HostLayout::windows("C:/Users/shenh", "0.4.0").unwrap();
    let agent = "C:/Users/shenh/.space-lens/agents/0.4.0/spacelens.exe";
    let node = "C:/Program Files/nodejs/node.exe";
    let body = serve_script_pwsh(
      &host,
      agent,
      node,
      "C:/Users/shenh",
      &RemotePolicy {
        allow_cleanup: true,
      },
    )
    .unwrap();
    assert!(body.contains(
      "Set-Content -LiteralPath 'C:/Users/shenh/.space-lens/remote/0.4.0/server.pid' -Value $PID"
    ));
    assert!(body.contains(&format!("$env:SPACELENS_LOCAL_SCAN_BIN = '{agent}'")));
    assert!(body.contains(
      "& 'C:/Program Files/nodejs/node.exe' 'C:/Users/shenh/.space-lens/host/0.4.0/node_modules/@space-lens/cli/dist/cli.mjs' serve --machine --allow-memory --allow-cleanup"
    ));
    assert!(body.ends_with("exit $LASTEXITCODE"));
    assert!(!body.contains("--allow-process-control"));
    // An apostrophe in the home doubles inside the literal and stays inert.
    let quoted = HostLayout::windows("C:/Users/shenh's", "0.4.0").unwrap();
    let body = serve_script_pwsh(
      &quoted,
      "C:/Users/shenh's/.space-lens/agents/0.4.0/spacelens.exe",
      node,
      "C:/Users/shenh's",
      &RemotePolicy::read_only(),
    )
    .unwrap();
    assert!(body.contains("--root 'C:/Users/shenh''s'"));
    // The channel carries it as one base64 token, not free text.
    let token = ssh_exec_shell(&alias(), &body, RemoteShell::WindowsPwsh)
      .unwrap()
      .args
      .last()
      .unwrap()
      .clone();
    assert!(token.starts_with("powershell -NoProfile"));
    assert_eq!(decode_pwsh_token(token.rsplit(' ').next().unwrap()), body);
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
    let command = ssh_probe_shell(&alias(), RemoteShell::Posix).unwrap();
    let script = command.args.last().unwrap();
    assert!(script.contains("SPACELENS-PROBE"));
    // The whole probe is one shell word; its newlines are script syntax.
    assert_eq!(
      script.matches('\n').count(),
      PROBE_SCRIPT.matches('\n').count()
    );
  }

  #[test]
  fn windows_probe_command_is_one_encoded_token() {
    let command = ssh_probe_shell(&alias(), RemoteShell::WindowsPwsh).unwrap();
    let token = command.args.last().unwrap();
    let encoded = token.rsplit(' ').next().unwrap();
    // The exact script travels, unchanged, inside the token.
    assert_eq!(decode_pwsh_token(encoded), PROBE_SCRIPT_PWSH);
  }

  #[test]
  fn kill_scripts_stop_the_recorded_pid_and_clean_up() {
    let posix = kill_script("/home/alice/.space-lens/remote/0.4.0/server.pid").unwrap();
    assert!(posix.contains("kill -TERM"));
    assert!(posix.contains("rm -f '/home/alice/.space-lens/remote/0.4.0/server.pid'"));
    let windows = kill_script_pwsh("C:/Users/shenh/.space-lens/remote/0.4.0/server.pid").unwrap();
    assert!(windows.contains("Stop-Process -Id $serverPid -ErrorAction SilentlyContinue"));
    assert!(windows.contains(
      "Remove-Item -LiteralPath 'C:/Users/shenh/.space-lens/remote/0.4.0/server.pid' -ErrorAction SilentlyContinue"
    ));
    assert!(kill_script_pwsh("C:\\pid").is_err());
    assert!(kill_script("C:/pid").is_err());
  }

  #[test]
  fn micro_scripts_speak_their_own_flavor() {
    // POSIX bodies are byte-for-byte the one-liners the runtime has always
    // sent; Windows bodies are the equivalent PowerShell steps.
    assert_eq!(
      mkdir_script("/home/a/.space-lens/host", RemoteShell::Posix).unwrap(),
      "mkdir -p '/home/a/.space-lens/host'"
    );
    assert_eq!(
      staging_dir_script("/home/a/staging", RemoteShell::Posix).unwrap(),
      "mkdir -p '/home/a/staging' && chmod 700 '/home/a/staging'"
    );
    assert_eq!(
      staging_dir_script("C:/Users/a/staging", RemoteShell::WindowsPwsh).unwrap(),
      "New-Item -ItemType Directory -Force -Path 'C:/Users/a/staging' | Out-Null"
    );
    assert_eq!(
      byte_count_script("/home/a/bin", RemoteShell::Posix).unwrap(),
      "wc -c < '/home/a/bin'"
    );
    assert_eq!(
      byte_count_script("C:/Users/a/bin.exe", RemoteShell::WindowsPwsh).unwrap(),
      "(Get-Item -LiteralPath 'C:/Users/a/bin.exe').Length"
    );
    assert_eq!(
      agent_health_script("/home/a/spacelens", RemoteShell::Posix).unwrap(),
      "exec '/home/a/spacelens' agent-info --json"
    );
    assert_eq!(
      agent_smoke_script("/home/a/spacelens", RemoteShell::Posix).unwrap(),
      "chmod 755 '/home/a/spacelens' && exec '/home/a/spacelens' agent-info --json"
    );
    assert_eq!(
      agent_smoke_script("C:/Users/a/spacelens.exe", RemoteShell::WindowsPwsh).unwrap(),
      "& 'C:/Users/a/spacelens.exe' agent-info --json\nexit $LASTEXITCODE"
    );
    assert_eq!(
      activate_agent_script(
        "/home/a/.space-lens/agents/0.4.0",
        "/home/a/staging/spacelens",
        "/home/a/.space-lens/agents/0.4.0/spacelens",
        "/home/a/staging",
        RemoteShell::Posix,
      )
      .unwrap(),
      "mkdir -p '/home/a/.space-lens/agents/0.4.0' && mv '/home/a/staging/spacelens' '/home/a/.space-lens/agents/0.4.0/spacelens' && rmdir '/home/a/staging'"
    );
    assert_eq!(
      remove_tree_script("/home/a/staging", RemoteShell::Posix).unwrap(),
      "rm -rf '/home/a/staging'"
    );
    assert_eq!(
      remove_file_script("/home/a/x.tgz", RemoteShell::Posix).unwrap(),
      "rm -f '/home/a/x.tgz'"
    );
    assert_eq!(
      file_exists_script("/home/a/entry.mjs", RemoteShell::Posix).unwrap(),
      "test -f '/home/a/entry.mjs'"
    );
    assert_eq!(
      file_exists_script("C:/Users/a/entry.mjs", RemoteShell::WindowsPwsh).unwrap(),
      "if (Test-Path -LiteralPath 'C:/Users/a/entry.mjs' -PathType Leaf) { exit 0 }\nexit 1"
    );
    // A path from the other platform never crosses a channel.
    assert!(mkdir_script("C:/x", RemoteShell::Posix).is_err());
    assert!(mkdir_script("/x", RemoteShell::WindowsPwsh).is_err());
    assert!(file_exists_script("C:\\x", RemoteShell::WindowsPwsh).is_err());
  }

  /// Decodes the base64 UTF-16LE payload of an `-EncodedCommand` token, so
  /// tests assert on the script PowerShell actually receives.
  fn decode_pwsh_token(encoded: &str) -> String {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
      .decode(encoded)
      .expect("the payload is standard base64");
    let units: Vec<u16> = bytes
      .as_chunks::<2>()
      .0
      .iter()
      .map(|pair| u16::from_le_bytes(*pair))
      .collect();
    String::from_utf16_lossy(&units)
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
    let script = host_install_script(&host, "/usr/bin/npm", "/usr/bin").unwrap();
    assert!(script.contains("install --prefix"));
    assert!(script.contains("'@space-lens/cli@0.4.0'"));
    // npm is an `env node` script: the node directory must lead PATH.
    assert!(script.contains("PATH='/usr/bin':$PATH"));
    assert!(!script.contains("sudo"));
  }

  #[test]
  fn windows_host_install_prepends_the_node_directory_and_reports_npm() {
    let host = HostLayout::windows("C:/Users/shenh", "0.4.0").unwrap();
    let npm = "C:/Program Files/nodejs/npm.cmd";
    let body = host_install_script_pwsh(&host, npm, "C:/Program Files/nodejs").unwrap();
    assert!(body.contains("$env:Path = 'C:/Program Files/nodejs' + ';' + $env:Path"));
    assert!(body.contains(&format!(
      "& '{npm}' install --prefix 'C:/Users/shenh/.space-lens/host/0.4.0'"
    )));
    assert!(body.contains("'@space-lens/cli@0.4.0'"));
    assert!(body.ends_with("exit $LASTEXITCODE"));
    assert!(host_install_script_pwsh(&host, "C:\\npm.cmd", "C:/n").is_err());

    let tarball = host_install_tarball_script_pwsh(
      &host,
      npm,
      "C:/Users/shenh/.space-lens/host/.staging-x-host.tgz",
      "C:/Program Files/nodejs",
    )
    .unwrap();
    assert!(tarball.contains("install --prefix 'C:/Users/shenh/.space-lens/host/0.4.0' --no-audit --no-fund --loglevel=error 'C:/Users/shenh/.space-lens/host/.staging-x-host.tgz'"));

    // The POSIX tarball path keeps its exact historical shape.
    let posix = HostLayout::new("/home/alice", "0.4.0").unwrap();
    let body = host_install_tarball_script(
      &posix,
      "/usr/bin/npm",
      "/home/alice/.space-lens/host/.staging-x-host.tgz",
      "/usr/bin",
    )
    .unwrap();
    assert!(body.contains(
      "PATH='/usr/bin':$PATH exec '/usr/bin/npm' install --prefix '/home/alice/.space-lens/host/0.4.0'"
    ));
    assert!(body.contains("'/home/alice/.space-lens/host/.staging-x-host.tgz'"));
  }
}
