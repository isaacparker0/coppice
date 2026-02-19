use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path as RoutePath, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use tokio::fs;

use crate::compiler_adapter::{
    build_workspace_binary, check_workspace, run_binary, write_workspace_source,
};
use crate::models::{
    CheckRequest, CheckResponse, DiagnosticResponse, ErrorResponse, HealthResponse, RunRequest,
    RunResponse, SessionResponse, failure_response,
};
use crate::session_store::SessionStore;

const MAX_SOURCE_BYTES: usize = 128 * 1024;
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const BASIC_AUTH_HEADER_VALUE: &str = "Basic cGxheWdyb3VuZDpiYXplbC1pcy1jb29s";
const BASIC_AUTH_CHALLENGE: &str = "Basic realm=\"coppice-playground\"";

#[derive(Clone)]
pub struct AppState {
    session_store: Arc<SessionStore>,
    web_root: PathBuf,
}

pub fn build_router(session_store: Arc<SessionStore>, web_root: PathBuf) -> Router {
    let app_state = AppState {
        session_store,
        web_root,
    };

    Router::new()
        .route("/", get(serve_index))
        .route("/health", get(health))
        .route("/session", post(create_session))
        .route("/check", post(check))
        .route("/run", post(run))
        .route("/{*path}", get(serve_asset))
        .layer(middleware::from_fn(require_basic_auth))
        .layer(axum::extract::DefaultBodyLimit::max(
            MAX_SOURCE_BYTES + 1024,
        ))
        .with_state(app_state)
}

async fn require_basic_auth(request: Request, next: Next) -> Response {
    if request.uri().path() == "/health" {
        return next.run(request).await;
    }

    let is_authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == BASIC_AUTH_HEADER_VALUE);
    if is_authorized {
        return next.run(request).await;
    }

    let mut response = Response::new(Body::from("unauthorized"));
    *response.status_mut() = StatusCode::UNAUTHORIZED;
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static(BASIC_AUTH_CHALLENGE),
    );
    response
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { ok: true })
}

async fn create_session(State(state): State<AppState>) -> Json<SessionResponse> {
    let session_id = state.session_store.create_session();
    Json(SessionResponse { session_id })
}

async fn serve_index(State(state): State<AppState>) -> Response {
    let path = state.web_root.join("index.html");
    serve_file(path).await
}

async fn serve_asset(
    State(state): State<AppState>,
    RoutePath(path): RoutePath<String>,
) -> Response {
    let Some(safe_path) = safe_join_web_path(&state.web_root, &path) else {
        return not_found_response();
    };
    serve_file(safe_path).await
}

fn safe_join_web_path(web_root: &Path, path: &str) -> Option<PathBuf> {
    let relative = Path::new(path);
    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(web_root.join(relative))
}

async fn serve_file(path: PathBuf) -> Response {
    let Ok(bytes) = fs::read(&path).await else {
        return not_found_response();
    };

    let content_type = content_type_for_path(&path);
    let mut response = Response::new(Body::from(bytes));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
}

fn content_type_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

fn not_found_response() -> Response {
    let mut response = Response::new(Body::from("not found"));
    *response.status_mut() = StatusCode::NOT_FOUND;
    response
}

async fn check(
    State(state): State<AppState>,
    Json(request): Json<CheckRequest>,
) -> (StatusCode, Json<CheckResponse>) {
    if request.source.len() > MAX_SOURCE_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(CheckResponse {
                ok: false,
                diagnostics: Vec::new(),
                error: Some(ErrorResponse {
                    kind: "payload_too_large".to_string(),
                    message: "source exceeds maximum payload size".to_string(),
                    details: vec![format!("max bytes: {MAX_SOURCE_BYTES}")],
                }),
            }),
        );
    }

    let Some(session_directory) = state.session_store.session_directory(&request.session_id) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(CheckResponse {
                ok: false,
                diagnostics: Vec::new(),
                error: Some(ErrorResponse {
                    kind: "invalid_session".to_string(),
                    message: "session not found".to_string(),
                    details: Vec::new(),
                }),
            }),
        );
    };

    let source = request.source;
    let result = tokio::task::spawn_blocking(move || {
        write_workspace_source(&session_directory, &source)?;
        check_workspace(&session_directory)
    })
    .await;

    let checked_target = match result {
        Ok(Ok(checked_target)) => checked_target,
        Ok(Err(error)) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(CheckResponse {
                    ok: false,
                    diagnostics: Vec::new(),
                    error: Some(failure_response(&error)),
                }),
            );
        }
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(CheckResponse {
                    ok: false,
                    diagnostics: Vec::new(),
                    error: Some(ErrorResponse {
                        kind: "internal".to_string(),
                        message: "check worker panicked".to_string(),
                        details: vec![error.to_string()],
                    }),
                }),
            );
        }
    };

    let diagnostics = checked_target
        .diagnostics
        .iter()
        .map(DiagnosticResponse::from_rendered)
        .collect::<Vec<_>>();
    (
        StatusCode::OK,
        Json(CheckResponse {
            ok: diagnostics.is_empty(),
            diagnostics,
            error: None,
        }),
    )
}

