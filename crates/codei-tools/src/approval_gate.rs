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
    always_approve: bool,
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

    /// Approve the current request and auto-approve future destructive tool calls.
    pub async fn approve_always(&self) -> bool {
        let respond = {
            let mut guard = self.inner.lock().await;
            guard.always_approve = true;
            guard.pending.take().map(|p| p.respond)
        };
        if let Some(tx) = respond {
            tx.send(true).is_ok()
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

        {
            let guard = self.gate.inner.lock().await;
            if guard.always_approve {
                return ApprovalResponse { approved: true };
            }
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn approve_always_skips_future_prompts() {
        let gate = Arc::new(SharedApprovalGate::new());
        let handler = gate.handler();

        let first = tokio::spawn({
            let handler = gate.handler();
            async move {
                handler
                    .approve(ApprovalRequest {
                        tool_name: "shell".into(),
                        arguments: json!({"command": "ls"}),
                    })
                    .await
            }
        });

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        assert!(gate.take_pending().await.is_some());
        assert!(gate.approve_always().await);

        let first = first.await.unwrap();
        assert!(first.approved);

        let second = handler
            .approve(ApprovalRequest {
                tool_name: "write".into(),
                arguments: json!({"path": "a.txt"}),
            })
            .await;
        assert!(second.approved);
        assert!(gate.take_pending().await.is_none());
    }
}
