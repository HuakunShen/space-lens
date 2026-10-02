//! Session layer: pure logic over scan results, shared by every view —
//! no GPUI types here. Unit tests live beside each module
//! (`cargo test -p spacelens-gpui`).

pub mod format;
pub mod index;
pub mod plan;
pub mod sunburst;
