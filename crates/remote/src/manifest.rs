//! The pinned agent manifest the desktop embeds.
//!
//! GitHub's per-asset `digest` is not an authorization source: it lives in the
//! same trust domain as the asset and can be replaced with it. The manifest is
//! compiled into the app, so an install is authorized by what shipped, not by
//! what a server says today. A development build with no embedded hashes can
//! only use an explicitly configured local agent path.

use std::collections::BTreeMap;
use std::io::Read;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::RemoteError;

pub const PACKAGE: &str = "@space-lens/cli";
pub const SCHEMA: u32 = 1;
const MAX_AGENT_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentManifest {
  pub schema: u32,
  pub host: HostPin,
  pub agent: AgentPin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostPin {
  pub package: String,
  pub version: String,
  pub api_major: u32,
  pub min_node: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentPin {
  pub version: String,
  pub protocol: u32,
  pub features: Vec<String>,
  pub targets: BTreeMap<String, AgentArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentArtifact {
  pub archive: String,
  pub sha256: String,
  pub size: u64,
}

impl AgentManifest {
  pub fn parse(json: &str) -> Result<Self, RemoteError> {
    let manifest: Self = serde_json::from_str(json)
      .map_err(|error| RemoteError::Manifest(format!("manifest is not valid: {error}")))?;
    manifest.validate()?;
    Ok(manifest)
  }

  pub fn validate(&self) -> Result<(), RemoteError> {
    if self.schema != SCHEMA {
      return Err(RemoteError::Manifest(format!(
        "unsupported manifest schema {} (expected {SCHEMA})",
        self.schema
      )));
    }
    if self.host.package != PACKAGE {
      return Err(RemoteError::Manifest(format!(
        "host package is {:?}, expected {PACKAGE}",
        self.host.package
      )));
    }
    validate_version("host version", &self.host.version)?;
    validate_version("agent version", &self.agent.version)?;
    if self.host.api_major == 0 || self.agent.protocol == 0 {
      return Err(RemoteError::Manifest(
        "protocol numbers must be positive".into(),
      ));
    }
    if self.host.min_node == 0 {
      return Err(RemoteError::Manifest("minNode must be positive".into()));
    }
    if self.agent.features.is_empty() {
      return Err(RemoteError::Manifest(
        "agent features must not be empty".into(),
      ));
    }
    let mut seen = std::collections::HashSet::new();
    for feature in &self.agent.features {
      if feature.is_empty()
        || !feature
          .chars()
          .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
      {
        return Err(RemoteError::Manifest(format!(
          "invalid feature name: {feature:?}"
        )));
      }
      if !seen.insert(feature) {
        return Err(RemoteError::Manifest(format!(
          "duplicate feature: {feature}"
        )));
      }
    }
    for (target, artifact) in &self.agent.targets {
      if target.is_empty()
        || !target
          .chars()
          .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
      {
        return Err(RemoteError::Manifest(format!("invalid target: {target:?}")));
      }
      if artifact.size == 0 || artifact.size > MAX_AGENT_BYTES {
        return Err(RemoteError::Manifest(format!(
          "{target}: size {} is outside 1..={MAX_AGENT_BYTES}",
          artifact.size
        )));
      }
      if artifact.sha256.len() != 64 || !artifact.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(RemoteError::Manifest(format!(
          "{target}: sha256 must be 64 hex digits"
        )));
      }
      let archive = &artifact.archive;
      if archive.is_empty()
        || archive.contains('/')
        || archive.contains('\\')
        || archive.contains("..")
        || archive.chars().any(char::is_whitespace)
        // cli-release.yml ships `.tar.gz` on POSIX targets and `.zip` on
        // Windows; both are extracted locally under the same security rules.
        || !(archive.ends_with(".tar.gz") || archive.ends_with(".zip"))
      {
        return Err(RemoteError::Manifest(format!(
          "{target}: unsafe archive name {archive:?}"
        )));
      }
    }
    Ok(())
  }

  pub fn artifact(&self, target: &str) -> Result<&AgentArtifact, RemoteError> {
    self.agent.targets.get(target).ok_or_else(|| {
      RemoteError::UnsupportedTarget(format!(
        "manifest pins no agent for {target}; this desktop cannot install it"
      ))
    })
  }

  pub fn download_url(&self, artifact: &AgentArtifact) -> String {
    format!(
      "https://github.com/HuakunShen/space-lens/releases/download/cli-v{}/{}",
      self.agent.version, artifact.archive
    )
  }

  pub fn verify_artifact(&self, artifact: &AgentArtifact, bytes: &[u8]) -> Result<(), RemoteError> {
    if bytes.len() as u64 != artifact.size {
      return Err(RemoteError::Manifest(format!(
        "agent size mismatch: expected {} bytes, downloaded {}",
        artifact.size,
        bytes.len()
      )));
    }
    let digest = sha256_hex(bytes);
    if !digest.eq_ignore_ascii_case(&artifact.sha256) {
      return Err(RemoteError::Manifest(format!(
        "agent checksum mismatch: expected {}, got {digest}",
        artifact.sha256
      )));
    }
    Ok(())
  }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
  let digest = Sha256::digest(bytes);
  let mut out = String::with_capacity(64);
  for byte in digest {
    use std::fmt::Write as _;
    let _ = write!(out, "{byte:02x}");
  }
  out
}

pub fn sha256_reader(reader: &mut impl Read) -> Result<String, RemoteError> {
  let mut hasher = Sha256::new();
  let mut buffer = [0u8; 64 * 1024];
  loop {
    let read = reader.read(&mut buffer)?;
    if read == 0 {
      break;
    }
    hasher.update(&buffer[..read]);
  }
  let digest = hasher.finalize();
  let mut out = String::with_capacity(64);
  for byte in digest {
    use std::fmt::Write as _;
    let _ = write!(out, "{byte:02x}");
  }
  Ok(out)
}

fn validate_version(what: &str, version: &str) -> Result<(), RemoteError> {
  let core = version
    .split_once('-')
    .map(|(core, _)| core)
    .unwrap_or(version);
  let parts: Vec<&str> = core.split('.').collect();
  if parts.len() != 3
    || parts
      .iter()
      .any(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()))
  {
    return Err(RemoteError::Manifest(format!(
      "{what} is not a semantic version: {version:?}"
    )));
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn manifest_json() -> String {
    r#"{
      "schema": 1,
      "host": { "package": "@space-lens/cli", "version": "0.4.0", "apiMajor": 1, "minNode": 20 },
      "agent": {
        "version": "0.4.0",
        "protocol": 1,
        "features": ["scan.stream", "memory.stdio"],
        "targets": {
          "x86_64-unknown-linux-musl": {
            "archive": "spacelens-x86_64-unknown-linux-musl-v0.4.0.tar.gz",
            "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "size": 4
          },
          "x86_64-pc-windows-msvc": {
            "archive": "spacelens-x86_64-pc-windows-msvc-v0.4.0.zip",
            "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "size": 4
          }
        }
      }
    }"#
      .to_string()
  }

  #[test]
  fn windows_targets_pin_zip_archives() {
    let manifest = AgentManifest::parse(&manifest_json()).unwrap();
    let artifact = manifest.artifact("x86_64-pc-windows-msvc").unwrap();
    assert_eq!(
      artifact.archive,
      "spacelens-x86_64-pc-windows-msvc-v0.4.0.zip"
    );
    assert_eq!(
      manifest.download_url(artifact),
      "https://github.com/HuakunShen/space-lens/releases/download/cli-v0.4.0/spacelens-x86_64-pc-windows-msvc-v0.4.0.zip"
    );
  }

  #[test]
  fn parses_and_verifies_a_pinned_manifest() {
    let manifest = AgentManifest::parse(&manifest_json()).unwrap();
    assert_eq!(manifest.agent.version, "0.4.0");
    let artifact = manifest.artifact("x86_64-unknown-linux-musl").unwrap();
    assert_eq!(
      artifact.archive,
      "spacelens-x86_64-unknown-linux-musl-v0.4.0.tar.gz"
    );
    // The fixture digest is fake, so a real payload must fail closed.
    assert!(manifest.verify_artifact(artifact, b"data").is_err());
    let mut artifact = artifact.clone();
    artifact.sha256 = sha256_hex(b"data");
    manifest.verify_artifact(&artifact, b"data").unwrap();
    assert!(manifest.verify_artifact(&artifact, b"dat").is_err());
  }

  #[test]
  fn rejects_unknown_targets() {
    let manifest = AgentManifest::parse(&manifest_json()).unwrap();
    let error = manifest
      .artifact("riscv64gc-unknown-linux-gnu")
      .unwrap_err();
    assert_eq!(error.code(), "unsupported-target");
  }

  #[test]
  fn rejects_bad_schemas_versions_and_digests() {
    for (needle, replacement) in [
      (r#""schema": 1"#, r#""schema": 2"#),
      (
        r#""version": "0.4.0", "apiMajor""#,
        r#""version": "next", "apiMajor""#,
      ),
      (r#""minNode": 20"#, r#""minNode": 0"#),
      (
        r#""features": ["scan.stream", "memory.stdio"]"#,
        r#""features": []"#,
      ),
      (r#""size": 4"#, r#""size": 0"#),
      (r#""size": 4"#, r#""size": 999999999999"#),
      (
        r#""archive": "spacelens-x86_64-unknown-linux-musl-v0.4.0.tar.gz""#,
        r#""archive": "../escape.tar.gz""#,
      ),
      (
        r#""archive": "spacelens-x86_64-pc-windows-msvc-v0.4.0.zip""#,
        r#""archive": "../escape.zip""#,
      ),
      (
        r#""archive": "spacelens-x86_64-pc-windows-msvc-v0.4.0.zip""#,
        r#""archive": "spacelens-x86_64-pc-windows-msvc-v0.4.0.rar""#,
      ),
      (r#""sha256": "aaaa"#, r#""sha256": "zzzz"#),
    ] {
      let broken = manifest_json().replace(needle, replacement);
      assert!(
        AgentManifest::parse(&broken).is_err(),
        "accepted replacement {replacement}"
      );
    }
  }

  #[test]
  fn download_url_is_the_pinned_release() {
    let manifest = AgentManifest::parse(&manifest_json()).unwrap();
    let artifact = manifest.artifact("x86_64-unknown-linux-musl").unwrap();
    assert_eq!(
      manifest.download_url(artifact),
      "https://github.com/HuakunShen/space-lens/releases/download/cli-v0.4.0/spacelens-x86_64-unknown-linux-musl-v0.4.0.tar.gz"
    );
  }
}
