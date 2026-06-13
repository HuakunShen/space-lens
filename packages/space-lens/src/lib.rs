#![deny(clippy::all)]

pub mod clean;
pub mod scanner;

pub use clean::{
  build_removal_plan, execute_removal_plan, find_candidates, CandidateOptions, CleanupCandidate,
  CleanupPreset, RemovalEntry, RemovalOutcome, RemovalPlan,
};
pub use scanner::{measure_path, scan_directory, IgnoredMode, ScanNode, ScanOptions};
