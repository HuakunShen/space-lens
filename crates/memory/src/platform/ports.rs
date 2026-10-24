//! Listening TCP ports per process (macOS).
//!
//! The memory view wants to answer "which process is holding my port?" without
//! making the sampler walk every process's file descriptors itself. `lsof`
//! already does that walk in ~60 ms on a normal machine, so this module runs it
//! at most once every ten seconds and caches the pid → ports mapping for the
//! ticks in between.
//!
//! Only listening TCP sockets are reported. A failure (lsof missing, refused,
//! malformed output) yields an empty map and never fails the sample: ports are
//! evidence for the stale-process view, not a requirement of it.

use std::collections::HashMap;
use std::process::Command;
use std::time::{Duration, Instant};

/// How long one lsof reading is reused.
const REFRESH_EVERY: Duration = Duration::from_secs(10);
/// The most ports one process reports; a port range belongs in the command.
const MAX_PORTS: usize = 32;

/// The cached pid → listening TCP ports map.
#[derive(Debug)]
pub struct ListeningPorts {
  fetched_at: Option<Instant>,
  by_pid: HashMap<u32, Vec<u16>>,
}

impl ListeningPorts {
  /// An empty cache that refreshes on its first use.
  pub fn new() -> Self {
    ListeningPorts {
      fetched_at: None,
      by_pid: HashMap::new(),
    }
  }

  /// Refresh the map when the cached reading is older than [`REFRESH_EVERY`].
  pub fn refresh_if_stale(&mut self) {
    let now = Instant::now();
    if self
      .fetched_at
      .is_some_and(|fetched| now.duration_since(fetched) < REFRESH_EVERY)
    {
      return;
    }
    self.by_pid = read_listening_ports();
    self.fetched_at = Some(now);
  }

  /// The listening ports of `pid`, sorted ascending, or an empty slice.
  pub fn for_pid(&self, pid: u32) -> &[u16] {
    self.by_pid.get(&pid).map_or(&[], Vec::as_slice)
  }
}

impl Default for ListeningPorts {
  fn default() -> Self {
    ListeningPorts::new()
  }
}

/// Run lsof and parse its `-Fpcn` output.
fn read_listening_ports() -> HashMap<u32, Vec<u16>> {
  let output = match Command::new("/usr/sbin/lsof")
    .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpcn"])
    .output()
  {
    Ok(output) => output,
    Err(_) => return HashMap::new(),
  };
  if !output.status.success() {
    // lsof exits 1 when its last probe found nothing; stdout is still useful.
    // A real failure (permission, missing binary) also just yields no ports.
  }
  parse_lsof(&String::from_utf8_lossy(&output.stdout))
}

/// Parse lsof's field output: `p<pid>`, `c<command>`, `f<fd>`, `n<address>`.
/// Only the `p` and `n` records matter here.
fn parse_lsof(output: &str) -> HashMap<u32, Vec<u16>> {
  let mut by_pid: HashMap<u32, Vec<u16>> = HashMap::new();
  let mut pid: Option<u32> = None;
  for line in output.lines() {
    if let Some(raw) = line.strip_prefix('p') {
      pid = raw.trim().parse().ok();
      continue;
    }
    let Some(raw) = line.strip_prefix('n') else {
      continue;
    };
    let Some(pid) = pid else {
      continue;
    };
    let Some(port) = port_from_address(raw) else {
      continue;
    };
    let ports = by_pid.entry(pid).or_default();
    if ports.len() < MAX_PORTS && !ports.contains(&port) {
      ports.push(port);
    }
  }
  for ports in by_pid.values_mut() {
    ports.sort_unstable();
  }
  by_pid
}

/// The port of an lsof `n` address such as `*:7000`, `127.0.0.1:7265` or
/// `[::1]:3080`. A connected pair (`a->b`) is not a listening socket.
fn port_from_address(address: &str) -> Option<u16> {
  if address.contains("->") {
    return None;
  }
  let (_, port) = address.rsplit_once(':')?;
  port.trim().parse().ok()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_pid_port_records_and_deduplicates() {
    let ports = parse_lsof(
      "p718\ncControlCenter\nf9\nn*:7000\nf10\nn*:7000\np14337\ncnode\nf20\nn127.0.0.1:3080\nf21\nn[::1]:8326\n",
    );
    assert_eq!(ports.get(&718), Some(&vec![7000]));
    assert_eq!(ports.get(&14337), Some(&vec![3080, 8326]));
  }

  #[test]
  fn ignores_connected_pairs_and_non_numeric_ports() {
    let ports = parse_lsof("p1\nn127.0.0.1:5000->10.0.0.2:6000\np2\nn*:http-alt\n");
    assert!(ports.is_empty());
  }

  #[test]
  fn keeps_the_first_thirty_two_ports() {
    let mut output = String::from("p9\n");
    for port in 1..=40 {
      output.push_str(&format!("n*:{port}\n"));
    }
    let ports = parse_lsof(&output);
    assert_eq!(ports.get(&9).map(Vec::len), Some(MAX_PORTS));
  }
}
