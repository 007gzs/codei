use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{oneshot, Mutex};

use crate::{ApprovalHandler, ApprovalRequest, ApprovalResponse};

/// Pending approval surfaced to the UI layer.
pub struct SharedApprovalGate {
    inner: Arc<Mutex<GateState>>,
}

#[derive(Default)]
struct GateState {
    pending: Option<PendingApproval>,
}

struct PendingApproval {
    request: ApprovalRequest,
    respond: oneshot::Sender<bool>,
}

impl Default for SharedApprovalGate {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedApprovalGate {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(GateState::default())),
        }
    }

    pub fn handler(self: &Arc<Self>) -> GateApprovalHandler {
        GateApprovalHandler {
            gate: Arc::clone(self),
        }
    }

    pub async fn take_pending(&self) -> Option<ApprovalRequest> {
        let guard = self.inner.lock().await;
        guard.pending.as_ref().map(|p| p.request.clone())
    }

    pub async fn respond(&self, approved: bool) -> bool {
        let respond = {
            let mut guard = self.inner.lock().await;
            guard.pending.take().map(|p| p.respond)
        };
        if let Some(tx) = respond {
            tx.send(approved).is_ok()
        } else {
            false
        }
    }
}

pub struct GateApprovalHandler {
    gate: Arc<SharedApprovalGate>,
}

#[async_trait]
impl ApprovalHandler for GateApprovalHandler {
    async fn approve(&self, request: ApprovalRequest) -> ApprovalResponse {
        match request.tool_name.as_str() {
            "write" | "edit" | "shell" => {}
            _ => return ApprovalResponse { approved: true },
        }

        let (tx, rx) = oneshot::channel();
        {
            let mut guard = self.gate.inner.lock().await;
            guard.pending = Some(PendingApproval {
                request: request.clone(),
                respond: tx,
            });
        }

        let approved = rx.await.unwrap_or(false);
        ApprovalResponse { approved }
    }
}
