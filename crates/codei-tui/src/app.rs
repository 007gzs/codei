use std::io;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use codei_agent::{AgentError, AgentEvent, AgentLoop};
use codei_commands::{filter_slash_hints, parse_input, Input, SlashCommand, SlashHint};
use codei_config::ResolvedConfig;
use codei_i18n::{t, t_fmt};
use codei_session::{Session, SessionStore};
use codei_tools::{handler_for_policy, ApprovalPolicy, SharedApprovalGate, ToolContext};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
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
    };

    let mut runtime = AppRuntime {
        agent,
        session: Arc::new(Mutex::new(session)),
        store: Arc::new(store),
        config,
        model,
        provider_name,
        approval_gate,
        rx,
    };

    let result = run_app(&mut terminal, &mut runtime, &mut state).await;

    disable_raw_mode()?;
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
                AgentEvent::TurnComplete { .. } => {
                    flush_assistant(&mut state.lines, &mut state.assistant_buf);
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

        let slash_hints = filter_slash_hints(&state.input);
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

            let mut constraints = vec![
                Constraint::Min(5),
                Constraint::Length(3),
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
            let input_title = if state.pending_approval.is_some() {
                t("tui_input_approval")
            } else if state.running {
                t("tui_input_running")
            } else {
                t("tui_input_normal")
            };
            let input_widget = Paragraph::new(state.input.as_str())
                .block(Block::default().borders(Borders::ALL).title(input_title));
            frame.render_widget(input_widget, input_area);

            if state.cursor_visible
                && !state.running
                && state.pending_approval.is_none()
                && input_area.width > 2
            {
                let cursor_x = input_area.x + 1 + state.input.width() as u16;
                let cursor_y = input_area.y + 1;
                let max_x = input_area.x + input_area.width.saturating_sub(2);
                frame.set_cursor_position((cursor_x.min(max_x), cursor_y));
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
                ],
            ));
            frame.render_widget(status_line, chunks[status_idx]);

            if let Some(req) = &state.pending_approval {
                render_approval_modal(frame, frame.area(), req);
            }
        })?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && key.code == KeyCode::Char('c')
                {
                    if state.pending_approval.is_some() {
                        runtime.approval_gate.respond(false).await;
                        state.pending_approval = None;
                    } else {
                        break;
                    }
                }

                if state.pending_approval.is_some() {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            runtime.approval_gate.respond(true).await;
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

                if !slash_hints.is_empty() {
                    match key.code {
                        KeyCode::Up => {
                            state.completion_index =
                                state.completion_index.saturating_sub(1);
                            continue;
                        }
                        KeyCode::Down => {
                            state.completion_index = (state.completion_index + 1)
                                .min(slash_hints.len().saturating_sub(1));
                            continue;
                        }
                        KeyCode::Tab => {
                            apply_slash_completion(
                                state,
                                slash_hints[state.completion_index],
                            );
                            continue;
                        }
                        _ => {}
                    }
                }

                match key.code {
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
                    KeyCode::Enter => {
                        let line = std::mem::take(&mut state.input);
                        state.completion_index = 0;
                        if line.trim().is_empty() {
                            continue;
                        }
                        state.chat_follow_bottom = true;
                        state.lines.push(ChatLine {
                            text: format!("> {line}"),
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
                                };
                                match handle_slash(cmd, &mut ctx).await? {
                                    SlashAction::Exit => break,
                                    SlashAction::Message(text) => {
                                        state.lines.push(ChatLine {
                                            text,
                                            style: Style::default().fg(Color::Cyan),
                                        })
                                    }
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
                    KeyCode::Backspace => {
                        state.input.pop();
                    }
                    KeyCode::Delete => {
                        state.input.pop();
                    }
                    KeyCode::Char(c) => {
                        if key.modifiers.contains(KeyModifiers::CONTROL) {
                            if c == 'h' || c == '\x08' {
                                state.input.pop();
                            }
                            continue;
                        }
                        if c == '\x7f' {
                            state.input.pop();
                            continue;
                        }
                        if !c.is_control() {
                            state.input.push(c);
                        }
                    }
                    _ => {}
                }
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
        Err(err) => {
            state.lines.push(ChatLine {
                text: t_fmt(
                    "tui_agent_task_failed",
                    &[("message", &err.to_string())],
                ),
                style: Style::default().fg(Color::Red),
            });
            state.status = t("tui_status_error");
            state.running = false;
        }
    }
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
    state.input = hint.command.to_string();
    if matches!(hint.command, "/model" | "/provider" | "/session resume") {
        state.input.push(' ');
    }
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

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

fn is_chat_error_line(text: &str) -> bool {
    text.starts_with("Error:") || text.starts_with("错误：")
}
