use std::io;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use codei_agent::{AgentError, AgentEvent, AgentLoop};
use codei_commands::{filter_slash_hints, parse_input, Input, SlashCommand, SlashHint};
use codei_config::ResolvedConfig;
use codei_i18n::{t, t_fmt};
use codei_llm::Usage;
use codei_session::{Session, SessionStore};
use codei_tools::{handler_for_policy, ApprovalPolicy, SharedApprovalGate, ToolContext};
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyModifiers,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Wrap,
};
use ratatui::DefaultTerminal;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use unicode_width::UnicodeWidthStr;

use crate::clipboard::copy_to_clipboard;
use crate::launch::InteractiveLaunch;
use crate::slash::{handle_slash, SlashAction, SlashContext};

const INPUT_MIN_HEIGHT: u16 = 3;
const INPUT_MAX_HEIGHT: u16 = 10;
const INPUT_HISTORY_LIMIT: usize = 200;

struct InputHistory {
    entries: Vec<String>,
    browse_index: Option<usize>,
    draft: Option<String>,
}

impl InputHistory {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            browse_index: None,
            draft: None,
        }
    }

    fn push(&mut self, line: String) {
        if line.trim().is_empty() {
            return;
        }
        if self.entries.last() != Some(&line) {
            self.entries.push(line);
            if self.entries.len() > INPUT_HISTORY_LIMIT {
                self.entries.remove(0);
            }
        }
        self.browse_index = None;
        self.draft = None;
    }

    fn browse_older(&mut self, current_input: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        match self.browse_index {
            None => {
                self.draft = Some(current_input.to_string());
                let idx = self.entries.len() - 1;
                self.browse_index = Some(idx);
                Some(self.entries[idx].clone())
            }
            Some(0) => None,
            Some(i) => {
                let idx = i - 1;
                self.browse_index = Some(idx);
                Some(self.entries[idx].clone())
            }
        }
    }

    fn browse_newer(&mut self) -> Option<String> {
        let i = self.browse_index?;
        if i + 1 < self.entries.len() {
            let idx = i + 1;
            self.browse_index = Some(idx);
            Some(self.entries[idx].clone())
        } else {
            self.browse_index = None;
            Some(self.draft.take().unwrap_or_default())
        }
    }

    fn clear_browse(&mut self) {
        self.browse_index = None;
        self.draft = None;
    }
}

struct ChatLine {
    text: String,
    style: Style,
}

pub struct TuiOptions {
    pub auto_approve: bool,
}

pub async fn run_tui(launch: InteractiveLaunch, opts: TuiOptions) -> Result<()> {
    let InteractiveLaunch {
        config,
        provider,
        provider_name,
        model,
        session,
        store,
        mcp,
    } = launch;
    let approval_gate = Arc::new(SharedApprovalGate::new());
    let approval: Arc<dyn codei_tools::ApprovalHandler> = if opts.auto_approve {
        Arc::from(handler_for_policy(ApprovalPolicy::Never))
    } else {
        Arc::from(approval_gate.handler())
    };

    let (tx, rx) = mpsc::unbounded_channel();
    let tool_ctx = ToolContext {
        cwd: config.cwd.clone(),
        config: Arc::clone(&config),
        approval,
    };
    let provider_name = Arc::new(RwLock::new(provider_name));
    let agent = Arc::new(AgentLoop::new(
        Arc::clone(&config),
        Arc::clone(&model),
        provider,
        provider_name.read().expect("provider lock").clone(),
        tool_ctx,
        mcp,
        Some(tx),
    ));

    let mut stdout = io::stdout();
    enable_raw_mode()?;
    stdout.execute(EnableBracketedPaste)?;
    stdout.execute(PushKeyboardEnhancementFlags(
        KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
    ))?;
    stdout.execute(EnterAlternateScreen)?;
    let mut terminal = ratatui::init();

    let mut state = AppState {
        lines: vec![ChatLine {
            text: codei_i18n::t("app_tagline"),
            style: Style::default().fg(Color::Cyan),
        }],
        input: String::new(),
        model_name: model.read().expect("model lock").clone(),
        provider_label: provider_name.read().expect("provider lock").clone(),
        status: t("tui_status_idle"),
        assistant_buf: String::new(),
        running: false,
        pending_approval: None,
        turn_task: None,
        chat_scroll: 0,
        chat_follow_bottom: true,
        cursor_visible: true,
        last_blink: Instant::now(),
        completion_index: 0,
        slash_completion_dismissed: false,
        pending_quit: false,
        token_usage: Usage::default(),
        last_turn_usage: None,
        input_history: InputHistory::new(),
        input_cursor: 0,
        pending_cancel: false,
    };

    let mut runtime = AppRuntime {
        agent,
        session: Arc::new(Mutex::new(session)),
        store,
        config,
        model,
        provider_name,
        approval_gate,
        rx,
    };

    let result = run_app(&mut terminal, &mut runtime, &mut state).await;

    disable_raw_mode()?;
    let _ = stdout.execute(DisableBracketedPaste);
    let _ = stdout.execute(PopKeyboardEnhancementFlags);
    stdout.execute(LeaveAlternateScreen)?;
    ratatui::restore();

    result
}

