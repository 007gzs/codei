#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    UserMessage(String),
    SlashCommand(SlashCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommand {
    Help,
    Exit,
    Clear,
    Compact,
    Model(String),
    Provider(String),
    SessionList,
    SessionNew,
    SessionResume(String),
    Copy,
    CopyLast,
    Language(String),
    Tokens,
    Unknown(String),
}

pub fn parse_input(line: &str) -> Input {
    let trimmed = line.trim();
    if !trimmed.starts_with('/') {
        return Input::UserMessage(trimmed.to_string());
    }

    let mut parts = trimmed.split_whitespace();
    let cmd = parts.next().unwrap_or("").to_ascii_lowercase();
    match cmd.as_str() {
        "/help" => Input::SlashCommand(SlashCommand::Help),
        "/exit" | "/quit" => Input::SlashCommand(SlashCommand::Exit),
        "/clear" => Input::SlashCommand(SlashCommand::Clear),
        "/compact" => Input::SlashCommand(SlashCommand::Compact),
        "/copy" => match parts.next().map(str::to_ascii_lowercase).as_deref() {
            Some("last") => Input::SlashCommand(SlashCommand::CopyLast),
            None => Input::SlashCommand(SlashCommand::Copy),
            _ => Input::SlashCommand(SlashCommand::Unknown(trimmed.to_string())),
        },
        "/model" => {
            let model = parts.collect::<Vec<_>>().join(" ");
            if model.is_empty() {
                Input::SlashCommand(SlashCommand::Unknown(trimmed.to_string()))
            } else {
                Input::SlashCommand(SlashCommand::Model(model))
            }
        }
        "/provider" => {
            let provider = parts.collect::<Vec<_>>().join(" ");
            if provider.is_empty() {
                Input::SlashCommand(SlashCommand::Unknown(trimmed.to_string()))
            } else {
                Input::SlashCommand(SlashCommand::Provider(provider))
            }
        }
        "/language" | "/lang" => {
            let language = parts.collect::<Vec<_>>().join(" ");
            Input::SlashCommand(SlashCommand::Language(language))
        }
        "/tokens" => Input::SlashCommand(SlashCommand::Tokens),
        "/session" => match parts.next().map(str::to_ascii_lowercase).as_deref() {
            Some("list") => Input::SlashCommand(SlashCommand::SessionList),
            Some("new") => Input::SlashCommand(SlashCommand::SessionNew),
            Some("resume") => {
                let id = parts.collect::<Vec<_>>().join(" ");
                if id.is_empty() {
                    Input::SlashCommand(SlashCommand::Unknown(trimmed.to_string()))
                } else {
                    Input::SlashCommand(SlashCommand::SessionResume(id))
                }
            }
            _ => Input::SlashCommand(SlashCommand::Unknown(trimmed.to_string())),
        },
        _ => Input::SlashCommand(SlashCommand::Unknown(trimmed.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_user_message() {
        assert!(matches!(parse_input("hello"), Input::UserMessage(s) if s == "hello"));
    }

    #[test]
    fn parses_model_command() {
        assert!(matches!(
            parse_input("/model gpt-4o-mini"),
            Input::SlashCommand(SlashCommand::Model(s)) if s == "gpt-4o-mini"
        ));
    }

    #[test]
    fn parses_session_list() {
        assert!(matches!(
            parse_input("/session list"),
            Input::SlashCommand(SlashCommand::SessionList)
        ));
    }

    #[test]
    fn parses_copy_commands() {
        assert!(matches!(
            parse_input("/copy"),
            Input::SlashCommand(SlashCommand::Copy)
        ));
        assert!(matches!(
            parse_input("/copy last"),
            Input::SlashCommand(SlashCommand::CopyLast)
        ));
    }

    #[test]
    fn parses_language_command() {
        assert!(matches!(
            parse_input("/language zh-CN"),
            Input::SlashCommand(SlashCommand::Language(s)) if s == "zh-CN"
        ));
        assert!(matches!(
            parse_input("/lang en-US"),
            Input::SlashCommand(SlashCommand::Language(s)) if s == "en-US"
        ));
    }

    #[test]
    fn parses_tokens_command() {
        assert!(matches!(
            parse_input("/tokens"),
            Input::SlashCommand(SlashCommand::Tokens)
        ));
    }
}
