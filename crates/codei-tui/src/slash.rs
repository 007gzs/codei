//! Shared slash-command handling for interactive UIs.

use std::sync::{Arc, RwLock};

use anyhow::Result;
use codei_agent::AgentLoop;
use codei_commands::{
    execute_command, model_after_command, provider_after_command, CommandOutcome, SlashCommand,
};
use codei_config::{discover_skills, find_skill, read_skill_body};
use codei_i18n::{t, t_fmt};
use codei_llm::Usage;
use codei_session::{Session, SessionStore};

pub struct SlashContext<'a> {
    pub session: &'a mut Session,
    pub store: &'a SessionStore,
    pub model: &'a Arc<RwLock<String>>,
    pub provider_name: &'a Arc<RwLock<String>>,
    pub agent: &'a AgentLoop,
    pub token_usage: &'a mut Usage,
    pub last_turn_usage: &'a mut Option<Usage>,
}

pub async fn handle_slash(cmd: SlashCommand, ctx: &mut SlashContext<'_>) -> Result<SlashAction> {
    match &cmd {
        SlashCommand::SkillList => {
            let skills = discover_skills(ctx.agent.config());
            if skills.is_empty() {
                return Ok(SlashAction::Message(t("slash_skill_none")));
            }
            let mut lines = vec![t("slash_skill_list_title")];
            for skill in skills {
                lines.push(t_fmt(
                    "slash_skill_line",
                    &[("name", &skill.name), ("description", &skill.description)],
                ));
            }
            return Ok(SlashAction::Message(lines.join("\n")));
        }
        SlashCommand::SkillShow(name) => {
            let skills = discover_skills(ctx.agent.config());
            let Some(skill) = find_skill(&skills, name) else {
                return Ok(SlashAction::Message(t_fmt(
                    "slash_skill_not_found",
                    &[("name", name)],
                )));
            };
            let body = read_skill_body(skill).unwrap_or_else(|err| err.to_string());
            return Ok(SlashAction::Message(format!("# {}\n\n{body}", skill.name)));
        }
        SlashCommand::Compact => {
            ctx.agent.compact_session(ctx.session, ctx.store).await?;
            return Ok(SlashAction::Message(t("slash_session_compacted")));
        }
        _ => {}
    }

    let current_model = ctx.model.read().expect("model lock").clone();
    let token_stats = Some(codei_commands::TokenStats {
        session_input: ctx.token_usage.input_tokens,
        session_output: ctx.token_usage.output_tokens,
        last_input: ctx.last_turn_usage.map(|u| u.input_tokens).unwrap_or(0),
        last_output: ctx.last_turn_usage.map(|u| u.output_tokens).unwrap_or(0),
    });
    let outcome = execute_command(
        cmd,
        ctx.session,
        &current_model,
        token_stats.as_ref(),
        &ctx.agent.config().config.agent,
    );

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
                        (
                            "updated",
                            &s.updated_at.format("%Y-%m-%d %H:%M").to_string(),
                        ),
                        ("cwd", &s.cwd.display().to_string()),
                    ],
                ));
            }
            return Ok(SlashAction::Message(lines.join("\n")));
        }
        CommandOutcome::SessionNew => {
            *ctx.session = Session::new(ctx.session.cwd.clone());
            ctx.store.save(ctx.session)?;
            *ctx.token_usage = Usage::default();
            *ctx.last_turn_usage = None;
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
        CommandOutcome::Cleared => {
            ctx.store.save(ctx.session)?;
            *ctx.token_usage = Usage::default();
            *ctx.last_turn_usage = None;
            return Ok(SlashAction::Message(t("slash_session_cleared")));
        }
        _ => {}
    }

    let _ = model_after_command(&outcome, &current_model);
    let _ = provider_after_command(&outcome, &ctx.provider_name.read().expect("provider lock"));

    match outcome {
        CommandOutcome::Exit => Ok(SlashAction::Exit),
        CommandOutcome::Help(text) => Ok(SlashAction::Message(text)),
        CommandOutcome::LanguageChanged(language) => Ok(SlashAction::Message(t_fmt(
            "slash_language_changed",
            &[("language", &language)],
        ))),
        CommandOutcome::LanguageInfo(language) => Ok(SlashAction::Message(t_fmt(
            "slash_language_current",
            &[("language", &language)],
        ))),
        CommandOutcome::LanguageInvalid(message) => Ok(SlashAction::Message(t_fmt(
            "slash_language_invalid",
            &[("message", &message)],
        ))),
        CommandOutcome::TokensReport(text) => Ok(SlashAction::Message(text)),
        _ => Ok(SlashAction::Continue),
    }
}

#[derive(Debug)]
pub enum SlashAction {
    Continue,
    Exit,
    Message(String),
}
