//! High-level one-shot client built on shared runtime helpers.

use std::path::PathBuf;
use std::sync::Arc;

use codei_agent::{AgentEvent, TurnOutcome};
use codei_config::{load, LoadOptions, ResolvedConfig};
use codei_session::{Session, SessionStore};

use crate::error::SdkError;
use crate::runtime::build_interactive_launch;
use crate::turn::{approval_policy, run_turn_with_events};

/// Result of a single agent run.
#[derive(Debug, Clone)]
pub struct RunResult {
    pub session_id: String,
    pub outcome: TurnOutcome,
}

/// Builder for [`CodeiClient`].
pub struct CodeiClientBuilder {
    cwd: Option<PathBuf>,
    model: Option<String>,
    provider: Option<String>,
    auto_approve: bool,
}

impl Default for CodeiClientBuilder {
    fn default() -> Self {
        Self {
            cwd: None,
            model: None,
            provider: None,
            auto_approve: true,
        }
    }
}

impl CodeiClientBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    pub fn auto_approve(mut self, yes: bool) -> Self {
        self.auto_approve = yes;
        self
    }

    pub async fn build(self) -> Result<CodeiClient, SdkError> {
        let resolved = load(&LoadOptions {
            cwd: self.cwd,
            model: self.model.clone(),
            provider: self.provider.clone(),
            language: None,
        })?;

        let config = Arc::new(resolved);
        let store = Arc::new(SessionStore::open_for_config(&config.config.session)?);
        let session = Session::new(config.cwd.clone());
        let launch = build_interactive_launch(config, session, store).await?;

        Ok(CodeiClient {
            launch,
            auto_approve: self.auto_approve,
        })
    }
}

/// Programmatic entry point for running CodeI agents.
pub struct CodeiClient {
    launch: crate::runtime::InteractiveLaunch,
    auto_approve: bool,
}

impl CodeiClient {
    pub fn builder() -> CodeiClientBuilder {
        CodeiClientBuilder::new()
    }

    pub fn config(&self) -> &Arc<ResolvedConfig> {
        &self.launch.config
    }

    /// Run a prompt and invoke `on_event` for each agent event.
    pub async fn run_with_handler<F>(
        &mut self,
        prompt: &str,
        on_event: F,
    ) -> Result<RunResult, SdkError>
    where
        F: FnMut(AgentEvent),
    {
        let policy = approval_policy(self.auto_approve);
        let session_id = self.launch.session.id.clone();
        let runtime = self.launch.runtime();

        let outcome =
            run_turn_with_events(&runtime, &mut self.launch.session, prompt, policy, on_event)
                .await?;

        Ok(RunResult {
            session_id,
            outcome,
        })
    }

    /// Convenience wrapper that collects events only for completion.
    pub async fn run(&mut self, prompt: &str) -> Result<RunResult, SdkError> {
        self.run_with_handler(prompt, |_| {}).await
    }
}