async fn run(
    State(state): State<AppState>,
    Json(request): Json<RunRequest>,
) -> (StatusCode, Json<RunResponse>) {
    let Some(session_directory) = state.session_store.session_directory(&request.session_id) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(RunResponse {
                ok: false,
                exit_code: 1,
                stdout: String::new(),
                stderr: String::new(),
                diagnostics: Vec::new(),
                timed_out: false,
                error: Some(ErrorResponse {
                    kind: "invalid_session".to_string(),
                    message: "session not found".to_string(),
                    details: Vec::new(),
                }),
            }),
        );
    };

    let source = request.source;
    let build_result = tokio::task::spawn_blocking(move || {
        write_workspace_source(&session_directory, &source)?;

        let checked_target = check_workspace(&session_directory)?;
        if !checked_target.diagnostics.is_empty() {
            return Ok((checked_target, None));
        }

        let binary_path = build_workspace_binary(&session_directory)?;
        Ok((checked_target, Some(binary_path)))
    })
    .await;

    let (checked_target, binary_path) = match build_result {
        Ok(Ok((checked_target, binary_path))) => (checked_target, binary_path),
        Ok(Err(error)) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(RunResponse {
                    ok: false,
                    exit_code: 1,
                    stdout: String::new(),
                    stderr: String::new(),
                    diagnostics: Vec::new(),
                    timed_out: false,
                    error: Some(failure_response(&error)),
                }),
            );
        }
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(RunResponse {
                    ok: false,
                    exit_code: 1,
                    stdout: String::new(),
                    stderr: String::new(),
                    diagnostics: Vec::new(),
                    timed_out: false,
                    error: Some(ErrorResponse {
                        kind: "internal".to_string(),
                        message: "run worker panicked".to_string(),
                        details: vec![error.to_string()],
                    }),
                }),
            );
        }
    };

    let diagnostics = checked_target
        .diagnostics
        .iter()
        .map(DiagnosticResponse::from_rendered)
        .collect::<Vec<_>>();

    if !diagnostics.is_empty() {
        return (
            StatusCode::OK,
            Json(RunResponse {
                ok: false,
                exit_code: 1,
                stdout: String::new(),
                stderr: String::new(),
                diagnostics,
                timed_out: false,
                error: None,
            }),
        );
    }

    let Some(binary_path) = binary_path else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RunResponse {
                ok: false,
                exit_code: 1,
                stdout: String::new(),
                stderr: String::new(),
                diagnostics: Vec::new(),
                timed_out: false,
                error: Some(ErrorResponse {
                    kind: "internal".to_string(),
                    message: "missing built binary for run request".to_string(),
                    details: Vec::new(),
                }),
            }),
        );
    };

    match run_binary(&binary_path, Duration::from_secs(5), MAX_OUTPUT_BYTES).await {
        Ok(execution) => (
            StatusCode::OK,
            Json(RunResponse {
                ok: execution.exit_code == 0 && !execution.timed_out,
                exit_code: execution.exit_code,
                stdout: execution.stdout,
                stderr: execution.stderr,
                diagnostics: Vec::new(),
                timed_out: execution.timed_out,
                error: None,
            }),
        ),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(RunResponse {
                ok: false,
                exit_code: 1,
                stdout: String::new(),
                stderr: String::new(),
                diagnostics: Vec::new(),
                timed_out: false,
                error: Some(failure_response(&error)),
            }),
        ),
    }
}