struct AppRuntime {
    agent: Arc<AgentLoop>,
    session: Arc<Mutex<Session>>,
    store: Arc<SessionStore>,
    config: Arc<ResolvedConfig>,
    model: Arc<RwLock<String>>,
    provider_name: Arc<RwLock<String>>,
    approval_gate: Arc<SharedApprovalGate>,
    rx: mpsc::UnboundedReceiver<AgentEvent>,
}

struct AppState {
    lines: Vec<ChatLine>,
    input: String,
    model_name: String,
    provider_label: String,
    status: String,
    assistant_buf: String,
    running: bool,
    pending_approval: Option<codei_tools::ApprovalRequest>,
    turn_task: Option<JoinHandle<Result<codei_agent::TurnOutcome, AgentError>>>,
    chat_scroll: u16,
    chat_follow_bottom: bool,
    cursor_visible: bool,
    last_blink: Instant,
    completion_index: usize,
    slash_completion_dismissed: bool,
    pending_quit: bool,
    token_usage: Usage,
    last_turn_usage: Option<Usage>,
    input_history: InputHistory,
    input_cursor: usize,
    pending_cancel: bool,
}

async fn run_app(
    terminal: &mut DefaultTerminal,
    runtime: &mut AppRuntime,
    state: &mut AppState,
) -> Result<()> {
    loop {
        poll_turn_task(state).await;

        while let Ok(event) = runtime.rx.try_recv() {
            match event {
                AgentEvent::AssistantDelta { text } => {
                    state.assistant_buf.push_str(&text);
                    if let Some(last) = state.lines.last_mut() {
                        if last.style == Style::default() {
                            last.text.push_str(&text);
                            continue;
                        }
                    }
                    state.lines.push(ChatLine {
                        text: text.clone(),
                        style: Style::default(),
                    });
                }
                AgentEvent::ToolStarted { name, args } => {
                    flush_assistant(&mut state.lines, &mut state.assistant_buf);
                    state.lines.push(ChatLine {
                        text: format!("[tool:{name}] {args}"),
                        style: Style::default().fg(Color::Yellow),
                    });
                }
                AgentEvent::ToolFinished { name, result } => {
                    let prefix = if result.is_error {
                        t("tui_tool_status_error")
                    } else {
                        t("tui_tool_status_ok")
                    };
                    state.lines.push(ChatLine {
                        text: format!("[tool:{name}:{prefix}] {}", truncate(&result.content, 200)),
                        style: Style::default().fg(Color::DarkGray),
                    });
                }
                AgentEvent::TurnComplete { usage } => {
                    flush_assistant(&mut state.lines, &mut state.assistant_buf);
                    if let Some(u) = usage {
                        state.token_usage.add_assign(u);
                        state.last_turn_usage = Some(u);
                    }
                    state.status = t("tui_status_idle");
                    state.running = false;
                    state.turn_task = None;
                }
                AgentEvent::Error { message } => {
                    state.lines.push(ChatLine {
                        text: t_fmt("tui_error_prefix", &[("message", &message)]),
                        style: Style::default().fg(Color::Red),
                    });
                    state.status = t("tui_status_error");
                    state.running = false;
                    state.turn_task = None;
                }
            }
        }

        if state.pending_approval.is_none() {
            state.pending_approval = runtime.approval_gate.take_pending().await;
        }

        if state.last_blink.elapsed() >= Duration::from_millis(530) {
            state.cursor_visible = !state.cursor_visible;
            state.last_blink = Instant::now();
        }

        let raw_slash_hints = if state.input.contains('\n') {
            Vec::new()
        } else {
            filter_slash_hints(&state.input)
        };
        let slash_hints = if state.slash_completion_dismissed {
            Vec::new()
        } else {
            raw_slash_hints.clone()
        };
        if slash_hints.is_empty() {
            state.completion_index = 0;
        } else if state.completion_index >= slash_hints.len() {
            state.completion_index = slash_hints.len().saturating_sub(1);
        }

        terminal.draw(|frame| {
            let completion_rows = if slash_hints.is_empty() {
                0
            } else {
                slash_hints.len().min(6) as u16 + 2
            };

            let input_height = input_box_height(&state.input);

            let mut constraints = vec![
                Constraint::Min(5),
                Constraint::Length(input_height),
                Constraint::Length(1),
            ];
            if completion_rows > 0 {
                constraints.insert(1, Constraint::Length(completion_rows));
            }

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(constraints)
                .split(frame.area());

            let chat_area = chunks[0];
            let (input_idx, status_idx) = if completion_rows > 0 { (2, 3) } else { (1, 2) };

            let wrapped_lines = wrap_chat_lines(&state.lines, chat_area.width.saturating_sub(2));
            let visible_height = chat_area.height.saturating_sub(2) as usize;
            let total_lines = wrapped_lines.len();
            let max_scroll = total_lines.saturating_sub(visible_height) as u16;
            if state.chat_follow_bottom {
                state.chat_scroll = max_scroll;
            } else {
                state.chat_scroll = state.chat_scroll.min(max_scroll);
                state.chat_follow_bottom = state.chat_scroll >= max_scroll;
            }

            let chat_widget = Paragraph::new(wrapped_lines)
                .wrap(Wrap { trim: false })
                .scroll((state.chat_scroll, 0))
                .block(Block::default().borders(Borders::ALL).title(t_fmt(
                    "tui_chat_title",
                    &[
                        ("provider", &state.provider_label),
                        ("model", &state.model_name),
                        ("cwd", &runtime.config.cwd.display().to_string()),
                    ],
                )));
            frame.render_widget(chat_widget, chat_area);

            if total_lines > visible_height {
                let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(Some("↑"))
                    .end_symbol(Some("↓"));
                let mut scrollbar_state = ScrollbarState::new(total_lines)
                    .position(state.chat_scroll as usize)
                    .viewport_content_length(visible_height);
                frame.render_stateful_widget(
                    scrollbar,
                    chat_area.inner(Margin {
                        vertical: 1,
                        horizontal: 0,
                    }),
                    &mut scrollbar_state,
                );
            }

            if completion_rows > 0 {
                render_slash_completions(frame, chunks[1], &slash_hints, state.completion_index);
            }

            let input_area = chunks[input_idx];
            let input_title = if state.pending_cancel {
                t("tui_input_cancel")
            } else if state.pending_quit {
                t("tui_input_quit")
            } else if state.pending_approval.is_some() {
                t("tui_input_approval")
            } else if state.running {
                t("tui_input_running")
            } else {
                t("tui_input_normal")
            };
            let input_widget = Paragraph::new(input_display_text(&state.input))
                .block(Block::default().borders(Borders::ALL).title(input_title));
            frame.render_widget(input_widget, input_area);

            if state.cursor_visible
                && !state.running
                && state.pending_approval.is_none()
                && !state.pending_quit
                && !state.pending_cancel
                && input_area.width > 2
            {
                let (cursor_x, cursor_y) =
                    input_cursor_pos(&state.input, state.input_cursor, input_area);
                frame.set_cursor_position((cursor_x, cursor_y));
            }

            let status_line = Paragraph::new(t_fmt(
                "tui_status_bar",
                &[
                    ("status", &state.status),
                    (
                        "session",
                        &runtime
                            .session
                            .try_lock()
                            .map(|s| s.id.clone())
                            .unwrap_or_else(|_| "?".into()),
                    ),
                    ("input_tokens", &state.token_usage.input_tokens.to_string()),
                    (
                        "output_tokens",
                        &state.token_usage.output_tokens.to_string(),
                    ),
                ],
            ));
            frame.render_widget(status_line, chunks[status_idx]);

            if let Some(req) = &state.pending_approval {
                render_approval_modal(frame, frame.area(), req);
            } else if state.pending_cancel {
                render_cancel_modal(frame, frame.area());
            } else if state.pending_quit {
                render_quit_modal(frame, frame.area());
            }
        })?;

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Paste(text)
                    if !state.running
                        && state.pending_approval.is_none()
                        && !state.pending_quit
                        && !state.pending_cancel =>
                {
                    insert_input_text(state, &text);
                    continue;
                }
                Event::Key(key) => {
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c')
                    {
                        if state.pending_approval.is_some() {
                            runtime.approval_gate.respond(false).await;
                            state.pending_approval = None;
                        } else if state.pending_cancel {
                            cancel_running_turn(state);
                        } else if state.running {
                            state.pending_cancel = true;
                        } else if state.pending_quit {
                            break;
                        } else {
                            state.pending_quit = true;
                        }
                        continue;
                    }

                    if state.pending_cancel {
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                                cancel_running_turn(state);
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                state.pending_cancel = false;
                            }
                            _ => {}
                        }
                        continue;
                    }

                    if state.pending_quit {
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => break,
                            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                state.pending_quit = false;
                            }
                            _ => {}
                        }
                        continue;
                    }

                    if state.pending_approval.is_some() {
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                runtime.approval_gate.respond(true).await;
                                state.pending_approval = None;
                            }
                            KeyCode::Char('a') | KeyCode::Char('A') => {
                                runtime.approval_gate.approve_always().await;
                                state.pending_approval = None;
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                runtime.approval_gate.respond(false).await;
                                state.pending_approval = None;
                            }
                            _ => {}
                        }
                        continue;
                    }

                    if state.running {
                        continue;
                    }

                    if !raw_slash_hints.is_empty() && !state.slash_completion_dismissed {
                        match key.code {
                            KeyCode::Up => {
                                state.completion_index = state.completion_index.saturating_sub(1);
                                continue;
                            }
                            KeyCode::Down => {
                                state.completion_index = (state.completion_index + 1)
                                    .min(raw_slash_hints.len().saturating_sub(1));
                                continue;
                            }
                            KeyCode::Tab => {
                                apply_slash_completion(
                                    state,
                                    raw_slash_hints[state.completion_index],
                                );
                                continue;
                            }
                            KeyCode::Esc => {
                                state.slash_completion_dismissed = true;
                                state.completion_index = 0;
                                continue;
                            }
                            _ => {}
                        }
                    }

                    match key.code {
                        KeyCode::Up => {
                            if let Some(text) = state.input_history.browse_older(&state.input) {
                                mark_input_edited(state);
                                state.input = text;
                                state.input_cursor = state.input.len();
                            }
                            continue;
                        }
                        KeyCode::Down => {
                            if let Some(text) = state.input_history.browse_newer() {
                                mark_input_edited(state);
                                state.input = text;
                                state.input_cursor = state.input.len();
                            }
                            continue;
                        }
                        KeyCode::Left => {
                            state.input_cursor =
                                prev_char_boundary(&state.input, state.input_cursor);
                            continue;
                        }
                        KeyCode::Right => {
                            state.input_cursor =
                                next_char_boundary(&state.input, state.input_cursor);
                            continue;
                        }
                        KeyCode::PageUp => {
                            scroll_chat(state, -3);
                            continue;
                        }
                        KeyCode::PageDown => {
                            scroll_chat(state, 3);
                            continue;
                        }
                        KeyCode::Home => {
                            state.chat_scroll = 0;
                            state.chat_follow_bottom = false;
                            continue;
                        }
                        KeyCode::End => {
                            state.chat_follow_bottom = true;
                            continue;
                        }
                        KeyCode::Esc => {
                            if !state.input.is_empty() {
                                state.input.clear();
                                state.input_cursor = 0;
                                state.completion_index = 0;
                                state.slash_completion_dismissed = false;
                                state.input_history.clear_browse();
                            }
                            continue;
                        }
                        KeyCode::Char('y') | KeyCode::Char('Y')
                            if key.modifiers.contains(KeyModifiers::CONTROL)
                                && key.modifiers.contains(KeyModifiers::SHIFT) =>
                        {
                            copy_chat_with_status(state, CopyScope::All);
                            continue;
                        }
                        KeyCode::Char('l') | KeyCode::Char('L')
                            if key.modifiers.contains(KeyModifiers::CONTROL)
                                && key.modifiers.contains(KeyModifiers::SHIFT) =>
                        {
                            copy_chat_with_status(state, CopyScope::LastAssistant);
                            continue;
                        }
                        KeyCode::Enter if key_inserts_newline(&key) => {
                            insert_newline_at_cursor(state);
                        }
                        KeyCode::Enter => {
                            let line = std::mem::take(&mut state.input);
                            state.input_cursor = 0;
                            state.completion_index = 0;
                            state.input_history.clear_browse();
                            if line.trim().is_empty() {
                                continue;
                            }
                            state.input_history.push(line.clone());
                            state.chat_follow_bottom = true;
                            state.lines.push(ChatLine {
                                text: format_user_prompt(&line),
                                style: Style::default().fg(Color::Green),
                            });

                            match parse_input(&line) {
                                Input::SlashCommand(SlashCommand::Copy) => {
                                    copy_chat_with_status(state, CopyScope::All);
                                }
                                Input::SlashCommand(SlashCommand::CopyLast) => {
                                    copy_chat_with_status(state, CopyScope::LastAssistant);
                                }
                                Input::SlashCommand(cmd) => {
                                    let mut session = runtime.session.lock().await;
                                    let mut ctx = SlashContext {
                                        session: &mut session,
                                        store: &runtime.store,
                                        model: &runtime.model,
                                        provider_name: &runtime.provider_name,
                                        agent: runtime.agent.as_ref(),
                                        token_usage: &mut state.token_usage,
                                        last_turn_usage: &mut state.last_turn_usage,
                                    };
                                    match handle_slash(cmd, &mut ctx).await? {
                                        SlashAction::Exit => break,
                                        SlashAction::Message(text) => state.lines.push(ChatLine {
                                            text,
                                            style: Style::default().fg(Color::Cyan),
                                        }),
                                        SlashAction::Continue => {}
                                    }
                                    state.model_name =
                                        runtime.model.read().expect("model lock").clone();
                                    state.provider_label = runtime
                                        .provider_name
                                        .read()
                                        .expect("provider lock")
                                        .clone();
                                }
                                Input::UserMessage(msg) => {
                                    start_agent_turn(runtime, state, msg);
                                }
                            }
                        }
                        KeyCode::Backspace => backspace_at_cursor(state),
                        KeyCode::Delete => delete_at_cursor(state),
                        KeyCode::Char(c) => {
                            if key.modifiers.contains(KeyModifiers::CONTROL) {
                                if c == 'h' || c == '\x08' {
                                    backspace_at_cursor(state);
                                } else if c == 'j' {
                                    insert_newline_at_cursor(state);
                                }
                                continue;
                            }
                            if c == '\x7f' {
                                backspace_at_cursor(state);
                                continue;
                            }
                            if c == '\n' {
                                insert_newline_at_cursor(state);
                                continue;
                            }
                            if !c.is_control() {
                                insert_char_at_cursor(state, c);
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        } else if state.running {
            tokio::task::yield_now().await;
        }
    }

    Ok(())
}

