//! Process and memory sampling, grouping and safe termination planning for
//! Space Lens (§3 and §4 of docs/memory-cleaner-plan.md).
//!
//! The crate is pure computation plus a thin platform layer: every wire type
//! mirrors the Zod contract in `packages/contract/src/memory.ts`, and all
//! operating-system access sits behind the seams in [`sampler`] so the domain
//! logic can be tested without a real machine.

#![deny(unsafe_code)]

pub mod error;
pub mod ids;
pub mod model;
pub mod raw;
pub mod sampler;

pub use error::MemoryError;
pub use ids::{AuditId, GroupId, PlanId, ProcessKey};
pub use model::*;
pub use raw::{AppInfo, QuitAck, RawProcess, RawSample, RawSystem, Signal, VmPages};
