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
    check_workspace_via_cli, run_workspace_via_cli, write_workspace_source,
};
use crate::models::{
    CheckRequest, CheckResponse, ErrorResponse, HealthResponse, RunRequest, RunResponse,
    SessionResponse, failure_response,
};
use crate::path_sanitizer::sanitize_workspace_path;
use crate::session_store::SessionStore;
use compiler__reports::{CompilerCheckJsonOutput, CompilerFailure};

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
        Ok::<PathBuf, CompilerFailure>(session_directory)
    })
    .await;

    let session_directory = match result {
        Ok(Ok(session_directory)) => session_directory,
        Ok(Err(error)) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(CheckResponse {
                    ok: false,
                    diagnostics: Vec::new(),
                    error: Some(ErrorResponse {
                        kind: format!("{:?}", error.kind),
                        message: error.message,
                        details: error
                            .details
                            .into_iter()
                            .map(|detail| detail.message)
                            .collect(),
                    }),
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

    match check_workspace_via_cli(&session_directory, Duration::from_secs(5), MAX_OUTPUT_BYTES)
        .await
    {
        Ok(execution) => {
            if execution.timed_out {
                return (
                    StatusCode::OK,
                    Json(CheckResponse {
                        ok: false,
                        diagnostics: Vec::new(),
                        error: Some(ErrorResponse {
                            kind: "check_timeout".to_string(),
                            message: execution.stderr,
                            details: Vec::new(),
                        }),
                    }),
                );
            }

            if execution.exit_code != 0 {
                if let Ok(mut cli_output) =
                    serde_json::from_str::<CompilerCheckJsonOutput>(&execution.stdout)
                {
                    for diagnostic in &mut cli_output.diagnostics {
                        diagnostic.path =
                            sanitize_workspace_path(&diagnostic.path, &session_directory);
                    }
                    let error = cli_output.error.as_ref().map(failure_response);

                    return (
                        StatusCode::OK,
                        Json(CheckResponse {
                            ok: false,
                            diagnostics: cli_output.diagnostics,
                            error,
                        }),
                    );
                }

                let message = if execution.stderr.is_empty() {
                    "check failed".to_string()
                } else {
                    execution.stderr
                };
                return (
                    StatusCode::OK,
                    Json(CheckResponse {
                        ok: false,
                        diagnostics: Vec::new(),
                        error: Some(ErrorResponse {
                            kind: "check_failed".to_string(),
                            message,
                            details: Vec::new(),
                        }),
                    }),
                );
            }

            if let Ok(mut cli_output) =
                serde_json::from_str::<CompilerCheckJsonOutput>(&execution.stdout)
            {
                for diagnostic in &mut cli_output.diagnostics {
                    diagnostic.path = sanitize_workspace_path(&diagnostic.path, &session_directory);
                }
                return (
                    StatusCode::OK,
                    Json(CheckResponse {
                        ok: cli_output.ok,
                        diagnostics: cli_output.diagnostics,
                        error: None,
                    }),
                );
            }

            (
                StatusCode::OK,
                Json(CheckResponse {
                    ok: true,
                    diagnostics: Vec::new(),
                    error: None,
                }),
            )
        }
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(CheckResponse {
                ok: false,
                diagnostics: Vec::new(),
                error: Some(ErrorResponse {
                    kind: format!("{:?}", error.kind),
                    message: error.message,
                    details: error
                        .details
                        .into_iter()
                        .map(|detail| detail.message)
                        .collect(),
                }),
            }),
        ),
    }
}

async fn run(
    State(state): State<AppState>,
    Json(request): Json<RunRequest>,
) -> (StatusCode, Json<RunResponse>) {
    if request.source.len() > MAX_SOURCE_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(RunResponse {
                ok: false,
                exit_code: 1,
                stdout: String::new(),
                stderr: String::new(),
                diagnostics: Vec::new(),
                timed_out: false,
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
    let result = tokio::task::spawn_blocking(move || {
        write_workspace_source(&session_directory, &source)?;
        Ok::<PathBuf, CompilerFailure>(session_directory)
    })
    .await;

    let session_directory = match result {
        Ok(Ok(session_directory)) => session_directory,
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
                    error: Some(ErrorResponse {
                        kind: format!("{:?}", error.kind),
                        message: error.message,
                        details: error
                            .details
                            .into_iter()
                            .map(|detail| detail.message)
                            .collect(),
                    }),
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

    match run_workspace_via_cli(&session_directory, Duration::from_secs(5), MAX_OUTPUT_BYTES).await
    {
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
                error: Some(ErrorResponse {
                    kind: format!("{:?}", error.kind),
                    message: error.message,
                    details: error
                        .details
                        .into_iter()
                        .map(|detail| detail.message)
                        .collect(),
                }),
            }),
        ),
    }
}