fn start_agent_turn(runtime: &AppRuntime, state: &mut AppState, msg: String) {
    state.status = t("tui_status_running");
    state.running = true;
    state.chat_follow_bottom = true;
    state.assistant_buf.clear();
    state.lines.push(ChatLine {
        text: String::new(),
        style: Style::default(),
    });

    let agent = Arc::clone(&runtime.agent);
    let session = Arc::clone(&runtime.session);
    let store = Arc::clone(&runtime.store);

    state.turn_task = Some(tokio::spawn(async move {
        let mut session = session.lock().await;
        agent.run_turn(&mut session, &msg, &store).await
    }));
}

async fn poll_turn_task(state: &mut AppState) {
    let finished = state
        .turn_task
        .as_ref()
        .is_some_and(|task| task.is_finished());
    if !finished {
        return;
    }
    let Some(task) = state.turn_task.take() else {
        return;
    };
    match task.await {
        Ok(Ok(_)) => {}
        Ok(Err(err)) => {
            state.lines.push(ChatLine {
                text: t_fmt("tui_error_prefix", &[("message", &err.to_string())]),
                style: Style::default().fg(Color::Red),
            });
            state.status = t("tui_status_error");
            state.running = false;
        }
        Err(err) if err.is_cancelled() => {
            if state.running {
                finish_cancel_turn(state);
            }
        }
        Err(err) => {
            state.lines.push(ChatLine {
                text: t_fmt("tui_agent_task_failed", &[("message", &err.to_string())]),
                style: Style::default().fg(Color::Red),
            });
            state.status = t("tui_status_error");
            state.running = false;
        }
    }
}

