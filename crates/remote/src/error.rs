//! Closed error type for remote-host work.
//!
//! Every failure the UI shows is one of these codes; the frontend never parses
//! prose. `retryable` says whether repeating the same action can plausibly
//! succeed (a dropped link) or cannot (an unsupported architecture).

use crate::install::InstallStage;

#[derive(Debug, thiserror::Error)]
pub enum RemoteError {
  #[error("invalid SSH target: {0}")]
  InvalidTarget(String),
  #[error("unsupported target: {0}")]
  UnsupportedTarget(String),
  #[error("invalid remote path: {0}")]
  InvalidPath(String),
  #[error("unsupported shell: {0}")]
  UnsupportedShell(String),
  #[error("probe failed: {0}")]
  Probe(String),
  #[error("archive rejected: {0}")]
  Archive(String),
  #[error("manifest rejected: {0}")]
  Manifest(String),
  #[error("agent install failed at {stage}: {message}")]
  Stage {
    stage: InstallStage,
    message: String,
    retryable: bool,
  },
  #[error("host is not usable: {0}")]
  HostUnavailable(String),
  #[error("io error: {0}")]
  Io(#[from] std::io::Error),
  #[error("json error: {0}")]
  Json(#[from] serde_json::Error),
}

impl RemoteError {
  pub fn code(&self) -> &'static str {
    match self {
      Self::InvalidTarget(_) => "invalid-target",
      Self::UnsupportedTarget(_) => "unsupported-target",
      Self::InvalidPath(_) => "invalid-path",
      Self::UnsupportedShell(_) => "unsupported-shell",
      Self::Probe(_) => "probe-failed",
      Self::Archive(_) => "archive-rejected",
      Self::Manifest(_) => "manifest-rejected",
      Self::Stage { .. } => "install-failed",
      Self::HostUnavailable(_) => "host-unavailable",
      Self::Io(_) => "io-error",
      Self::Json(_) => "protocol-error",
    }
  }

  pub fn retryable(&self) -> bool {
    match self {
      Self::Stage { retryable, .. } => *retryable,
      Self::HostUnavailable(_) | Self::Io(_) | Self::Probe(_) => true,
      Self::InvalidTarget(_)
      | Self::UnsupportedTarget(_)
      | Self::InvalidPath(_)
      | Self::UnsupportedShell(_)
      | Self::Archive(_)
      | Self::Manifest(_)
      | Self::Json(_) => false,
    }
  }

  pub fn stage(stage: InstallStage, message: impl Into<String>) -> Self {
    Self::Stage {
      stage,
      message: message.into(),
      retryable: stage.retryable(),
    }
  }

  pub fn stage_retryable(stage: InstallStage, message: impl Into<String>, retryable: bool) -> Self {
    Self::Stage {
      stage,
      message: message.into(),
      retryable,
    }
  }

  pub fn problem(&self) -> serde_json::Value {
    serde_json::json!({
      "code": self.code(),
      "message": self.to_string(),
      "retryable": self.retryable(),
      "stage": match self {
        Self::Stage { stage, .. } => Some(stage.code()),
        _ => None,
      },
    })
  }
}

/// One-line, code-prefixed rendering for CLI shells, which print errors
/// instead of mapping them onto a UI problem frame.
pub fn problem_text(error: &RemoteError) -> String {
  format!("{}: {error}", error.code())
}

/// The same rendering for a problem JSON the host (or this crate) produced.
/// Accepts both the bare problem object and its `{"problem": …}` envelope.
pub fn problem_text_value(problem: &serde_json::Value) -> String {
  let inner = problem.get("problem").unwrap_or(problem);
  let code = inner
    .get("code")
    .and_then(serde_json::Value::as_str)
    .unwrap_or("error");
  let message = inner
    .get("message")
    .and_then(serde_json::Value::as_str)
    .unwrap_or("");
  if message.is_empty() {
    code.to_string()
  } else {
    format!("{code}: {message}")
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn every_error_has_a_stable_code() {
    let errors = [
      RemoteError::InvalidTarget("x".into()),
      RemoteError::UnsupportedTarget("x".into()),
      RemoteError::InvalidPath("x".into()),
      RemoteError::UnsupportedShell("fish".into()),
      RemoteError::Probe("x".into()),
      RemoteError::Archive("x".into()),
      RemoteError::Manifest("x".into()),
      RemoteError::HostUnavailable("x".into()),
      RemoteError::stage(InstallStage::AgentInstall, "x"),
    ];
    for error in errors {
      assert!(!error.code().is_empty());
      assert_eq!(error.problem()["code"], error.code());
    }
    assert!(!RemoteError::stage(InstallStage::AgentInstall, "x").retryable());
    assert!(RemoteError::HostUnavailable("x".into()).retryable());
  }

  #[test]
  fn problem_text_names_the_code_for_shells() {
    let error = RemoteError::UnsupportedShell("fish".into());
    assert!(problem_text(&error).starts_with("unsupported-shell: "));
    assert!(
      problem_text(&error).contains("fish"),
      "problem_text dropped the message"
    );

    let host_problem = serde_json::json!({
      "problem": { "code": "Forbidden", "message": "cleanup is disabled", "retryable": false }
    });
    assert_eq!(
      problem_text_value(&host_problem),
      "Forbidden: cleanup is disabled"
    );
    assert_eq!(
      problem_text_value(&serde_json::json!({ "code": "NotFound", "message": "" })),
      "NotFound"
    );
  }
}
