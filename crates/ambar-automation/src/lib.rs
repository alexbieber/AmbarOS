//! # ambar-automation
//!
//! The local automation engine for Ambar OS.
//!
//! A [`Workflow`] combines a [`Trigger`] (manual, schedule or folder watch),
//! optional [`Condition`]s, and a list of [`Action`]s. The [`Engine`] validates
//! workflows, enforces the user's permission [`Grant`]s on every operation,
//! runs actions, records history in SQLite, and supports pause, resume and
//! cancellation.
//!
//! Security properties:
//! - No action can run a shell command or arbitrary program.
//! - Nothing runs without a stored, user-approved grant.
//! - Every path is resolved (symlinks followed) and checked at run time.
//! - Destructive operations need explicit approval; "delete" moves to a
//!   recoverable trash rather than unlinking.

pub mod actions;
pub mod engine;
pub mod error;
pub mod history;
pub mod notify;
pub mod permissions;
pub mod template;
mod triggers;
pub mod workflow;

pub use engine::{
    ApprovalRequest, Approver, DenyAll, Engine, EngineConfig, EngineEvent, ShutdownReport,
    WorkflowState,
};
pub use error::{EngineError, Result, ValidationError};
pub use history::{ExecutionRecord, ExecutionStatus, StepRecord, StepStatus, Store};
pub use notify::{DesktopNotifier, LogNotifier, MemoryNotifier, Notification, Notifier};
pub use permissions::{Access, Grant, PermissionError, PermissionRequest};
pub use workflow::{Action, Condition, FileEventKind, Trigger, ValidationOptions, Workflow};