fn key_inserts_newline(key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Enter => key
            .modifiers
            .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::CONTROL),
        KeyCode::Char('\n') => true,
        _ => false,
    }
}

fn normalize_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn mark_input_edited(state: &mut AppState) {
    state.slash_completion_dismissed = false;
}

fn insert_input_text(state: &mut AppState, text: &str) {
    mark_input_edited(state);
    state.input_history.clear_browse();
    let pos = state.input_cursor.min(state.input.len());
    let normalized = normalize_line_endings(text);
    state.input.insert_str(pos, &normalized);
    state.input_cursor = pos + normalized.len();
}

fn insert_char_at_cursor(state: &mut AppState, ch: char) {
    mark_input_edited(state);
    state.input_history.clear_browse();
    let pos = state.input_cursor.min(state.input.len());
    state.input.insert(pos, ch);
    state.input_cursor = pos + ch.len_utf8();
}

fn insert_newline_at_cursor(state: &mut AppState) {
    mark_input_edited(state);
    state.input_history.clear_browse();
    let pos = state.input_cursor.min(state.input.len());
    state.input.insert(pos, '\n');
    state.input_cursor = pos + 1;
}

fn backspace_at_cursor(state: &mut AppState) {
    if state.input_cursor == 0 {
        return;
    }
    mark_input_edited(state);
    state.input_history.clear_browse();
    let start = prev_char_boundary(&state.input, state.input_cursor);
    state.input.drain(start..state.input_cursor);
    state.input_cursor = start;
}

