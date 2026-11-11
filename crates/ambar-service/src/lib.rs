//! # ambar-service
//!
//! `ambard`, the Ambar OS background automation service, plus its local IPC
//! protocol and client. The automation engine itself lives in
//! `ambar-automation` and stays platform-independent; this crate adds the
//! Unix-specific pieces (socket, peer credentials, launchd/systemd units).
//!
//! See `docs/service.md` for the design.

#![cfg(unix)]

pub mod agents;
pub mod approvals;
pub mod auth;
pub mod client;
pub mod paths;
pub mod protocol;
pub mod server;
pub mod units;

pub use client::{Client, ClientError};
pub use paths::{PathError, ServicePaths};
pub use server::{DaemonConfig, DaemonError, RunningDaemon, start};
