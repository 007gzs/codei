//! Shared slash-command handling for interactive UIs.

use std::sync::{Arc, RwLock};

use anyhow::Result;
use codei_agent::AgentLoop;
use codei_commands::{
    execute_command, model_after_command, provider_after_command, CommandOutcome, SlashCommand,
};
use codei_i18n::{t, t_fmt};
use codei_session::{Session, SessionStore};

pub struct SlashContext<'a> {
    pub session: &'a mut Session,
    pub store: &'a SessionStore,
    pub model: &'a Arc<RwLock<String>>,
    pub provider_name: &'a Arc<RwLock<String>>,
    pub agent: &'a AgentLoop,
}

pub async fn handle_slash(cmd: SlashCommand, ctx: &mut SlashContext<'_>) -> Result<SlashAction> {
    let current_model = ctx.model.read().expect("model lock").clone();
    let outcome = execute_command(cmd, ctx.session, &current_model);

    match &outcome {
        CommandOutcome::ModelChanged(name) => {
            *ctx.model.write().expect("model lock") = name.clone();
        }
        CommandOutcome::ProviderChanged(name) => {
            ctx.agent.set_provider(name)?;
            *ctx.provider_name.write().expect("provider lock") = name.clone();
        }
        CommandOutcome::SessionList => {
            let sessions = ctx.store.list(20)?;
            let mut lines = vec![t("slash_saved_sessions")];
            for s in sessions {
                lines.push(t_fmt(
                    "slash_session_line",
                    &[
                        ("id", &s.id),
                        ("updated", &s.updated_at.format("%Y-%m-%d %H:%M").to_string()),
                        ("cwd", &s.cwd.display().to_string()),
                    ],
                ));
            }
            return Ok(SlashAction::Message(lines.join("\n")));
        }
        CommandOutcome::SessionNew => {
            *ctx.session = Session::new(ctx.session.cwd.clone());
            ctx.store.save(ctx.session)?;
            return Ok(SlashAction::Message(t_fmt(
                "slash_new_session",
                &[("id", &ctx.session.id)],
            )));
        }
        CommandOutcome::SessionResume(id) => {
            let loaded = ctx.store.load(id)?;
            let id = loaded.id.clone();
            *ctx.session = loaded;
            return Ok(SlashAction::Message(t_fmt(
                "slash_resumed_session",
                &[("id", &id)],
            )));
        }
        CommandOutcome::Compacted => {
            ctx.store.save(ctx.session)?;
            return Ok(SlashAction::Message(t("slash_session_compacted")));
        }
        CommandOutcome::Cleared => {
            ctx.store.save(ctx.session)?;
            return Ok(SlashAction::Message(t("slash_session_cleared")));
        }
        _ => {}
    }

    let _ = model_after_command(&outcome, &current_model);
    let _ = provider_after_command(&outcome, &ctx.provider_name.read().expect("provider lock"));

    match outcome {
        CommandOutcome::Exit => Ok(SlashAction::Exit),
        CommandOutcome::Help(text) => Ok(SlashAction::Message(text)),
        _ => Ok(SlashAction::Continue),
    }
}

#[derive(Debug)]
pub enum SlashAction {
    Continue,
    Exit,
    Message(String),
}
