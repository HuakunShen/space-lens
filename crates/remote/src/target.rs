//! One structured SSH destination, validated before it reaches `ssh`.
//!
//! The WebView may name a `~/.ssh/config` alias or a concrete `user@host`.
//! It may never supply a command, an option, an identity file, or anything
//! else that would become local `ssh` syntax. Validation here is deliberately
//! narrower than OpenSSH's grammar: a rejected target is a bug report, not a
//! prompt to be clever.

use std::fmt;
use std::net::Ipv6Addr;

use serde::{Deserialize, Serialize};

use crate::error::RemoteError;

const MAX_FIELD: usize = 255;
const MAX_HOST: usize = 253;

/// A validated SSH destination.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
  tag = "kind",
  rename_all = "camelCase",
  rename_all_fields = "camelCase",
  deny_unknown_fields
)]
pub enum SshTarget {
  /// A concrete `Host` entry from `~/.ssh/config`.
  Alias { alias: String },
  /// A destination the user typed: user, host, optional port.
  Host {
    user: String,
    host: String,
    #[serde(default)]
    port: Option<u16>,
  },
}

impl SshTarget {
  pub fn validate(&self) -> Result<(), RemoteError> {
    match self {
      Self::Alias { alias } => validate_alias(alias),
      Self::Host { user, host, port } => {
        validate_user(user)?;
        validate_host(host)?;
        if let Some(port) = port {
          if *port == 0 {
            return Err(RemoteError::InvalidTarget("port must be 1-65535".into()));
          }
        }
        Ok(())
      }
    }
  }

  /// What goes in the `ssh` destination argument (never includes `-p`).
  pub fn destination(&self) -> String {
    match self {
      Self::Alias { alias } => alias.clone(),
      Self::Host { user, host, .. } => format!("{user}@{host}"),
    }
  }

  /// Stable identity for a saved device. Never shown as a label.
  pub fn device_id(&self) -> String {
    match self {
      Self::Alias { alias } => format!("remote:{alias}"),
      Self::Host { user, host, port } => match port {
        Some(port) => format!("remote:{user}@{host}:{port}"),
        None => format!("remote:{user}@{host}"),
      },
    }
  }

  /// Human label for the device switcher.
  pub fn label(&self) -> String {
    match self {
      Self::Alias { alias } => alias.clone(),
      Self::Host { user, host, port } => match port {
        Some(port) => format!("{user}@{host}:{port}"),
        None => format!("{user}@{host}"),
      },
    }
  }

  /// The port passed to `ssh -p`, if any.
  pub fn port(&self) -> Option<u16> {
    match self {
      Self::Alias { .. } => None,
      Self::Host { port, .. } => *port,
    }
  }
}

impl fmt::Display for SshTarget {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&self.label())
  }
}

fn validate_alias(alias: &str) -> Result<(), RemoteError> {
  if alias.is_empty() || alias.len() > MAX_FIELD {
    return Err(RemoteError::InvalidTarget(
      "alias must be 1-255 bytes".into(),
    ));
  }
  if alias.starts_with('-') {
    return Err(RemoteError::InvalidTarget(
      "alias may not start with '-'".into(),
    ));
  }
  if !alias
    .chars()
    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '@' | '-'))
  {
    return Err(RemoteError::InvalidTarget(format!(
      "alias contains an unsupported character: {alias:?}"
    )));
  }
  Ok(())
}

fn validate_user(user: &str) -> Result<(), RemoteError> {
  if user.is_empty() || user.len() > MAX_FIELD {
    return Err(RemoteError::InvalidTarget(
      "user must be 1-255 bytes".into(),
    ));
  }
  if user.starts_with('-') {
    return Err(RemoteError::InvalidTarget(
      "user may not start with '-'".into(),
    ));
  }
  if !user
    .chars()
    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
  {
    return Err(RemoteError::InvalidTarget(format!(
      "user contains an unsupported character: {user:?}"
    )));
  }
  Ok(())
}

