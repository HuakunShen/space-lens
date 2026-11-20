//! Read-only listing of concrete hosts from OpenSSH configuration.
//!
//! Enumerating candidates must not run `ssh`, evaluate a `Match`, touch a key,
//! or expand a `%` token. This parser reads the files, keeps only concrete
//! `Host` aliases (wildcards, negations and `Match` blocks are skipped), and
//! marks the result incomplete when a limit or an unreadable include prevents a
//! full answer. A candidate is a hint for the picker, never proof the machine
//! exists.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::RemoteError;

const MAX_FILES: usize = 32;
const MAX_BYTES: usize = 1024 * 1024;
const MAX_DEPTH: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshConfigHost {
  pub alias: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub host_name: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub user: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub port: Option<u16>,
  pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshConfigListing {
  pub config_path: String,
  pub hosts: Vec<SshConfigHost>,
  /// False when a limit, an unreadable include, or a syntax error means the
  /// listing may be missing aliases. Never means "no hosts exist".
  pub complete: bool,
  pub problems: Vec<String>,
}

/// File access the parser is allowed to perform. Tests inject a map.
pub trait ConfigIo {
  fn read(&mut self, path: &Path) -> Option<String>;
  fn list(&mut self, path: &Path) -> Vec<String>;
}

/// Loads `~/.ssh/config` for `home`.
pub fn load(home: &Path) -> SshConfigListing {
  let path = home.join(".ssh").join("config");
  let mut reader = FsReader {
    reads: HashMap::new(),
  };
  parse_with_reader(&path.to_string_lossy(), &path, home, &mut reader)
}

/// Testable entry point: all file access goes through [`ConfigIo`].
pub fn parse_with_reader(
  config_path: &str,
  path: &Path,
  home: &Path,
  io: &mut impl ConfigIo,
) -> SshConfigListing {
  let mut parser = Parser {
    io,
    home: home.to_path_buf(),
    visited: HashSet::new(),
    files: 0,
    bytes: 0,
    complete: true,
    problems: Vec::new(),
    hosts: Vec::new(),
  };
  parser.file(path, true, 0);
  SshConfigListing {
    config_path: config_path.to_string(),
    hosts: parser.hosts,
    complete: parser.complete,
    problems: parser.problems,
  }
}

struct Parser<'a> {
  io: &'a mut dyn ConfigIo,
  home: PathBuf,
  visited: HashSet<PathBuf>,
  files: usize,
  bytes: usize,
  complete: bool,
  problems: Vec<String>,
  hosts: Vec<SshConfigHost>,
}

#[derive(Default)]
struct Block {
  aliases: Vec<String>,
  host_name: Option<String>,
  user: Option<String>,
  port: Option<u16>,
  source: String,
}

impl Parser<'_> {
  fn file(&mut self, path: &Path, required: bool, depth: usize) {
    if depth > MAX_DEPTH {
      self.problem(format!("include depth exceeded at {}", path.display()));
      return;
    }
    if self.files >= MAX_FILES {
      self.problem(format!(
        "more than {MAX_FILES} configuration files; listing truncated"
      ));
      return;
    }
    let normalized = normalize(path);
    if !self.visited.insert(normalized.clone()) {
      self.problem(format!("include cycle at {}", path.display()));
      return;
    }
    let Some(text) = self.io.read(&normalized) else {
      if required {
        self.problem(format!("cannot read {}", path.display()));
      }
      return;
    };
    self.files += 1;
    self.bytes += text.len();
    if self.bytes > MAX_BYTES {
      self.problem(format!(
        "configuration larger than {MAX_BYTES} bytes; listing truncated"
      ));
      return;
    }
    self.text(&text, path);
  }

  fn text(&mut self, text: &str, source: &Path) {
    let mut block: Option<Block> = None;
    let mut skipping_match = false;

    let flush = |block: &mut Option<Block>, hosts: &mut Vec<SshConfigHost>| {
      if let Some(block) = block.take() {
        let source = block.source.clone();
        for alias in block.aliases {
          if hosts.iter().any(|host| host.alias == alias) {
            continue;
          }
          hosts.push(SshConfigHost {
            alias,
            host_name: block.host_name.clone(),
            user: block.user.clone(),
            port: block.port,
            source: source.clone(),
          });
        }
      }
    };

    for raw in text.lines() {
      let tokens = tokenize(raw);
      let Some(keyword) = tokens.first() else {
        continue;
      };
      let key = keyword.to_ascii_lowercase();
      match key.as_str() {
        "host" => {
          flush(&mut block, &mut self.hosts);
          skipping_match = false;
          let mut next = Block {
            source: source.display().to_string(),
            ..Block::default()
          };
          for arg in tokens.iter().skip(1) {
            if arg.starts_with('!') || arg.contains('*') || arg.contains('?') || arg.contains('%') {
              continue;
            }
            next.aliases.push(arg.clone());
          }
          block = Some(next);
        }
        "match" => {
          flush(&mut block, &mut self.hosts);
          skipping_match = true;
        }
        "include" => {
          if skipping_match {
            continue;
          }
          for pattern in tokens.iter().skip(1) {
            for include in self.expand(pattern) {
              self.file(&include, false, 0);
            }
          }
        }
        _ if skipping_match => {}
        "hostname" => {
          if let Some(block) = block.as_mut() {
            if block.host_name.is_none() {
              block.host_name = tokens.get(1).cloned();
            }
          }
        }
        "user" => {
          if let Some(block) = block.as_mut() {
            if block.user.is_none() {
              block.user = tokens.get(1).cloned();
            }
          }
        }
        "port" => {
          if let Some(block) = block.as_mut() {
            if block.port.is_none() {
              block.port = tokens.get(1).and_then(|value| value.parse().ok());
            }
          }
        }
        _ => {}
      }
    }
    flush(&mut block, &mut self.hosts);
  }

  fn expand(&mut self, pattern: &str) -> Vec<PathBuf> {
    let expanded = match pattern.strip_prefix("~/") {
      Some(rest) => self.home.join(rest),
      None => self.home.join(".ssh").join(pattern),
    };
    let has_glob = expanded
      .file_name()
      .map(|name| {
        let name = name.to_string_lossy();
        name.contains('*') || name.contains('?')
      })
      .unwrap_or(false)
      || expanded.components().any(|component| {
        let value = component.as_os_str().to_string_lossy();
        value.contains('*') || value.contains('?')
      });
    if !has_glob {
      return vec![expanded];
    }
    let Some(parent) = expanded.parent() else {
      return Vec::new();
    };
    let Some(name) = expanded
      .file_name()
      .map(|name| name.to_string_lossy().to_string())
    else {
      return Vec::new();
    };
    let mut matches: Vec<PathBuf> = self
      .io
      .list(parent)
      .into_iter()
      .filter(|entry| wildcard_match(&name, entry))
      .map(|entry| parent.join(entry))
      .collect();
    matches.sort();
    matches
  }

  fn problem(&mut self, message: String) {
    self.complete = false;
    if self.problems.len() < 16 {
      self.problems.push(message);
    }
  }
}