fn delete_at_cursor(state: &mut AppState) {
    if state.input_cursor >= state.input.len() {
        return;
    }
    mark_input_edited(state);
    state.input_history.clear_browse();
    let end = next_char_boundary(&state.input, state.input_cursor);
    state.input.drain(state.input_cursor..end);
}

fn prev_char_boundary(text: &str, cursor: usize) -> usize {
    let cursor = cursor.min(text.len());
    if cursor == 0 {
        return 0;
    }
    let mut pos = cursor - 1;
    while pos > 0 && !text.is_char_boundary(pos) {
        pos -= 1;
    }
    if !text.is_char_boundary(pos) {
        return 0;
    }
    pos
}

fn next_char_boundary(text: &str, cursor: usize) -> usize {
    let cursor = cursor.min(text.len());
    if cursor >= text.len() {
        return text.len();
    }
    let mut pos = cursor + 1;
    while pos < text.len() && !text.is_char_boundary(pos) {
        pos += 1;
    }
    pos
}

fn cancel_running_turn(state: &mut AppState) {
    if let Some(task) = state.turn_task.take() {
        task.abort();
    }
    finish_cancel_turn(state);
    state.pending_cancel = false;
}

fn finish_cancel_turn(state: &mut AppState) {
    state.running = false;
    state.status = t("tui_status_idle");
    state.assistant_buf.clear();
    if let Some(last) = state.lines.last() {
        if last.style == Style::default() && last.text.is_empty() {
            state.lines.pop();
        } else if let Some(last) = state.lines.last_mut() {
            if last.style == Style::default() {
                last.text
                    .push_str(&format!("\n\n[{}]", t("tui_turn_cancelled")));
            }
        }
    }
}

