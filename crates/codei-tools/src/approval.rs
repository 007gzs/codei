use std::io::{self, Write};

use async_trait::async_trait;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    pub tool_name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone)]
pub struct ApprovalResponse {
    pub approved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalPolicy {
    Always,
    Never,
    OnDestructive,
}

#[async_trait]
pub trait ApprovalHandler: Send + Sync {
    async fn approve(&self, request: ApprovalRequest) -> ApprovalResponse;
}

pub struct AutoApprove;

#[async_trait]
impl ApprovalHandler for AutoApprove {
    async fn approve(&self, _request: ApprovalRequest) -> ApprovalResponse {
        ApprovalResponse { approved: true }
    }
}

pub struct PromptApprove;

#[async_trait]
impl ApprovalHandler for PromptApprove {
    async fn approve(&self, request: ApprovalRequest) -> ApprovalResponse {
        eprintln!(
            "\n[approval] tool={} args={}",
            request.tool_name, request.arguments
        );
        eprint!("Approve? [y/N]: ");
        let _ = io::stderr().flush();
        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_err() {
            return ApprovalResponse { approved: false };
        }
        let approved = line.trim().eq_ignore_ascii_case("y");
        ApprovalResponse { approved }
    }
}

/// Auto-approve safe tools; prompt for destructive ones.
pub struct OnDestructiveApprove {
    inner: PromptApprove,
}

#[async_trait]
impl ApprovalHandler for OnDestructiveApprove {
    async fn approve(&self, request: ApprovalRequest) -> ApprovalResponse {
        match request.tool_name.as_str() {
            "write" | "edit" | "shell" => self.inner.approve(request).await,
            _ => ApprovalResponse { approved: true },
        }
    }
}

pub fn handler_for_policy(policy: ApprovalPolicy) -> Box<dyn ApprovalHandler> {
    match policy {
        ApprovalPolicy::Never => Box::new(AutoApprove),
        ApprovalPolicy::Always => Box::new(PromptApprove),
        ApprovalPolicy::OnDestructive => Box::new(OnDestructiveApprove {
            inner: PromptApprove,
        }),
    }
}
