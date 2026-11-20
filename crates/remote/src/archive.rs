//! Local extraction of the agent archive.
//!
//! The archive is unpacked by the desktop, never by the remote host: a hostile
//! archive therefore cannot write outside a staging directory, and the remote
//! needs no `tar`. Exactly one regular file is accepted, at a safe relative
//! path, with a bounded size. Links, devices, absolute paths, `..`, multiple
//! regular files, and anything but `spacelens`/`spacelens.exe` are refused.

use std::io::Read;
use std::path::Component;

use crate::error::RemoteError;

const MAX_AGENT_BYTES: u64 = 128 * 1024 * 1024;
const MAX_ENTRIES: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedAgent {
  /// The archive-relative path, for diagnostics only.
  pub archive_path: String,
  pub bytes: Vec<u8>,
}

/// Extracts exactly one agent binary from a `.tar.gz` payload.
pub fn extract_agent(archive: &[u8]) -> Result<ExtractedAgent, RemoteError> {
  let decoder = flate2::read::GzDecoder::new(archive);
  let mut tar = tar::Archive::new(decoder);
  let entries = tar
    .entries()
    .map_err(|error| RemoteError::Archive(format!("cannot read tar header: {error}")))?;

  let mut found: Option<ExtractedAgent> = None;
  let mut count = 0usize;
  for entry in entries {
    let mut entry =
      entry.map_err(|error| RemoteError::Archive(format!("cannot read tar entry: {error}")))?;
    count += 1;
    if count > MAX_ENTRIES {
      return Err(RemoteError::Archive(format!(
        "archive has more than {MAX_ENTRIES} entries"
      )));
    }
    let kind = entry.header().entry_type();
    match kind {
      tar::EntryType::Directory => continue,
      tar::EntryType::Regular => {}
      tar::EntryType::Symlink | tar::EntryType::Link => {
        return Err(RemoteError::Archive(
          "archive contains a link; only one regular file is allowed".into(),
        ))
      }
      other => {
        return Err(RemoteError::Archive(format!(
          "archive contains an unsupported entry type: {other:?}"
        )))
      }
    }

    let path = entry
      .path()
      .map_err(|error| RemoteError::Archive(format!("invalid archive path: {error}")))?
      .to_string_lossy()
      .to_string();
    let safe = safe_relative(&path)?;
    let name = safe
      .file_name()
      .map(|name| name.to_string_lossy().to_string())
      .unwrap_or_default();
    if name != "spacelens" && name != "spacelens.exe" {
      return Err(RemoteError::Archive(format!(
        "archive entry {path:?} is not the agent binary"
      )));
    }
    let declared = entry.header().size().unwrap_or(u64::MAX);
    if declared > MAX_AGENT_BYTES {
      return Err(RemoteError::Archive(format!(
        "agent is larger than {MAX_AGENT_BYTES} bytes"
      )));
    }
    if found.is_some() {
      return Err(RemoteError::Archive(
        "archive contains more than one regular file".into(),
      ));
    }
    let mut bytes = Vec::with_capacity(declared.min(MAX_AGENT_BYTES) as usize);
    entry
      .by_ref()
      .take(MAX_AGENT_BYTES + 1)
      .read_to_end(&mut bytes)
      .map_err(|error| RemoteError::Archive(format!("cannot read agent: {error}")))?;
    if bytes.len() as u64 > MAX_AGENT_BYTES {
      return Err(RemoteError::Archive(format!(
        "agent is larger than {MAX_AGENT_BYTES} bytes"
      )));
    }
    if bytes.is_empty() {
      return Err(RemoteError::Archive("agent binary is empty".into()));
    }
    found = Some(ExtractedAgent {
      archive_path: path,
      bytes,
    });
  }

  found.ok_or_else(|| RemoteError::Archive("archive contains no agent binary".into()))
}

fn safe_relative(path: &str) -> Result<std::path::PathBuf, RemoteError> {
  if path.is_empty() || path.len() > 4096 || path.contains('\0') {
    return Err(RemoteError::Archive(format!(
      "unsafe archive path: {path:?}"
    )));
  }
  let parsed = std::path::Path::new(path);
  if parsed.is_absolute() {
    return Err(RemoteError::Archive(format!(
      "absolute archive path: {path:?}"
    )));
  }
  for component in parsed.components() {
    match component {
      Component::Normal(value) if !value.is_empty() => {}
      _ => {
        return Err(RemoteError::Archive(format!(
          "unsafe archive path: {path:?}"
        )))
      }
    }
  }
  Ok(parsed.to_path_buf())
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::io::Write;

  fn archive(entries: &[(&str, &[u8], tar::EntryType)]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, bytes, kind) in entries {
      let mut header = tar::Header::new_gnu();
      header.set_path(path).unwrap();
      header.set_size(bytes.len() as u64);
      header.set_mode(0o755);
      header.set_entry_type(*kind);
      header.set_cksum();
      builder.append(&header, &bytes[..]).unwrap();
    }
    let tar_bytes = builder.into_inner().unwrap();
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar_bytes).unwrap();
    encoder.finish().unwrap()
  }

  #[test]
  fn extracts_one_agent_binary() {
    let payload = archive(&[(
      "spacelens-x86_64-unknown-linux-musl-v0.4.0/spacelens",
      b"binary",
      tar::EntryType::Regular,
    )]);
    let extracted = extract_agent(&payload).unwrap();
    assert_eq!(extracted.bytes, b"binary");
    assert!(extracted.archive_path.ends_with("/spacelens"));
  }

  #[test]
  fn accepts_a_windows_name_for_future_targets() {
    let payload = archive(&[("spacelens/spacelens.exe", b"pe", tar::EntryType::Regular)]);
    assert_eq!(extract_agent(&payload).unwrap().bytes, b"pe");
  }

  #[test]
  fn rejects_traversal_and_absolute_paths() {
    for path in ["../evil", "/etc/passwd", "a/../../b", "./spacelens"] {
      assert!(safe_relative(path).is_err(), "accepted {path:?}");
    }
  }

  #[test]
  fn rejects_links() {
    for kind in [tar::EntryType::Symlink, tar::EntryType::Link] {
      let payload = archive(&[("spacelens", b"x", kind)]);
      assert!(extract_agent(&payload).is_err());
    }
  }

  #[test]
  fn rejects_multiple_regular_files_and_missing_files() {
    let multiple = archive(&[
      ("a/spacelens", b"x", tar::EntryType::Regular),
      ("a/README", b"y", tar::EntryType::Regular),
    ]);
    assert!(extract_agent(&multiple).is_err());

    let directories_only = archive(&[("a", b"", tar::EntryType::Directory)]);
    assert!(extract_agent(&directories_only).is_err());
  }

  #[test]
  fn rejects_non_agent_file_names() {
    let payload = archive(&[("spacelens/README.md", b"x", tar::EntryType::Regular)]);
    assert!(extract_agent(&payload).is_err());
  }

  #[test]
  fn rejects_garbage() {
    assert!(extract_agent(b"not a tarball").is_err());
  }
}
