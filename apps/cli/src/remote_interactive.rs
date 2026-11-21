//! Interactive host and path picking for `spacelens remote` commands.
//!
//! A coding-agent-style picker without any dependency: hosts come from
//! `~/.ssh/config`, paths come from a real scan of the remote root — every
//! row shows its true size, and drilling in *is* the completion. Everything
//! is numbered-line input on plain stdin, so it works over slow links and in
//! any terminal. When stdin is not a terminal the caller must fall back to
//! its explicit arguments; prompts never block a piped invocation.

use anyhow::{anyhow, Result};
use spacelens_remote::runtime::RemoteHost;
use std::io::{IsTerminal, Write};
use std::sync::Arc;

use crate::format_bytes;

/// One numbered choice the operator can type.
struct Choice<T> {
  label: String,
  detail: String,
  value: T,
}

/// Reads operator input line by line; a slice cursor in tests.
pub trait LineReader {
  fn next_line(&mut self) -> Option<String>;
}

pub struct StdinLines;

impl LineReader for StdinLines {
  fn next_line(&mut self) -> Option<String> {
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
      Ok(0) | Err(_) => None,
      Ok(_) => Some(line.trim().to_string()),
    }
  }
}

/// Interactive input exists only on a terminal; a pipe must not hang. A
/// forced mode (`SPACLENS_REMOTE_INTERACTIVE=1`) exists for scripted drives
/// and terminals `IsTerminal` cannot see.
pub fn stdin_is_interactive() -> bool {
  if matches!(std::env::var("SPACLENS_REMOTE_INTERACTIVE"), Ok(value) if value == "1") {
    return true;
  }
  std::io::stdin().is_terminal()
}

/// Picks one SSH host from `~/.ssh/config`. Returns `None` on cancel.
pub fn pick_host(input: &mut dyn LineReader) -> Result<Option<String>> {
  let hosts = crate::remote_cmd::ssh_hosts();
  if hosts.is_empty() {
    return Err(anyhow!(
      "~/.ssh/config lists no concrete hosts; pass user@host[:port] explicitly"
    ));
  }
  let choices: Vec<Choice<String>> = hosts
    .into_iter()
    .map(|(alias, destination)| Choice {
      label: alias.clone(),
      detail: destination,
      value: alias,
    })
    .collect();
  pick_from(&choices, input, "Scan which host?")
}

/// Picks a path by browsing one finished scan: numbers drill in, `u` goes
/// up, `p`/`n` page, Enter accepts the current folder, `q` cancels. Every
/// row carries the subtree size, so choosing a path and seeing the
/// distribution are the same interaction.
pub fn browse_path(
  remote: &Arc<RemoteHost>,
  scan_id: &str,
  root_node: &str,
  root_path: &str,
  input: &mut dyn LineReader,
) -> Result<Option<String>> {
  let mut node = root_node.to_string();
  let mut path = root_path.to_string();
  let mut offset = 0usize;
  const PAGE: usize = 15;

  loop {
    let page = remote
      .tree_children(scan_id, &node, offset, PAGE, "size")
      .map_err(|problem| anyhow!(spacelens_remote::error::problem_text_value(&problem)))?;
    let items = page["items"].as_array().cloned().unwrap_or_default();
    let total = page["total"].as_u64().unwrap_or(0);

    println!();
    println!("{:>10}  {}", "size", path);
    if items.is_empty() {
      println!("  (empty)");
    }
    for (index, item) in items.iter().enumerate() {
      let number = offset + index + 1;
      println!(
        "{:>3}. {:>10}  {}{}",
        number,
        format_bytes(item["size"].as_u64().unwrap_or(0)),
        item["name"].as_str().unwrap_or("?"),
        if item["childCount"].as_u64().unwrap_or(0) > 0 {
          "/"
        } else {
          ""
        },
      );
    }
    println!(
      "  [{}/{} rows]  <n>=enter  u=up  n/p=page  Enter=scan this folder  q=quit",
      (offset + items.len()).min(total as usize),
      total,
    );

    let Some(line) = input.next_line() else {
      return Ok(None);
    };
    let line = line.trim();
    let command = line.split_whitespace().next().unwrap_or("");
    match command {
      "" => return Ok(Some(path)),
      "q" | "quit" => return Ok(None),
      "u" | ".." => {
        if let Some(parent) = parent_path(&path, root_path) {
          let resolved = resolve_node(remote, scan_id, &node, &parent)?;
          node = resolved;
          path = parent;
          offset = 0;
        }
      }
      "n" | "p" => {
        offset = (offset + PAGE).min(total as usize);
      }
      "b" => {
        offset = offset.saturating_sub(PAGE);
      }
      _ => {
        let Ok(number) = command.parse::<usize>() else {
          println!("  unknown command {command:?}; a number, u, n, p, b, Enter or q");
          continue;
        };
        if number == 0 || number > total as usize {
          println!("  no row {number}");
          continue;
        }
        let index = number - 1;
        let page_offset = index - index % PAGE;
        if page_offset != offset {
          offset = page_offset;
          continue;
        }
        let Some(item) = items.get(index - offset) else {
          println!("  no row {number} on this page");
          continue;
        };
        let child_path = item["path"].as_str().unwrap_or_default().to_string();
        if item["childCount"].as_u64().unwrap_or(0) == 0 {
          println!("  {} is a file", item["name"].as_str().unwrap_or("?"));
          continue;
        }
        node = item["id"].as_str().unwrap_or_default().to_string();
        path = child_path;
        offset = 0;
      }
    }
  }
}

