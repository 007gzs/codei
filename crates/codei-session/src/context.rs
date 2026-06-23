use codei_config::AgentConfig;
use codei_llm::Message;

use crate::compact::{compact_messages, estimate_tokens, TokenBudget};
use crate::model::{to_llm_messages, Session};

/// Builds the message list sent to the LLM from a session.
pub struct ContextBuilder;

impl ContextBuilder {
    pub fn build(session: &Session, system_prompt: &str) -> Vec<Message> {
        Self::build_with_config(session, system_prompt, None)
    }

    pub fn build_with_config(
        session: &Session,
        system_prompt: &str,
        agent: Option<&AgentConfig>,
    ) -> Vec<Message> {
        let messages = to_llm_messages(session, system_prompt);
        if let Some(cfg) = agent {
            let budget = TokenBudget {
                max_tokens: cfg.context_window_tokens,
                compaction_threshold: cfg.compaction_threshold,
            };
            compact_messages(messages, budget)
        } else {
            messages
        }
    }

    pub fn estimate_session_tokens(session: &Session, system_prompt: &str) -> u32 {
        estimate_tokens(&to_llm_messages(session, system_prompt))
    }
}
