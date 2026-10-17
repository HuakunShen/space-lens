//! Process and memory sampling, grouping and safe termination planning for
//! Space Lens (§3 and §4 of docs/memory-cleaner-plan.md).
//!
//! The crate is pure computation plus a thin platform layer: every wire type
//! mirrors the Zod contract in `packages/contract/src/memory.ts`, and all
//! operating-system access sits behind the seams in [`sampler`] so the domain
//! logic can be tested without a real machine.

#![deny(unsafe_code)]

pub mod candidates;
pub mod error;
pub mod fake;
pub mod group;
pub mod history;
pub mod ids;
pub mod model;
pub mod plan;
pub mod raw;
pub mod safety;
pub mod sampler;

pub use candidates::candidates;
pub use error::MemoryError;
pub use fake::{FakeClock, FakeSignaller, FakeSleeper, Identity};
pub use group::{build_groups, bundle_root, classify_kind, group_id_for, is_main_executable};
pub use history::History;
pub use ids::{AuditId, GroupId, PlanId, ProcessKey};
pub use model::*;
pub use plan::{AuditRing, PlanStore, SnapshotIndex};
pub use raw::{AppInfo, QuitAck, RawProcess, RawSample, RawSystem, Signal, VmPages};
pub use safety::{apply, classify, context, SafetyContext};
