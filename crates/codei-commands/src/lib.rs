//! Slash command parsing and execution.

mod completions;
mod parse;

pub use completions::{filter_slash_hints, SlashHint};
pub use parse::{parse_input, Input, SlashCommand};

use codei_i18n::{t, t_fmt};
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
}

pub fn execute_command(
    cmd: SlashCommand,
    session: &mut Session,
    _current_model: &str,
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
