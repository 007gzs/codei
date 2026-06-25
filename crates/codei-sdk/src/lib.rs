//! Programmatic SDK for CodeI.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use codei_agent::{AgentEvent, AgentLoop, TurnOutcome};
use codei_config::{load, LoadOptions, ResolvedConfig};
use codei_llm::create_provider;
use codei_mcp::McpManager;
use codei_session::{Session, SessionStore};
use codei_tools::{handler_for_policy, ApprovalPolicy, ToolContext};
use tokio::sync::mpsc;

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
        })
        .map_err(SdkError::Config)?;

        let config = Arc::new(resolved);
        let provider_name = self
            .provider
            .unwrap_or_else(|| config.config.defaults.provider.clone());
        let provider = create_provider(&config).map_err(SdkError::Llm)?;
        let model = Arc::new(RwLock::new(
            self.model
                .unwrap_or_else(|| config.config.defaults.model.clone()),
        ));

        Ok(CodeiClient {
            config,
            provider,
            provider_name,
            model,
            auto_approve: self.auto_approve,
        })
    }
}

/// Programmatic entry point for running CodeI agents.
pub struct CodeiClient {
    config: Arc<ResolvedConfig>,
    provider: Arc<dyn codei_llm::LlmProvider>,
    provider_name: String,
    model: Arc<RwLock<String>>,
    auto_approve: bool,
}

impl CodeiClient {
    pub fn builder() -> CodeiClientBuilder {
        CodeiClientBuilder::new()
    }

    /// Run a prompt and invoke `on_event` for each agent event.
    pub async fn run_with_handler<F>(
        &self,
        prompt: &str,
        mut on_event: F,
    ) -> Result<RunResult, SdkError>
    where
        F: FnMut(AgentEvent),
    {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let policy = if self.auto_approve {
            ApprovalPolicy::Never
        } else {
            ApprovalPolicy::OnDestructive
        };
        let tool_ctx = ToolContext {
            cwd: self.config.cwd.clone(),
            config: Arc::clone(&self.config),
            approval: Arc::from(handler_for_policy(policy)),
        };
        let mcp = McpManager::connect_optional().await;
        let agent = AgentLoop::new(
            Arc::clone(&self.config),
            Arc::clone(&self.model),
            Arc::clone(&self.provider),
            self.provider_name.clone(),
            tool_ctx,
            mcp,
            Some(tx),
        );

        let mut session = Session::new(self.config.cwd.clone());
        let store = SessionStore::open_for_config(&self.config.config.session)
            .map_err(SdkError::Session)?;
        let prompt = prompt.to_string();

        let session_id = session.id.clone();
        let agent_task = async {
            agent
                .run_turn(&mut session, &prompt, &store)
                .await
                .map_err(SdkError::Agent)
        };
        tokio::pin!(agent_task);

        let mut outcome = TurnOutcome::default();
        loop {
            tokio::select! {
                event = rx.recv() => {
                    match event {
                        Some(AgentEvent::TurnComplete { usage }) => {
                            outcome.usage = usage;
                            on_event(AgentEvent::TurnComplete { usage: outcome.usage });
                            break;
                        }
                        Some(other) => on_event(other),
                        None => break,
                    }
                }
                result = &mut agent_task => {
                    outcome = result?;
                    break;
                }
            }
        }

        Ok(RunResult {
            session_id,
            outcome,
        })
    }

    /// Convenience wrapper that collects events only for completion.
    pub async fn run(&self, prompt: &str) -> Result<RunResult, SdkError> {
        self.run_with_handler(prompt, |_| {}).await
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SdkError {
    #[error("config error: {0}")]
    Config(#[from] codei_config::ConfigError),
    #[error("llm error: {0}")]
    Llm(#[from] codei_llm::LlmError),
    #[error("session error: {0}")]
    Session(#[from] codei_session::SessionError),
    #[error("agent error: {0}")]
    Agent(#[from] codei_agent::AgentError),
}
