use codei_llm::{collect_response, ChatRequest, LlmProvider, Message};
use codei_session::{
    cap_output_tokens, format_transcript, should_compact_session, Session, SessionStore,
};
use tracing::warn;

use crate::error::AgentError;
use crate::loop_::AgentLoop;

const SUMMARIZE_SYSTEM: &str = r#"You compress conversation history for a coding assistant context window.

Produce a dense summary that preserves:
- User goals, requirements, and constraints
- Key decisions, conclusions, and agreed approaches
- Important file paths, symbols, commands, and code changes
- Unresolved tasks, errors, and open questions

Do not invent facts. Omit small talk. Use the same language as the conversation."#;

impl AgentLoop {
    /// Summarize older session messages with the LLM and keep recent turns intact.
    pub async fn compact_session(
        &self,
        session: &mut Session,
        store: &SessionStore,
    ) -> Result<(), AgentError> {
        let agent_cfg = &self.config().config.agent;
        let keep = agent_cfg.compaction_keep_messages.max(2) as usize;
        if session.messages.len() <= keep {
            return Ok(());
        }

        let split = session.messages.len() - keep;
        let transcript = format_transcript(&session.messages[..split]);
        if transcript.trim().is_empty() {
            session.compact(keep);
            store.save(session)?;
            return Ok(());
        }

        let model = self.model().read().expect("model lock poisoned").clone();
        let provider = self
            .provider()
            .read()
            .expect("provider lock poisoned")
            .clone();
        let language = &self.config().config.defaults.language;
        let max_tokens = cap_output_tokens(
            &[
                Message::system(SUMMARIZE_SYSTEM),
                Message::user(format!(
                "Summarize the conversation below. Write the summary in {language}.\n\n{transcript}"
            )),
            ],
            None,
            agent_cfg
                .compaction_summary_max_tokens
                .min(self.config().config.defaults.max_tokens),
            agent_cfg.context_window_tokens,
        );

        let summary = match summarize_transcript(
            provider.as_ref(),
            &model,
            &transcript,
            language,
            max_tokens,
        )
        .await
        {
            Ok(summary) => summary,
            Err(err) => {
                warn!(error = %err, "LLM compaction failed; falling back to truncation");
                session.compact(keep);
                store.save(session)?;
                return Ok(());
            }
        };

        session.compact_with_summary(keep, summary);
        store.save(session)?;
        Ok(())
    }

    pub(crate) async fn compact_session_if_needed(
        &self,
        session: &mut Session,
        store: &SessionStore,
    ) -> Result<bool, AgentError> {
        if !should_compact_session(session, self.system_prompt(), &self.config().config.agent) {
            return Ok(false);
        }
        self.compact_session(session, store).await?;
        Ok(true)
    }
}

async fn summarize_transcript(
    provider: &dyn LlmProvider,
    model: &str,
    transcript: &str,
    language: &str,
    max_tokens: u32,
) -> Result<String, codei_llm::LlmError> {
    let request = ChatRequest {
        model: model.to_string(),
        messages: vec![
            Message::system(SUMMARIZE_SYSTEM),
            Message::user(format!(
                "Summarize the conversation below. Write the summary in {language}.\n\n{transcript}"
            )),
        ],
        tools: None,
        temperature: Some(0.1),
        max_tokens: Some(max_tokens.max(256)),
    };

    let stream = provider.chat(request).await?;
    let response = collect_response(stream).await?;
    let summary = response.content.trim().to_string();
    if summary.is_empty() {
        return Err(codei_llm::LlmError::Config(
            "compaction summary was empty".into(),
        ));
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use codei_config::AgentConfig;
    use codei_session::TokenBudget;

    #[test]
    fn budget_limit_uses_threshold() {
        let budget = TokenBudget::from_agent(&AgentConfig {
            context_window_tokens: 1000,
            compaction_threshold: 0.8,
            ..Default::default()
        });
        assert_eq!(budget.limit(), 800);
    }
}
