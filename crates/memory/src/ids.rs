//! Validated identifier newtypes used by the memory wire types (§4.1 of
//! docs/memory-cleaner-plan.md). Each newtype wraps a `String` that can only be
//! built from a value matching its documented pattern; serde reuses the same
//! check (`try_from`/`into`), so a malformed identifier can never cross the
//! boundary.

use crate::error::MemoryError;
use serde::{Deserialize, Serialize};

/// Number of characters after the prefix of every memory identifier.
const ID_BODY_LEN: usize = 16;

/// The body of `value` after `prefix`, or `None` when the prefix is missing or
/// the body is not exactly [`ID_BODY_LEN`] ASCII characters.
fn body<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
  value
    .strip_prefix(prefix)
    .filter(|body| body.len() == ID_BODY_LEN && body.is_ascii())
}

fn is_lower_hex(body: &str) -> bool {
  body
    .bytes()
    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_alphanumeric(body: &str) -> bool {
  body.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn reject(kind: &str) -> MemoryError {
  MemoryError::InvalidRequest {
    detail: format!("{kind} does not match its expected identifier pattern."),
  }
}

/// A process group identifier: `grp_` followed by the 16 lowercase hex digits
/// of the FNV-1a 64 hash of the group key.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct GroupId(String);

impl GroupId {
  /// The identifier as a string slice.
  pub fn as_str(&self) -> &str {
    &self.0
  }
}

impl TryFrom<String> for GroupId {
  type Error = MemoryError;

  fn try_from(value: String) -> Result<Self, Self::Error> {
    match body(&value, "grp_") {
      Some(candidate) if is_lower_hex(candidate) => Ok(GroupId(value)),
      _ => Err(reject("GroupId")),
    }
  }
}

impl From<GroupId> for String {
  fn from(value: GroupId) -> String {
    value.0
  }
}

/// A termination plan identifier: `tplan_` followed by 16 alphanumeric
/// characters.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PlanId(String);

impl PlanId {
  /// The identifier as a string slice.
  pub fn as_str(&self) -> &str {
    &self.0
  }
}

impl TryFrom<String> for PlanId {
  type Error = MemoryError;

  fn try_from(value: String) -> Result<Self, Self::Error> {
    match body(&value, "tplan_") {
      Some(candidate) if is_alphanumeric(candidate) => Ok(PlanId(value)),
      _ => Err(reject("PlanId")),
    }
  }
}

impl From<PlanId> for String {
  fn from(value: PlanId) -> String {
    value.0
  }
}

/// A termination audit identifier: `aud_` followed by 16 alphanumeric
/// characters.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AuditId(String);

impl AuditId {
  /// The identifier as a string slice.
  pub fn as_str(&self) -> &str {
    &self.0
  }
}

impl TryFrom<String> for AuditId {
  type Error = MemoryError;

  fn try_from(value: String) -> Result<Self, Self::Error> {
    match body(&value, "aud_") {
      Some(candidate) if is_alphanumeric(candidate) => Ok(AuditId(value)),
      _ => Err(reject("AuditId")),
    }
  }
}

impl From<AuditId> for String {
  fn from(value: AuditId) -> String {
    value.0
  }
}

/// The identity of a process across pid reuse: the pid plus its start time in
/// microseconds since the Unix epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessKey {
  /// The operating-system process id.
  pub pid: u32,
  /// The process start time in microseconds since the Unix epoch.
  pub start_time_us: u64,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn valid_ids_parse_and_round_trip() {
    let group = GroupId::try_from("grp_0123456789abcdef".to_string()).expect("valid group id");
    assert_eq!(group.as_str(), "grp_0123456789abcdef");
    assert_eq!(String::from(group.clone()), "grp_0123456789abcdef");

    let plan = PlanId::try_from("tplan_0123456789ABCDEF".to_string()).expect("valid plan id");
    assert_eq!(String::from(plan), "tplan_0123456789ABCDEF");

    let audit = AuditId::try_from("aud_0123456789abcdef".to_string()).expect("valid audit id");
    assert_eq!(audit.as_str(), "aud_0123456789abcdef");
  }

  #[test]
  fn group_id_rejects_values_outside_the_pattern() {
    for bad in [
      "grp_0123456789ABCDEF",  // uppercase hex is not allowed
      "grp_0123456789abcde",   // too short
      "grp_0123456789abcdef0", // too long
      "GRP_0123456789abcdef",  // wrong prefix
      "grp_0123456789abcdeg",  // not hex
      "grp_éééééééééééééééé",  // non-ASCII body of the right length
    ] {
      assert!(
        GroupId::try_from(bad.to_string()).is_err(),
        "{bad} should be rejected"
      );
    }
  }

  #[test]
  fn plan_and_audit_ids_reject_values_outside_the_pattern() {
    for bad in [
      "tplan_0123456789abcde",
      "tplan_0123456789abcde-",
      "xplan_0123456789abcdef",
    ] {
      assert!(
        PlanId::try_from(bad.to_string()).is_err(),
        "{bad} should be rejected"
      );
    }
    for bad in [
      "aud_0123456789abcde",
      "aud_0123456789abcde!",
      "adc_0123456789abcdef",
    ] {
      assert!(
        AuditId::try_from(bad.to_string()).is_err(),
        "{bad} should be rejected"
      );
    }
  }

  #[test]
  fn ids_reject_bad_values_through_serde() {
    assert!(serde_json::from_str::<GroupId>("\"grp_0123456789ABCDEF\"").is_err());
    assert!(serde_json::from_str::<GroupId>("\"grp_0123456789abcdef\"").is_ok());
    assert_eq!(
      serde_json::to_string(&GroupId::try_from("grp_0123456789abcdef".to_string()).expect("valid"))
        .expect("serialize"),
      "\"grp_0123456789abcdef\""
    );
  }

  #[test]
  fn process_key_serializes_camel_case() {
    let key = ProcessKey {
      pid: 4321,
      start_time_us: 1_729_180_800_000_000,
    };
    assert_eq!(
      serde_json::to_value(key).expect("serialize"),
      serde_json::json!({ "pid": 4321, "startTimeUs": 1_729_180_800_000_000u64 })
    );
  }
}
