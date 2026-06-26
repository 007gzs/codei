use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use codei_agent::AgentEvent;
use codei_session::{Role, Session};
use codei_tools::ApprovalPolicy;
use futures_util::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::session::SessionService;
use crate::turn::spawn_turn;

const INDEX_HTML: &str = include_str!("../../assets/index.html");

pub fn routes(default_cwd: PathBuf) -> Router<Arc<SessionService>> {
    Router::new()
        .route("/", get(index))
        .route("/api/config", get(move || config(default_cwd.clone())))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/{id}", get(get_session))
        .route("/api/sessions/{id}/chat", post(chat))
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

#[derive(Serialize)]
struct ServerConfig {
    default_cwd: String,
}

async fn config(default_cwd: PathBuf) -> Json<ServerConfig> {
    Json(ServerConfig {
        default_cwd: default_cwd.to_string_lossy().into_owned(),
    })
}

#[derive(Deserialize)]
struct CreateSessionRequest {
    cwd: String,
}

#[derive(Serialize)]
struct SessionSummary {
    id: String,
    cwd: String,
    title: Option<String>,
    message_count: usize,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

async fn list_sessions(
    State(service): State<Arc<SessionService>>,
) -> Result<Json<Vec<SessionSummary>>, ApiError> {
    let sessions = service.list_sessions().map_err(ApiError::storage)?;
    Ok(Json(
        sessions
            .iter()
            .map(|session| session_to_summary(&service, session))
            .collect(),
    ))
}

async fn create_session(
    State(service): State<Arc<SessionService>>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Json<SessionSummary>, ApiError> {
    let session = service
        .create_session(req.cwd.into())
        .await
        .map_err(ApiError::bad_request)?;

    Ok(Json(session_to_summary(&service, &session)))
}

async fn get_session(
    State(service): State<Arc<SessionService>>,
    Path(id): Path<String>,
) -> Result<Json<SessionDetail>, ApiError> {
    let handle = service
        .get_or_load(&id)
        .await
        .map_err(|_| ApiError::not_found("session"))?;
    let session = handle.session.read().await;

    let mut tool_names = std::collections::HashMap::new();
    for msg in &session.messages {
        if let Some(calls) = &msg.tool_calls {
            for call in calls {
                tool_names.insert(call.id.clone(), call.name.clone());
            }
        }
    }

    Ok(Json(SessionDetail {
        id: session.id.clone(),
        cwd: session.cwd.to_string_lossy().into_owned(),
        title: session.title.clone(),
        messages: session
            .messages
            .iter()
            .filter_map(|m| {
                let text = m.text()?;
                if m.role == Role::Assistant && text.trim().is_empty() {
                    return None;
                }
                Some(ChatMessage {
                    role: role_name(m.role).to_string(),
                    content: text.to_string(),
                    tool_name: m
                        .tool_call_id
                        .as_ref()
                        .and_then(|id| tool_names.get(id).cloned()),
                })
            })
            .collect(),
    }))
}

#[derive(Serialize)]
struct SessionDetail {
    id: String,
    cwd: String,
    title: Option<String>,
    messages: Vec<ChatMessage>,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
}

async fn chat(
    State(service): State<Arc<SessionService>>,
    Path(id): Path<String>,
    Json(req): Json<ChatRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let message = req.message.trim().to_string();
    if message.is_empty() {
        return Err(ApiError::bad_request("message cannot be empty"));
    }

    let handle = service
        .get_or_load(&id)
        .await
        .map_err(|_| ApiError::not_found("session"))?;

    let (tx, rx) = mpsc::unbounded_channel();
    spawn_turn(handle, message, ApprovalPolicy::Never, tx);

    let stream = UnboundedReceiverStream::new(rx).map(|event| {
        let payload = server_event_from_agent(&event);
        Ok(Event::default().json_data(payload).unwrap_or_else(|_| {
            Event::default().data(r#"{"type":"error","message":"serialization failed"}"#)
        }))
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerEvent {
    AssistantDelta {
        text: String,
    },
    ToolStarted {
        name: String,
        args: serde_json::Value,
    },
    ToolFinished {
        name: String,
        content: String,
        is_error: bool,
    },
    TurnComplete {
        input_tokens: Option<u32>,
        output_tokens: Option<u32>,
    },
    Error {
        message: String,
    },
}

fn server_event_from_agent(event: &AgentEvent) -> ServerEvent {
    match event {
        AgentEvent::AssistantDelta { text } => ServerEvent::AssistantDelta { text: text.clone() },
        AgentEvent::ToolStarted { name, args } => ServerEvent::ToolStarted {
            name: name.clone(),
            args: args.clone(),
        },
        AgentEvent::ToolFinished { name, result } => ServerEvent::ToolFinished {
            name: name.clone(),
            content: result.content.clone(),
            is_error: result.is_error,
        },
        AgentEvent::TurnComplete { usage } => ServerEvent::TurnComplete {
            input_tokens: usage.map(|u| u.input_tokens),
            output_tokens: usage.map(|u| u.output_tokens),
        },
        AgentEvent::Error { message } => ServerEvent::Error {
            message: message.clone(),
        },
    }
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

fn session_to_summary(service: &SessionService, session: &Session) -> SessionSummary {
    SessionSummary {
        id: session.id.clone(),
        cwd: session.cwd.to_string_lossy().into_owned(),
        title: session.title.clone(),
        message_count: service.message_count(session),
        created_at: session.created_at,
        updated_at: session.updated_at,
    }
}

struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(err: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: err.to_string(),
        }
    }

    fn not_found(resource: &str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: format!("{resource} not found"),
        }
    }

    fn storage(err: codei_session::SessionError) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: err.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({ "error": self.message }));
        (self.status, body).into_response()
    }
}