fn normalize(path: &Path) -> PathBuf {
  path.components().collect()
}

fn tokenize(line: &str) -> Vec<String> {
  let mut tokens = Vec::new();
  let mut current = String::new();
  let mut quote: Option<char> = None;
  let mut escaped = false;
  for c in line.chars() {
    match quote {
      Some(q) => {
        if escaped {
          current.push(c);
          escaped = false;
        } else if c == '\\' && q == '"' {
          escaped = true;
        } else if c == q {
          quote = None;
        } else {
          current.push(c);
        }
      }
      None => {
        if c == '"' || c == '\'' {
          quote = Some(c);
        } else if c == '#' {
          break;
        } else if c.is_whitespace() {
          if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
          }
        } else {
          current.push(c);
        }
      }
    }
  }
  if !current.is_empty() {
    tokens.push(current);
  }
  tokens
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
  fn go(p: &[u8], v: &[u8]) -> bool {
    match p.first() {
      None => v.is_empty(),
      Some(b'*') => go(&p[1..], v) || (!v.is_empty() && go(p, &v[1..])),
      Some(b'?') => !v.is_empty() && go(&p[1..], &v[1..]),
      Some(c) => v.first() == Some(c) && go(&p[1..], &v[1..]),
    }
  }
  go(pattern.as_bytes(), value.as_bytes())
}

/// Real filesystem access, cached per listing.
struct FsReader {
  reads: HashMap<PathBuf, Option<String>>,
}

impl ConfigIo for FsReader {
  fn read(&mut self, path: &Path) -> Option<String> {
    if let Some(cached) = self.reads.get(path) {
      return cached.clone();
    }
    let value = std::fs::read_to_string(path).ok().filter(|text| {
      // A binary or a file with NULs cannot be an SSH config.
      !text.contains('\0')
    });
    self.reads.insert(path.to_path_buf(), value.clone());
    value
  }

  fn list(&mut self, path: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(path) else {
      return Vec::new();
    };
    entries
      .flatten()
      .map(|entry| entry.file_name().to_string_lossy().to_string())
      .collect()
  }
}

/// Validates the shell reported by the probe. Only POSIX-compatible login
/// shells are accepted: the command string is written for POSIX `sh` and is
/// re-parsed by the login shell before `sh -c` sees it.
pub fn shell_supported(shell: &str) -> bool {
  let name = shell.rsplit('/').next().unwrap_or(shell);
  matches!(name, "sh" | "bash" | "zsh" | "dash" | "ksh")
}

pub fn load_or_error(home: &Path) -> Result<SshConfigListing, RemoteError> {
  let listing = load(home);
  if listing.hosts.is_empty() && !listing.complete {
    return Err(RemoteError::HostUnavailable(format!(
      "cannot read SSH configuration: {}",
      listing.problems.join("; ")
    )));
  }
  Ok(listing)
}

#[cfg(test)]
mod tests {
  use super::*;

