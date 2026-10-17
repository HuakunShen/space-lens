//! The closed error type shared by every memory operation (§4.1 of
//! docs/memory-cleaner-plan.md). Each variant maps onto an existing wire
//! problem code through [`MemoryError::problem_code`]; no new codes exist.

use thiserror::Error;

/// Every failure a memory operation can report. The set is closed, so a caller
/// can branch on the mapped problem code without inspecting message text.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum MemoryError {
  /// The current platform cannot provide the requested capability.
  #[error("This operation is not supported on this platform.")]
  Unsupported {
    /// The capability that is unavailable.
    what: &'static str,
  },

  /// The operating-system sampler could not read the memory state.
  #[error("The memory sampler failed: {detail}")]
  Sampler {
    /// A short, non-sensitive description of the failure.
    detail: String,
  },

  /// The requested termination plan does not exist.
  #[error("The termination plan was not found.")]
  PlanNotFound,

  /// The plan existed but is past its expiry.
  #[error("The termination plan has expired.")]
  PlanExpired,

  /// The plan was created by a different session.
  #[error("The termination plan belongs to a different session.")]
  PlanSessionMismatch,

  /// The caller did not pass the literal `confirm: true`.
  #[error("Terminating processes requires confirmation.")]
  NotConfirmed,

  /// The caller exceeded a plan or execution rate limit.
  #[error("Too many requests; retry after {retry_after_ms} milliseconds.")]
  RateLimited {
    /// How long the caller should wait before retrying.
    retry_after_ms: u64,
  },

  /// The service is already handling a conflicting call.
  #[error("The memory service is busy.")]
  Busy,

  /// The request was malformed.
  #[error("The request is invalid: {detail}")]
  InvalidRequest {
    /// A short description of the malformed input.
    detail: String,
  },

  /// An internal invariant was violated.
  #[error("An internal error occurred: {detail}")]
  Internal {
    /// A short description of the internal failure.
    detail: String,
  },
}

impl MemoryError {
  /// The wire problem code this error maps to (exhaustive, no catch-all).
  pub fn problem_code(&self) -> &'static str {
    match self {
      MemoryError::Unsupported { .. } => "UnsupportedOperation",
      MemoryError::Sampler { .. } => "Unavailable",
      MemoryError::PlanNotFound => "NotFound",
      MemoryError::PlanExpired => "StalePlan",
      MemoryError::PlanSessionMismatch => "Forbidden",
      MemoryError::NotConfirmed => "InvalidRequest",
      MemoryError::RateLimited { .. } | MemoryError::Busy => "LimitExceeded",
      MemoryError::InvalidRequest { .. } => "InvalidRequest",
      MemoryError::Internal { .. } => "InternalError",
    }
  }

  /// Whether the caller may retry the same call unchanged.
  pub fn retryable(&self) -> bool {
    matches!(
      self,
      MemoryError::Sampler { .. } | MemoryError::RateLimited { .. } | MemoryError::Busy
    )
  }
}
