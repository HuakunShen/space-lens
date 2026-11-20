//! The `spacelens agent-info` handshake and the pinned feature vocabulary.

use serde::{Deserialize, Serialize};

use crate::error::RemoteError;

pub const AGENT_PROTOCOL: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
  pub agent_protocol: u32,
  pub version: String,
  pub platform: String,
  pub arch: String,
  pub features: Vec<String>,
  #[serde(default)]
  pub home: Option<String>,
  #[serde(default)]
  pub path_separator: Option<String>,
  #[serde(default)]
  pub shell: Option<String>,
}

impl AgentInfo {
  pub fn parse(json: &str) -> Result<Self, RemoteError> {
    let info: Self = serde_json::from_str(json)
      .map_err(|error| RemoteError::Manifest(format!("agent-info is not valid JSON: {error}")))?;
    if info.agent_protocol != AGENT_PROTOCOL {
      return Err(RemoteError::Manifest(format!(
        "agent protocol {} is not supported (expected {AGENT_PROTOCOL})",
        info.agent_protocol
      )));
    }
    if info.version.is_empty() || info.features.is_empty() {
      return Err(RemoteError::Manifest(
        "agent-info is missing its version or features".into(),
      ));
    }
    Ok(info)
  }

  pub fn supports(&self, feature: &str) -> bool {
    self.features.iter().any(|value| value == feature)
  }

  /// The app requires the intersection it was built against. Missing features
  /// produce one message naming all of them, not the first.
  pub fn require(&self, required: &[&str]) -> Result<(), RemoteError> {
    let missing: Vec<&str> = required
      .iter()
      .copied()
      .filter(|feature| !self.supports(feature))
      .collect();
    if missing.is_empty() {
      Ok(())
    } else {
      Err(RemoteError::Manifest(format!(
        "agent {} lacks: {}",
        self.version,
        missing.join(", ")
      )))
    }
  }
}

/// Features the desktop needs for the read-only remote beta. The agent's list
/// is additive; this set is the minimum, not an equality check.
pub const REQUIRED_FEATURES: &[&str] = &["scan.stream", "scan.progress", "memory.stdio"];

/// The same requirement, scoped to one remote platform: the Windows agent
/// does not carry the memory stdio bridge (`cfg(unix)` in the CLI), so a
/// Windows session must not demand it and its host serves without the
/// memory view.
pub fn required_features_for(platform: &str) -> &'static [&'static str] {
  if platform == "windows" {
    &["scan.stream", "scan.progress"]
  } else {
    REQUIRED_FEATURES
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn info(protocol: u32, features: &[&str]) -> String {
    serde_json::json!({
      "agentProtocol": protocol,
      "version": "0.4.0",
      "platform": "linux",
      "arch": "x86_64",
      "features": features,
    })
    .to_string()
  }

  #[test]
  fn parses_the_handshake() {
    let parsed =
      AgentInfo::parse(&info(1, &["scan.stream", "scan.progress", "memory.stdio"])).unwrap();
    assert_eq!(parsed.version, "0.4.0");
    parsed.require(REQUIRED_FEATURES).unwrap();
    assert!(!parsed.supports("app-audit"));
  }

  #[test]
  fn protocol_mismatch_is_refused() {
    let error = AgentInfo::parse(&info(2, &["scan.stream"])).unwrap_err();
    assert!(error.to_string().contains("protocol"));
  }

  #[test]
  fn a_missing_feature_is_named() {
    let parsed = AgentInfo::parse(&info(1, &["scan.stream"])).unwrap();
    let error = parsed.require(REQUIRED_FEATURES).unwrap_err();
    assert!(error.to_string().contains("scan.progress"));
    assert!(error.to_string().contains("memory.stdio"));
  }

  #[test]
  fn windows_sessions_do_not_demand_the_memory_bridge() {
    assert_eq!(
      required_features_for("windows"),
      &["scan.stream", "scan.progress"] as &'static [&'static str]
    );
    assert_eq!(required_features_for("linux"), REQUIRED_FEATURES);
    assert_eq!(required_features_for("macos"), REQUIRED_FEATURES);
  }
}
