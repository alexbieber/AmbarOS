//! Live approvals for sensitive actions.
//!
//! When a workflow running in the background reaches a sensitive action
//! (moving to trash, overwriting a file) and its grant does not pre-approve
//! it, the run pauses and the request appears in the Automation Center and in
//! `ambar approval list`. The owner allows or denies it; no answer within the
//! timeout means **deny**. Only the owner can decide — never an agent.

use std::collections::HashMap;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ambar_automation::{ApprovalRequest, Approver, Notification, Notifier, Store};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A request waiting for the owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingApproval {
    /// Approval id (`appr-<8 hex>`).
    pub id: String,
    /// Workflow asking.
    pub workflow_id: String,
    /// Execution waiting on it.
    pub execution_id: String,
    /// Action kind, e.g. `trash_file`.
    pub action: String,
    /// Plain-language description, e.g. "move /home/a/x.log to the Ambar trash".
    pub description: String,
    /// When it was asked (RFC 3339).
    pub requested_at: String,
    /// When it is automatically denied (RFC 3339).
    pub expires_at: String,
}

#[derive(Debug)]
struct Slot {
    info: PendingApproval,
    reply: Sender<bool>,
}

/// Holds pending approvals and lets the owner decide them.
#[derive(Debug)]
pub struct ApprovalBroker {
    pending: Mutex<HashMap<String, Slot>>,
    timeout: Duration,
    notifier: Arc<dyn Notifier>,
    store: Store,
    closed: Mutex<bool>,
}

fn new_id() -> String {
    let mut b = [0u8; 4];
    let _ = getrandom::fill(&mut b);
    format!("appr-{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>())
}

impl ApprovalBroker {
    /// A broker that waits up to `timeout` for each decision.
    pub fn new(timeout: Duration, notifier: Arc<dyn Notifier>, store: Store) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            timeout,
            notifier,
            store,
            closed: Mutex::new(false),
        }
    }

    /// Requests waiting right now, oldest first.
    pub fn list(&self) -> Vec<PendingApproval> {
        let mut v: Vec<PendingApproval> = self
            .pending
            .lock()
            .map(|p| p.values().map(|s| s.info.clone()).collect())
            .unwrap_or_default();
        v.sort_by(|a, b| a.requested_at.cmp(&b.requested_at));
        v
    }

    /// Records the owner's decision. Returns the request, or `None` if it
    /// already expired or was decided.
    pub fn decide(&self, id: &str, allow: bool) -> Option<PendingApproval> {
        let slot = self.pending.lock().ok()?.remove(id)?;
        let _ = slot.reply.send(allow);
        Some(slot.info)
    }

    /// Denies everything pending and refuses new requests (used at shutdown).
    pub fn close(&self) {
        if let Ok(mut c) = self.closed.lock() {
            *c = true;
        }
        if let Ok(mut p) = self.pending.lock() {
            for (_, slot) in p.drain() {
                let _ = slot.reply.send(false);
            }
        }
    }
}

impl Approver for ApprovalBroker {
    fn approve(&self, r: &ApprovalRequest) -> bool {
        if self.closed.lock().map(|c| *c).unwrap_or(true) {
            return false;
        }
        let now = Utc::now();
        let expires: DateTime<Utc> =
            now + chrono::Duration::from_std(self.timeout).unwrap_or(chrono::Duration::zero());
        let info = PendingApproval {
            id: new_id(),
            workflow_id: r.workflow_id.clone(),
            execution_id: r.execution_id.clone(),
            action: r.action.clone(),
            description: r.description.clone(),
            requested_at: now.to_rfc3339(),
            expires_at: expires.to_rfc3339(),
        };
        let (tx, rx) = channel();
        match self.pending.lock() {
            Ok(mut p) => {
                p.insert(info.id.clone(), Slot { info: info.clone(), reply: tx });
            }
            Err(_) => return false,
        }
        let _ = self.store.audit(
            &info.workflow_id,
            "approval.requested",
            &format!("approval={} action={} {}", info.id, info.action, info.description),
        );
        let _ = self.notifier.notify(&Notification {
            workflow_id: info.workflow_id.clone(),
            title: "Ambar needs your approval".into(),
            body: format!("{} wants to {}", info.workflow_id, info.description),
        });
        let decision = rx.recv_timeout(self.timeout);
        if let Ok(mut p) = self.pending.lock() {
            p.remove(&info.id);
        }
        let (allowed, how) = match decision {
            Ok(true) => (true, "allowed"),
            Ok(false) => (false, "denied"),
            Err(_) => (false, "expired"),
        };
        let _ = self.store.audit(
            &info.workflow_id,
            &format!("approval.{how}"),
            &format!("approval={} action={}", info.id, info.action),
        );
        allowed
    }
}
