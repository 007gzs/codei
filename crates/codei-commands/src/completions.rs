//! Slash-command hints for interactive completion.

#[derive(Debug, Clone, Copy)]
pub struct SlashHint {
    pub command: &'static str,
    pub description_key: &'static str,
}

const HINTS: &[SlashHint] = &[
    SlashHint {
        command: "/help",
        description_key: "slash_help_desc",
    },
    SlashHint {
        command: "/exit",
        description_key: "slash_exit_desc",
    },
    SlashHint {
        command: "/quit",
        description_key: "slash_quit_desc",
    },
    SlashHint {
        command: "/clear",
        description_key: "slash_clear_desc",
    },
    SlashHint {
        command: "/compact",
        description_key: "slash_compact_desc",
    },
    SlashHint {
        command: "/copy",
        description_key: "slash_copy_desc",
    },
    SlashHint {
        command: "/copy last",
        description_key: "slash_copy_last_desc",
    },
    SlashHint {
        command: "/model",
        description_key: "slash_model_desc",
    },
    SlashHint {
        command: "/provider",
        description_key: "slash_provider_desc",
    },
    SlashHint {
        command: "/session list",
        description_key: "slash_session_list_desc",
    },
    SlashHint {
        command: "/session new",
        description_key: "slash_session_new_desc",
    },
    SlashHint {
        command: "/session resume",
        description_key: "slash_session_resume_desc",
    },
];

/// Return slash commands matching the current input prefix.
pub fn filter_slash_hints(input: &str) -> Vec<&'static SlashHint> {
    let query = input.trim().to_ascii_lowercase();
    if !query.starts_with('/') {
        return Vec::new();
    }
    if query == "/" {
        return HINTS.iter().collect();
    }
    HINTS
        .iter()
        .filter(|hint| hint.command.starts_with(&query))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_all_on_slash_only() {
        assert_eq!(filter_slash_hints("/").len(), HINTS.len());
    }

    #[test]
    fn filters_by_prefix() {
        let hints = filter_slash_hints("/hel");
        assert!(hints.iter().any(|h| h.command == "/help"));
        assert!(!hints.iter().any(|h| h.command == "/clear"));
    }

    #[test]
    fn filters_session_subcommands() {
        let hints = filter_slash_hints("/session l");
        assert!(hints.iter().any(|h| h.command == "/session list"));
    }
}