fn validate_host(host: &str) -> Result<(), RemoteError> {
  if host.is_empty() || host.len() > MAX_FIELD {
    return Err(RemoteError::InvalidTarget(
      "host must be 1-255 bytes".into(),
    ));
  }
  if host.starts_with('-') {
    return Err(RemoteError::InvalidTarget(
      "host may not start with '-'".into(),
    ));
  }
  if host
    .chars()
    .any(|c| c.is_whitespace() || c.is_control() || c == '/')
  {
    return Err(RemoteError::InvalidTarget(format!(
      "host contains whitespace, a control character, or '/': {host:?}"
    )));
  }
  // Bracketed IPv6 only: `user@[::1]`, never `user@::1` (the colon would be
  // ambiguous with the port in a saved device id and with ssh's own grammar).
  if let Some(inner) = host
    .strip_prefix('[')
    .and_then(|rest| rest.strip_suffix(']'))
  {
    return inner
      .parse::<Ipv6Addr>()
      .map(|_| ())
      .map_err(|_| RemoteError::InvalidTarget(format!("not a valid IPv6 address: {inner:?}")));
  }
  if host.contains(':') {
    return Err(RemoteError::InvalidTarget(
      "IPv6 hosts must be bracketed, for example [::1]".into(),
    ));
  }
  if host.len() > MAX_HOST {
    return Err(RemoteError::InvalidTarget(
      "host is longer than 253 bytes".into(),
    ));
  }
  if host.parse::<std::net::Ipv4Addr>().is_ok() {
    return Ok(());
  }
  for label in host.split('.') {
    if label.is_empty() || label.len() > 63 {
      return Err(RemoteError::InvalidTarget(format!(
        "invalid host label in {host:?}"
      )));
    }
    if label.starts_with('-') || label.ends_with('-') {
      return Err(RemoteError::InvalidTarget(format!(
        "invalid host label in {host:?}"
      )));
    }
    if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
      return Err(RemoteError::InvalidTarget(format!(
        "invalid host label in {host:?}"
      )));
    }
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn alias(value: &str) -> SshTarget {
    SshTarget::Alias {
      alias: value.into(),
    }
  }

  fn host(user: &str, value: &str, port: Option<u16>) -> SshTarget {
    SshTarget::Host {
      user: user.into(),
      host: value.into(),
      port,
    }
  }

  #[test]
  fn accepts_aliases_and_concrete_hosts() {
    for target in [
      alias("refyard"),
      alias("work-box_2"),
      alias("user@prod-01"),
      host("alice", "example.com", None),
      host("alice", "127.0.0.1", Some(2222)),
      host("alice", "[::1]", Some(22)),
    ] {
      target
        .validate()
        .unwrap_or_else(|error| panic!("{target:?}: {error}"));
    }
  }

  #[test]
  fn rejects_shell_and_option_syntax_everywhere() {
    for target in [
      alias("-oProxyCommand=evil"),
      alias("host;rm -rf /"),
      alias("host name"),
      alias("host\nname"),
      host("-root", "example.com", None),
      host("alice", "-example.com", None),
      host("alice", "example.com;id", None),
      host("alice", "example.com/", None),
      host("alice", "example.com:22", None),
      host("alice", "::1", None),
      host("alice", "[::1", None),
      host("alice", "[not-ipv6]", None),
      host("alice", "a..b", None),
      host("alice", "example.com", Some(0)),
    ] {
      assert!(target.validate().is_err(), "accepted {target:?}");
    }
  }

  #[test]
  fn device_ids_are_stable_and_distinct() {
    assert_eq!(alias("refyard").device_id(), "remote:refyard");
    assert_eq!(
      host("alice", "example.com", None).device_id(),
      "remote:alice@example.com"
    );
    assert_eq!(
      host("alice", "example.com", Some(2222)).device_id(),
      "remote:alice@example.com:2222"
    );
    assert_ne!(
      host("alice", "example.com", None).device_id(),
      host("alice", "example.com", Some(22)).device_id()
    );
  }
}