fn input_display_text(input: &str) -> Text<'static> {
    Text::from(input_display_lines(input))
}

fn input_display_lines(input: &str) -> Vec<Line<'static>> {
    if input.is_empty() {
        return vec![Line::from("")];
    }
    input
        .split('\n')
        .map(|line| Line::from(line.to_string()))
        .collect()
}

fn format_user_prompt(text: &str) -> String {
    if !text.contains('\n') {
        return format!("> {text}");
    }
    text.lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn input_box_height(text: &str) -> u16 {
    let line_count = input_display_lines(text).len().max(1);
    (line_count as u16 + 2).clamp(INPUT_MIN_HEIGHT, INPUT_MAX_HEIGHT)
}

fn input_cursor_pos(input: &str, cursor: usize, area: Rect) -> (u16, u16) {
    let inner_left = area.x + 1;
    let inner_bottom = area.y + area.height.saturating_sub(2);
    let cursor = cursor.min(input.len());
    if input.is_empty() {
        return (inner_left, area.y + 1);
    }

    let before = &input[..cursor];
    let line_idx = before.chars().filter(|&c| c == '\n').count();
    let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let col = input[line_start..cursor].width();

    let y = area.y + 1 + line_idx as u16;
    let x = inner_left + col as u16;
    let max_x = area.x + area.width.saturating_sub(2);
    (x.min(max_x), y.min(inner_bottom))
}

fn scroll_chat(state: &mut AppState, delta: i16) {
    state.chat_follow_bottom = false;
    if delta < 0 {
        state.chat_scroll = state.chat_scroll.saturating_sub((-delta) as u16);
    } else {
        state.chat_scroll = state.chat_scroll.saturating_add(delta as u16);
    }
}

#[derive(Clone, Copy)]
enum CopyScope {
    All,
    LastAssistant,
}

fn copy_chat_with_status(state: &mut AppState, scope: CopyScope) {
    let text = match scope {
        CopyScope::All => chat_text_all(&state.lines),
        CopyScope::LastAssistant => match last_assistant_text(&state.lines) {
            Some(text) => text,
            None => {
                state.status = t("tui_copy_nothing");
                return;
            }
        },
    };
    match copy_to_clipboard(&text) {
        Ok(()) => {
            state.status = t_fmt(
                "tui_copy_ok",
                &[("count", &text.chars().count().to_string())],
            );
        }
        Err(err) => {
            state.status = t_fmt("tui_copy_failed", &[("error", &format!("{err:#}"))]);
        }
    }
}

fn chat_text_all(lines: &[ChatLine]) -> String {
    lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn last_assistant_text(lines: &[ChatLine]) -> Option<String> {
    lines
        .iter()
        .rev()
        .find(|line| {
            line.style == Style::default()
                && !line.text.starts_with("[tool:")
                && !is_chat_error_line(&line.text)
        })
        .map(|line| line.text.clone())
}

fn apply_slash_completion(state: &mut AppState, hint: &SlashHint) {
    mark_input_edited(state);
    state.input = hint.command.to_string();
    if matches!(
        hint.command,
        "/model" | "/provider" | "/session resume" | "/skill show" | "/language"
    ) {
        state.input.push(' ');
    }
    state.input_cursor = state.input.len();
    state.input_history.clear_browse();
}

fn render_slash_completions(
    frame: &mut ratatui::Frame,
    area: Rect,
    hints: &[&SlashHint],
    selected: usize,
) {
    let items: Vec<ListItem> = hints
        .iter()
        .enumerate()
        .map(|(idx, hint)| {
            let style = if idx == selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<18}", hint.command), style),
                Span::styled(t(hint.description_key), style),
            ]))
        })
        .collect();
    let widget = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(t("tui_commands_title"))
            .style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(Clear, area);
    frame.render_widget(widget, area);
}

