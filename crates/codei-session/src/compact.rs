use codei_llm::Message;

/// Token budget for context window management.
#[derive(Debug, Clone, Copy)]
pub struct TokenBudget {
    pub max_tokens: u32,
    pub compaction_threshold: f32,
}

impl TokenBudget {
    pub fn limit(&self) -> u32 {
        ((self.max_tokens as f32) * self.compaction_threshold) as u32
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

/// Truncate older messages when over budget. Keeps system prompt and recent turns.
pub fn compact_messages(messages: Vec<Message>, budget: TokenBudget) -> Vec<Message> {
    if messages.is_empty() || estimate_tokens(&messages) <= budget.limit() {
        return messages;
    }

    const KEEP_RECENT: usize = 12;
    if messages.len() <= KEEP_RECENT + 1 {
        return messages;
    }

    let system = messages.first().cloned();
    let mut rest: Vec<Message> = messages.into_iter().skip(1).collect();
    if rest.len() <= KEEP_RECENT {
        return reconstruct(system, rest, 0);
    }

    let omitted = rest.len() - KEEP_RECENT;
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
        };
        let messages: Vec<Message> = std::iter::once(Message::system("sys"))
            .chain((0..20).map(|i| Message::user(format!("message {i} {}", "x".repeat(50)))))
            .collect();
        let compacted = compact_messages(messages, budget);
        assert!(compacted.len() < 21);
        assert!(compacted[0].content.as_deref() == Some("sys"));
    }
}