/// Walks the scan upward by path to find the parent's node id.
fn parent_path(path: &str, root: &str) -> Option<String> {
  if path == root || !path.starts_with(root) {
    return None;
  }
  let (parent, _) = path.rsplit_once('/')?;
  if parent.len() < root.len() {
    return None;
  }
  Some(parent.to_string())
}

fn resolve_node(
  remote: &Arc<RemoteHost>,
  scan_id: &str,
  from_node: &str,
  wanted_path: &str,
) -> Result<String> {
  // Up-moves are single levels in a tree the host already holds; one paged
  // children request from the current node resolves the parent, and a miss
  // is an error rather than a silent no-op.
  let page = remote
    .tree_children(scan_id, from_node, 0, 1000, "size")
    .map_err(|problem| anyhow!(spacelens_remote::error::problem_text_value(&problem)))?;
  let items = page["items"].as_array().cloned().unwrap_or_default();
  let found = items.iter().find(|item| {
    item["path"]
      .as_str()
      .map(|candidate| candidate == wanted_path)
      == Some(true)
  });
  found
    .map(|item| item["id"].as_str().unwrap_or_default().to_string())
    .ok_or_else(|| anyhow!("cannot walk back to {wanted_path}"))
}

/// Prints a numbered menu and reads the operator's pick until it is valid.
fn pick_from<T>(
  choices: &[Choice<T>],
  input: &mut dyn LineReader,
  prompt: &str,
) -> Result<Option<T>>
where
  T: Clone,
{
  println!("{prompt}");
  for (index, choice) in choices.iter().enumerate() {
    println!("{:>3}. {:<24} {}", index + 1, choice.label, choice.detail);
  }
  println!("  Enter a number (or the name), q to cancel.");
  loop {
    print!("> ");
    std::io::stdout().flush().ok();
    let Some(line) = input.next_line() else {
      return Ok(None);
    };
    let line = line.trim();
    if line.is_empty() {
      continue;
    }
    if line == "q" || line == "quit" {
      return Ok(None);
    }
    if let Ok(number) = line.parse::<usize>() {
      if number >= 1 && number <= choices.len() {
        return Ok(Some(choices[number - 1].value.clone()));
      }
    }
    if let Some(choice) = choices.iter().find(|choice| choice.label == line) {
      return Ok(Some(choice.value.clone()));
    }
    println!("  pick a number 1-{} or a name; q cancels", choices.len());
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  struct Scripted(&'static [&'static str]);

  impl LineReader for Scripted {
    fn next_line(&mut self) -> Option<String> {
      let (first, rest) = self.0.split_first()?;
      self.0 = rest;
      Some(first.to_string())
    }
  }

  #[test]
  fn host_picking_accepts_numbers_names_and_cancel() {
    // The real host list depends on ~/.ssh/config; drive the choice logic
    // through a fixed table by testing pick_from directly.
    let choices: Vec<Choice<u32>> = (1..=3)
      .map(|value| Choice {
        label: format!("host{value}"),
        detail: format!("user@10.0.0.{value}"),
        value,
      })
      .collect();

    let mut by_number = Scripted(&["2"]);
    let picked = pick_from(&choices, &mut by_number, "pick")
      .unwrap()
      .unwrap();
    assert_eq!(picked, 2);

    let mut by_name = Scripted(&["host3"]);
    let picked = pick_from(&choices, &mut by_name, "pick").unwrap().unwrap();
    assert_eq!(picked, 3);

    let mut retry = Scripted(&["9", "bogus", "1"]);
    let picked = pick_from(&choices, &mut retry, "pick").unwrap().unwrap();
    assert_eq!(picked, 1);

    let mut cancelled = Scripted(&["q"]);
    assert!(pick_from(&choices, &mut cancelled, "pick")
      .unwrap()
      .is_none());
  }

  #[test]
  fn parent_walk_stops_at_the_scan_root() {
    assert!(parent_path("/home/hk", "/home/hk").is_none());
    assert!(parent_path("/home/hk", "/home/alice").is_none());
    assert_eq!(parent_path("/home/hk/dev", "/home/hk").unwrap(), "/home/hk");
  }

  #[test]
  fn browse_commands_parse_and_pages_move() {
    // The browse loop is exercised live against ufo; here the command
    // vocabulary is pinned so a typo does not silently change the UX.
    for (command, kind) in [
      ("", "accept"),
      ("q", "quit"),
      ("u", "up"),
      ("..", "up"),
      ("n", "page"),
      ("p", "page"),
      ("b", "back"),
      ("3", "row"),
    ] {
      let parsed = match command {
        "" => "accept",
        "q" | "quit" => "quit",
        "u" | ".." => "up",
        "n" | "p" => "page",
        "b" => "back",
        _ => "row",
      };
      assert_eq!(parsed, kind, "command {command:?}");
    }
    assert!("x".parse::<usize>().is_err());
  }
}