fn wrap_chat_lines(lines: &[ChatLine], area_width: u16) -> Vec<Line<'static>> {
    let inner = area_width.saturating_sub(2) as usize;
    let mut rendered = Vec::new();
    for line in lines {
        for segment in wrap_text(&line.text, inner.max(1)) {
            rendered.push(Line::from(Span::styled(segment, line.style)));
        }
    }
    rendered
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        if ch == '\n' {
            lines.push(std::mem::take(&mut current));
            continue;
        }
        if current.chars().count() + 1 > width && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        lines.push(current);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    // Word-aware second pass: try to break at spaces for long lines.
    let mut refined = Vec::new();
    for line in lines {
        if line.chars().count() <= width {
            refined.push(line);
            continue;
        }
        let mut rest: String = line;
        while !rest.is_empty() {
            if rest.chars().count() <= width {
                refined.push(rest);
                break;
            }
            let byte_idx = rest
                .char_indices()
                .nth(width)
                .map(|(i, _)| i)
                .unwrap_or(rest.len());
            let mut break_at = byte_idx;
            if let Some(space) = rest[..byte_idx].rfind(' ') {
                if space > 0 {
                    break_at = space;
                }
            }
            let (part, remainder) = rest.split_at(break_at);
            refined.push(part.trim_end().to_string());
            rest = remainder.trim_start().to_string();
        }
    }
    refined
}