  struct MapIo {
    files: HashMap<PathBuf, String>,
    dirs: HashMap<PathBuf, Vec<String>>,
  }

  impl ConfigIo for MapIo {
    fn read(&mut self, path: &Path) -> Option<String> {
      self.files.get(path).cloned()
    }
    fn list(&mut self, path: &Path) -> Vec<String> {
      self.dirs.get(path).cloned().unwrap_or_default()
    }
  }

  fn fixture(files: &[(&str, &str)]) -> SshConfigListing {
    let mut io = MapIo {
      files: files
        .iter()
        .map(|(path, text)| (PathBuf::from(path), (*text).to_string()))
        .collect(),
      dirs: HashMap::new(),
    };
    parse_with_reader(
      "/home/alice/.ssh/config",
      Path::new("/home/alice/.ssh/config"),
      Path::new("/home/alice"),
      &mut io,
    )
  }

  fn aliases(listing: &SshConfigListing) -> Vec<&str> {
    listing
      .hosts
      .iter()
      .map(|host| host.alias.as_str())
      .collect()
  }

  #[test]
  fn lists_concrete_hosts_and_skips_patterns() {
    let listing = fixture(&[(
      "/home/alice/.ssh/config",
      "\
Host refyard
  HostName refyard.example.com
  User alice

Host work-* !work-secret ?single %h
Host box2
  Port 2222
",
    )]);
    assert_eq!(aliases(&listing), vec!["refyard", "box2"]);
    assert!(listing.complete);
    assert_eq!(
      listing.hosts[0].host_name.as_deref(),
      Some("refyard.example.com")
    );
    assert_eq!(listing.hosts[0].user.as_deref(), Some("alice"));
    assert_eq!(listing.hosts[1].port, Some(2222));
  }

  #[test]
  fn match_and_comments_do_not_leak_hosts() {
    let listing = fixture(&[(
      "/home/alice/.ssh/config",
      "\
# a comment
Host visible
  HostName \"quoted host\" # trailing comment

Match host secret exec \"true\"
  User nobody

Host after
",
    )]);
    assert_eq!(aliases(&listing), vec!["visible", "after"]);
    assert_eq!(listing.hosts[0].host_name.as_deref(), Some("quoted host"));
  }

  #[test]
  fn first_value_in_a_block_wins() {
    let listing = fixture(&[(
      "/home/alice/.ssh/config",
      "Host box\n  User first\n  User second\n  Port 2200\n  Port 2201\n",
    )]);
    assert_eq!(listing.hosts[0].user.as_deref(), Some("first"));
    assert_eq!(listing.hosts[0].port, Some(2200));
  }

  #[test]
  fn includes_are_read_and_cycles_reported() {
    let mut io = MapIo {
      files: [
        (
          PathBuf::from("/home/alice/.ssh/config"),
          "Include config.d/*\nHost primary\n".to_string(),
        ),
        (
          PathBuf::from("/home/alice/.ssh/config.d/10-work"),
          "Host work\n  Include config\n".to_string(),
        ),
      ]
      .into_iter()
      .collect(),
      dirs: [(
        PathBuf::from("/home/alice/.ssh/config.d"),
        vec!["10-work".to_string()],
      )]
      .into_iter()
      .collect(),
    };
    let listing = parse_with_reader(
      "/home/alice/.ssh/config",
      Path::new("/home/alice/.ssh/config"),
      Path::new("/home/alice"),
      &mut io,
    );
    assert_eq!(aliases(&listing), vec!["work", "primary"]);
    assert!(!listing.complete);
    assert!(listing
      .problems
      .iter()
      .any(|problem| problem.contains("cycle")));
  }

  #[test]
  fn duplicate_aliases_list_once() {
    let listing = fixture(&[(
      "/home/alice/.ssh/config",
      "Host box\nHost box\n  User alice\n",
    )]);
    assert_eq!(aliases(&listing), vec!["box"]);
    assert_eq!(listing.hosts[0].user, None);
  }

  #[test]
  fn unreadable_primary_config_is_incomplete() {
    let listing = fixture(&[]);
    assert!(listing.hosts.is_empty());
    assert!(!listing.complete);
    assert!(listing.problems[0].contains("cannot read"));
  }

  #[test]
  fn only_posix_shells_are_accepted() {
    for shell in [
      "/bin/sh",
      "/bin/bash",
      "/usr/bin/zsh",
      "/bin/dash",
      "ksh",
      "/usr/local/bin/fish",
    ] {
      assert_eq!(
        shell_supported(shell),
        shell != "/usr/local/bin/fish",
        "{shell}"
      );
    }
    assert!(!shell_supported("cmd.exe"));
    assert!(!shell_supported(""));
  }

  #[test]
  fn wildcard_matching_handles_star_and_question() {
    assert!(wildcard_match("*", "10-work"));
    assert!(!wildcard_match("*.conf", "10-work"));
    assert!(wildcard_match("host-?", "host-a"));
    assert!(!wildcard_match("host-?", "host-ab"));
    assert!(wildcard_match("*", "anything"));
  }
}
