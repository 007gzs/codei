use codei_config::AgentConfig;
use codei_llm::Message;

use crate::model::{to_llm_messages, Role, Session, StoredMessage};

/// Token budget for context window management.
#[derive(Debug, Clone, Copy)]
pub struct TokenBudget {
    pub max_tokens: u32,
    pub compaction_threshold: f32,
    pub keep_messages: usize,
}

impl TokenBudget {
    pub fn limit(&self) -> u32 {
        ((self.max_tokens as f32) * self.compaction_threshold) as u32
    }

    pub fn from_agent(agent: &AgentConfig) -> Self {
        Self {
            max_tokens: agent.context_window_tokens,
            compaction_threshold: agent.compaction_threshold,
            keep_messages: agent.compaction_keep_messages.max(2) as usize,
        }
    }
}

/// Rough token estimate (chars / 4).
pub fn estimate_tokens(messages: &[Message]) -> u32 {
    messages
        .iter()
        .map(|m| {
            let content_len = m.content.as_ref().map(|c| c.len()).unwrap_or(0);
            let tools_len = m
                .tool_calls
                .as_ref()
                .map(|calls| calls.iter().map(|c| c.arguments.len()).sum::<usize>())
                .unwrap_or(0);
            (content_len + tools_len) as u32 / 4 + 8
        })
        .sum()
}

/// Whether the session should be compacted based on estimated token usage.
pub fn should_compact_session(session: &Session, system_prompt: &str, agent: &AgentConfig) -> bool {
    let budget = TokenBudget::from_agent(agent);
    let tokens = estimate_tokens(&to_llm_messages(session, system_prompt));
    tokens > budget.limit() && session.messages.len() > budget.keep_messages
}

/// Format stored messages into a transcript for LLM summarization.
pub fn format_transcript(messages: &[StoredMessage]) -> String {
    let mut out = String::new();
    for msg in messages {
        let role = match msg.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
            Role::Tool => "Tool",
            Role::System => continue,
        };
        let Some(text) = msg.text() else { continue };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        out.push_str(role);
        out.push_str(": ");
        out.push_str(&truncate_chars(trimmed, 6_000));
        out.push_str("\n\n");
    }
    out
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    format!("{truncated}…")
}

/// Truncate older messages when over budget. Keeps system prompt and recent turns.
/// Used as a last-resort guard when building the LLM request.
pub fn compact_messages(messages: Vec<Message>, budget: TokenBudget) -> Vec<Message> {
    if messages.is_empty() || estimate_tokens(&messages) <= budget.limit() {
        return messages;
    }

    const MIN_KEEP: usize = 2;
    let keep_recent = budget.keep_messages.max(MIN_KEEP);
    if messages.len() <= keep_recent + 1 {
        return messages;
    }

    let system = messages.first().cloned();
    let mut rest: Vec<Message> = messages.into_iter().skip(1).collect();
    if rest.len() <= keep_recent {
        return reconstruct(system, rest, 0);
    }

    let omitted = rest.len() - keep_recent;
    rest.drain(0..omitted);
    reconstruct(system, rest, omitted)
}

fn reconstruct(system: Option<Message>, kept: Vec<Message>, omitted: usize) -> Vec<Message> {
    let mut out = Vec::new();
    if let Some(sys) = system {
        out.push(sys);
    }
    if omitted > 0 {
        out.push(Message::user(format!(
            "[Context compacted: {omitted} earlier messages omitted to fit the context window]"
        )));
    }
    out.extend(kept);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use codei_llm::Message;

    #[test]
    fn compacts_when_over_budget() {
        let budget = TokenBudget {
            max_tokens: 100,
            compaction_threshold: 0.5,
            keep_messages: 4,
        };
        let messages: Vec<Message> = std::iter::once(Message::system("sys"))
            .chain((0..20).map(|i| Message::user(format!("message {i} {}", "x".repeat(50)))))
            .collect();
        let compacted = compact_messages(messages, budget);
        assert!(compacted.len() < 21);
        assert!(compacted[0].content.as_deref() == Some("sys"));
    }

    #[test]
    fn should_compact_when_over_threshold() {
        use codei_config::AgentConfig;

        let mut session = Session::new(std::path::PathBuf::from("/tmp"));
        for i in 0..30 {
            session.push_user(format!("message {i} {}", "x".repeat(80)));
        }
        let agent = AgentConfig {
            context_window_tokens: 200,
            compaction_threshold: 0.5,
            compaction_keep_messages: 6,
            ..Default::default()
        };
        assert!(should_compact_session(&session, "system prompt", &agent));
    }

    #[test]
    fn compact_with_summary_inserts_summary_message() {
        let mut session = Session::new(std::path::PathBuf::from("/tmp"));
        for i in 0..5 {
            session.push_user(format!("msg {i}"));
        }
        session.compact_with_summary(2, "summary text".into());
        assert_eq!(session.messages.len(), 3);
        assert!(session.messages[0].text().unwrap().contains("summary text"));
        assert!(session.messages[1].text().unwrap().contains("msg 3"));
    }
}
