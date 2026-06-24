//! Slash command parsing and execution.

mod completions;
mod parse;

pub use completions::{filter_slash_hints, SlashHint};
pub use parse::{parse_input, Input, SlashCommand};

use codei_i18n::{locale, set_locale, t, t_fmt};
use codei_session::Session;

#[derive(Debug, Clone)]
pub enum CommandOutcome {
    Continue,
    Exit,
    ModelChanged(String),
    ProviderChanged(String),
    Cleared,
    Compacted,
    SessionList,
    SessionNew,
    SessionResume(String),
    Help(String),
    LanguageChanged(String),
    LanguageInfo(String),
    LanguageInvalid(String),
    TokensReport(String),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TokenStats {
    pub session_input: u32,
    pub session_output: u32,
    pub last_input: u32,
    pub last_output: u32,
}

pub fn execute_command(
    cmd: SlashCommand,
    session: &mut Session,
    _current_model: &str,
    token_stats: Option<&TokenStats>,
) -> CommandOutcome {
    match cmd {
        SlashCommand::Help => CommandOutcome::Help(help_text()),
        SlashCommand::Exit => CommandOutcome::Exit,
        SlashCommand::Clear => {
            session.clear_messages();
            CommandOutcome::Cleared
        }
        SlashCommand::Compact => {
            session.compact(12);
            CommandOutcome::Compacted
        }
        SlashCommand::Model(name) => CommandOutcome::ModelChanged(name),
        SlashCommand::Provider(name) => CommandOutcome::ProviderChanged(name),
        SlashCommand::SessionList => CommandOutcome::SessionList,
        SlashCommand::SessionNew => CommandOutcome::SessionNew,
        SlashCommand::SessionResume(id) => CommandOutcome::SessionResume(id),
        SlashCommand::Copy | SlashCommand::CopyLast => CommandOutcome::Continue,
        SlashCommand::Language(language) => {
            if language.is_empty() {
                CommandOutcome::LanguageInfo(locale())
            } else if let Err(err) = set_locale(&language) {
                CommandOutcome::LanguageInvalid(err.to_string())
            } else {
                CommandOutcome::LanguageChanged(language)
            }
        }
        SlashCommand::Tokens => {
            let Some(stats) = token_stats else {
                return CommandOutcome::TokensReport(t("slash_tokens_none"));
            };
            if stats.session_input == 0 && stats.session_output == 0 {
                CommandOutcome::TokensReport(t("slash_tokens_none"))
            } else {
                CommandOutcome::TokensReport(t_fmt(
                    "slash_tokens_report",
                    &[
                        ("input", &stats.session_input.to_string()),
                        ("output", &stats.session_output.to_string()),
                        ("total", &(stats.session_input + stats.session_output).to_string()),
                        ("last_input", &stats.last_input.to_string()),
                        ("last_output", &stats.last_output.to_string()),
                    ],
                ))
            }
        }
        SlashCommand::Unknown(raw) => CommandOutcome::Help(format!(
            "{}\n\n{}",
            t_fmt("slash_unknown_command", &[("command", &raw)]),
            help_text()
        )),
    }
}

fn help_text() -> String {
    [
        t("slash_help_title"),
        t("slash_help_help"),
        t("slash_help_exit"),
        t("slash_help_clear"),
        t("slash_help_compact"),
        t("slash_help_copy"),
        t("slash_help_copy_last"),
        t("slash_help_model"),
        t("slash_help_provider"),
        t("slash_help_language"),
        t("slash_help_tokens"),
        t("slash_help_session_list"),
        t("slash_help_session_new"),
        t("slash_help_session_resume"),
    ]
    .join("\n")
}

pub fn model_after_command(outcome: &CommandOutcome, current: &str) -> String {
    match outcome {
        CommandOutcome::ModelChanged(model) => model.clone(),
        _ => current.to_string(),
    }
}

pub fn provider_after_command(outcome: &CommandOutcome, current: &str) -> String {
    match outcome {
        CommandOutcome::ProviderChanged(name) => name.clone(),
        _ => current.to_string(),
    }
}