fn render_cancel_modal(frame: &mut ratatui::Frame, area: Rect) {
    let popup = centered_rect(60, 20, area);
    frame.render_widget(Clear, popup);
    let widget = Paragraph::new(t("tui_cancel_body"))
        .wrap(Wrap { trim: false })
        .style(Style::default().add_modifier(Modifier::BOLD))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(t("tui_cancel_title"))
                .style(Style::default().fg(Color::Yellow)),
        );
    frame.render_widget(widget, popup);
}

fn render_quit_modal(frame: &mut ratatui::Frame, area: Rect) {
    let popup = centered_rect(60, 20, area);
    frame.render_widget(Clear, popup);
    let widget = Paragraph::new(t("tui_quit_body"))
        .wrap(Wrap { trim: false })
        .style(Style::default().add_modifier(Modifier::BOLD))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(t("tui_quit_title"))
                .style(Style::default().fg(Color::Yellow)),
        );
    frame.render_widget(widget, popup);
}

fn render_approval_modal(
    frame: &mut ratatui::Frame,
    area: Rect,
    request: &codei_tools::ApprovalRequest,
) {
    let popup = centered_rect(70, 30, area);
    frame.render_widget(Clear, popup);
    let text = t_fmt(
        "tui_tool_approval_body",
        &[
            ("name", &request.tool_name),
            ("args", &request.arguments.to_string()),
        ],
    );
    let widget = Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .style(Style::default().add_modifier(Modifier::BOLD))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(t("tui_tool_approval_title"))
                .style(Style::default().fg(Color::Yellow)),
        );
    frame.render_widget(widget, popup);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn flush_assistant(lines: &mut [ChatLine], assistant_buf: &mut String) {
    if !assistant_buf.is_empty() {
        assistant_buf.clear();
    }
    let _ = lines;
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}...")
    }
}

fn is_chat_error_line(text: &str) -> bool {
    text.starts_with("Error:") || text.starts_with("错误：")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_line_endings_unifies_crlf_and_cr() {
        assert_eq!(normalize_line_endings("a\r\nb\rc"), "a\nb\nc");
    }

    #[test]
    fn input_display_lines_preserves_trailing_newline() {
        let lines = input_display_lines("a\nb\n");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[2], Line::from(""));
    }

    #[test]
    fn key_inserts_newline_for_alt_enter_and_ctrl_enter() {
        assert!(key_inserts_newline(&KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::ALT,
        )));
        assert!(key_inserts_newline(&KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::CONTROL,
        )));
        assert!(!key_inserts_newline(&KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::empty(),
        )));
    }

    #[test]
    fn input_history_browses_submitted_messages() {
        let mut history = InputHistory::new();
        history.push("first".into());
        history.push("second".into());

        assert_eq!(history.browse_older(""), as_deref("second"));
        assert_eq!(history.browse_older("ignored"), as_deref("first"));
        assert_eq!(history.browse_older("ignored"), None);
        assert_eq!(history.browse_newer(), as_deref("second"));
        assert_eq!(history.browse_newer(), Some(String::new()));
    }

    #[test]
    fn input_history_restores_draft_after_browse() {
        let mut history = InputHistory::new();
        history.push("old".into());
        assert_eq!(history.browse_older("draft text"), as_deref("old"));
        assert_eq!(history.browse_newer(), Some("draft text".into()));
    }

    fn as_deref(value: &str) -> Option<String> {
        Some(value.to_string())
    }

    #[test]
    fn input_cursor_pos_tracks_byte_offset() {
        use ratatui::layout::Rect;

        let area = Rect::new(0, 0, 40, 5);
        let input = "ab\ncd";
        let (x, y) = input_cursor_pos(input, 4, area);
        assert_eq!(y, 2);
        assert_eq!(x, 2);
    }

    #[test]
    fn prev_char_boundary_skips_utf8() {
        let text = "aéb";
        let end = text.len();
        let mid = prev_char_boundary(text, end);
        assert_eq!(&text[..mid], "aé");
    }

    #[test]
    fn truncate_respects_utf8_char_boundary() {
        let text = "终端优先的 AI 编程 Agent";
        let out = truncate(text, 5);
        assert!(out.ends_with("..."));
        assert_eq!(out.chars().count(), 5 + 3);
    }
}
