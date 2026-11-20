//! Headless remote-host support for the Space Lens desktop.
//!
//! The desktop connects to a machine over SSH, installs a pinned agent and host
//! package under the remote user's home, starts `spacelens serve` on the remote
//! loopback, and reaches it through an SSH local forward. This crate owns the
//! parts that can be tested without a Tauri shell or a live host: target
//! validation, quoting, OpenSSH config listing, probe parsing, the agent
//! manifest and archive rules, install stages, and the exact `ssh` argv.
//!
//! Nothing here spawns a process, reads the network, or touches a remote
//! filesystem. The desktop runtime injects those effects.

pub mod agent;
pub mod archive;
pub mod error;
pub mod install;
pub mod manifest;
pub mod probe;
pub mod quote;
pub mod server;
pub mod ssh_config;
pub mod target;

pub use error::RemoteError;
pub use install::{AgentLayout, HostLayout, InstallProgress, InstallStage};
pub use target::SshTarget;
