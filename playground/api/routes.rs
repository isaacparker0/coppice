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
    check_workspace_via_cli, run_workspace_via_cli, write_workspace_files,
};
use crate::models::{
    CheckRequest, CheckResponse, ErrorResponse, ExampleSummaryResponse, ExampleWorkspaceResponse,
    ExamplesListResponse, HealthResponse, RunRequest, RunResponse, SessionResponse,
    WorkspaceFileRequest, failure_response,
};
use crate::path_sanitizer::sanitize_workspace_path;
use crate::session_store::SessionStore;
use compiler__reports::{CompilerAnalysisJsonOutput, CompilerFailure};

const MAX_WORKSPACE_BYTES: usize = 512 * 1024;
const MAX_WORKSPACE_FILES: usize = 128;
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const BASIC_AUTH_HEADER_VALUE: &str = "Basic cGxheWdyb3VuZDpiYXplbC1pcy1jb29s";
const BASIC_AUTH_CHALLENGE: &str = "Basic realm=\"coppice-playground\"";

#[derive(Clone)]
pub struct AppState {
    session_store: Arc<SessionStore>,
    web_root: PathBuf,
    examples_root: PathBuf,
}

pub fn build_router(
    session_store: Arc<SessionStore>,
    web_root: PathBuf,
    examples_root: PathBuf,
) -> Router {
    let app_state = AppState {
        session_store,
        web_root,
        examples_root,
    };

    Router::new()
        .route("/", get(serve_index))
        .route("/health", get(health))
        .route("/examples", get(list_examples))
        .route("/examples/{example_id}", get(load_example))
        .route("/session", post(create_session))
        .route("/check", post(check))
        .route("/run", post(run))
        .route("/{*path}", get(serve_asset))
        .layer(middleware::from_fn(require_basic_auth))
        .layer(axum::extract::DefaultBodyLimit::max(
            MAX_WORKSPACE_BYTES + 16 * 1024,
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

async fn list_examples(State(state): State<AppState>) -> (StatusCode, Json<ExamplesListResponse>) {
    let examples = read_example_summaries(&state.examples_root).unwrap_or_default();
    (StatusCode::OK, Json(ExamplesListResponse { examples }))
}

async fn load_example(
    State(state): State<AppState>,
    RoutePath(example_id): RoutePath<String>,
) -> (StatusCode, Json<ExampleWorkspaceResponse>) {
    let Some(example_workspace) = read_example_workspace(&state.examples_root, &example_id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(ExampleWorkspaceResponse {
                id: example_id,
                name: "not found".to_string(),
                entrypoint_path: "main.bin.copp".to_string(),
                files: Vec::new(),
            }),
        );
    };
    (StatusCode::OK, Json(example_workspace))
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

fn read_example_summaries(examples_root: &Path) -> std::io::Result<Vec<ExampleSummaryResponse>> {
    let mut examples = Vec::new();
    for entry_result in std::fs::read_dir(examples_root)? {
        let entry = entry_result?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(example_id) = entry.file_name().to_str().map(ToString::to_string) else {
            continue;
        };
        if !is_valid_example_id(&example_id) {
            continue;
        }
        examples.push(ExampleSummaryResponse {
            name: example_name_for_id(&example_id),
            id: example_id,
        });
    }
    examples.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(examples)
}

fn read_example_workspace(
    examples_root: &Path,
    example_id: &str,
) -> Option<ExampleWorkspaceResponse> {
    if !is_valid_example_id(example_id) {
        return None;
    }

    let example_directory = examples_root.join(example_id);
    if !example_directory.is_dir() {
        return None;
    }

    let mut files = collect_workspace_files(&example_directory, &example_directory).ok()?;
    files.sort_by(|left, right| left.path.cmp(&right.path));

    let entrypoint_path = files
        .iter()
        .find(|file| file.path == "main.bin.copp")
        .or_else(|| files.iter().find(|file| file.path.ends_with(".bin.copp")))
        .map(|file| file.path.clone())?;

    Some(ExampleWorkspaceResponse {
        id: example_id.to_string(),
        name: example_name_for_id(example_id),
        entrypoint_path,
        files,
    })
}

fn collect_workspace_files(
    root_directory: &Path,
    current_directory: &Path,
) -> std::io::Result<Vec<WorkspaceFileRequest>> {
    let mut workspace_files = Vec::new();
    for entry_result in std::fs::read_dir(current_directory)? {
        let entry = entry_result?;
        let entry_path = entry.path();
        if entry.file_type()?.is_dir() {
            let child_files = collect_workspace_files(root_directory, &entry_path)?;
            workspace_files.extend(child_files);
            continue;
        }

        let Ok(relative_path) = entry_path.strip_prefix(root_directory) else {
            continue;
        };
        let Some(relative_path_string) = relative_path.to_str() else {
            continue;
        };
        let source = std::fs::read_to_string(&entry_path)?;
        workspace_files.push(WorkspaceFileRequest {
            path: relative_path_string.replace('\\', "/"),
            source,
        });
    }
    Ok(workspace_files)
}

fn is_valid_example_id(example_id: &str) -> bool {
    !example_id.is_empty()
        && example_id.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
}

fn example_name_for_id(example_id: &str) -> String {
    example_id.replace('_', " ")
}

async fn check(
    State(state): State<AppState>,
    Json(request): Json<CheckRequest>,
) -> (StatusCode, Json<CheckResponse>) {
    if let Some(error) = workspace_request_error(&request.entrypoint_path, &request.files) {
        return (
            StatusCode::BAD_REQUEST,
            Json(CheckResponse {
                ok: false,
                diagnostics: Vec::new(),
                error: Some(error),
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

    let entrypoint_path = request.entrypoint_path;
    let files = request.files;
    let result = tokio::task::spawn_blocking(move || {
        write_workspace_files(&session_directory, &files)?;
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

    match check_workspace_via_cli(
        &session_directory,
        &entrypoint_path,
        Duration::from_secs(5),
        MAX_OUTPUT_BYTES,
    )
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
                    serde_json::from_str::<CompilerAnalysisJsonOutput>(&execution.stdout)
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
                serde_json::from_str::<CompilerAnalysisJsonOutput>(&execution.stdout)
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
    if let Some(error) = workspace_request_error(&request.entrypoint_path, &request.files) {
        return (
            StatusCode::BAD_REQUEST,
            Json(RunResponse {
                ok: false,
                exit_code: 1,
                stdout: String::new(),
                stderr: String::new(),
                diagnostics: Vec::new(),
                timed_out: false,
                error: Some(error),
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

    let entrypoint_path = request.entrypoint_path;
    let files = request.files;
    let result = tokio::task::spawn_blocking(move || {
        write_workspace_files(&session_directory, &files)?;
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

    match run_workspace_via_cli(
        &session_directory,
        &entrypoint_path,
        Duration::from_secs(5),
        MAX_OUTPUT_BYTES,
    )
    .await
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

fn workspace_request_error(
    entrypoint_path: &str,
    files: &[WorkspaceFileRequest],
) -> Option<ErrorResponse> {
    if files.is_empty() {
        return Some(ErrorResponse {
            kind: "invalid_workspace".to_string(),
            message: "workspace must include at least one file".to_string(),
            details: Vec::new(),
        });
    }

    if files.len() > MAX_WORKSPACE_FILES {
        return Some(ErrorResponse {
            kind: "invalid_workspace".to_string(),
            message: "workspace has too many files".to_string(),
            details: vec![format!("max files: {MAX_WORKSPACE_FILES}")],
        });
    }

    let total_source_bytes: usize = files.iter().map(|file| file.source.len()).sum();
    if total_source_bytes > MAX_WORKSPACE_BYTES {
        return Some(ErrorResponse {
            kind: "payload_too_large".to_string(),
            message: "workspace source exceeds maximum payload size".to_string(),
            details: vec![format!("max bytes: {MAX_WORKSPACE_BYTES}")],
        });
    }

    if !entrypoint_path.ends_with(".bin.copp") {
        return Some(ErrorResponse {
            kind: "invalid_workspace".to_string(),
            message: "entrypoint must be a .bin.copp file".to_string(),
            details: Vec::new(),
        });
    }

    let has_manifest = files.iter().any(|file| file.path == "PACKAGE.copp");
    if !has_manifest {
        return Some(ErrorResponse {
            kind: "invalid_workspace".to_string(),
            message: "workspace root must include PACKAGE.copp".to_string(),
            details: Vec::new(),
        });
    }

    let has_entrypoint = files.iter().any(|file| file.path == entrypoint_path);
    if !has_entrypoint {
        return Some(ErrorResponse {
            kind: "invalid_workspace".to_string(),
            message: "entrypoint file not found in workspace".to_string(),
            details: vec![entrypoint_path.to_string()],
        });
    }

    None
}
