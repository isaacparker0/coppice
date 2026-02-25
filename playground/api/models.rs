use compiler__reports::{CompilerFailure, RenderedDiagnostic};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct WorkspaceFileRequest {
    pub path: String,
    pub source: String,
}

#[derive(Debug, Deserialize)]
pub struct CheckRequest {
    pub session_id: String,
    pub entrypoint_path: String,
    pub files: Vec<WorkspaceFileRequest>,
}

#[derive(Debug, Deserialize)]
pub struct RunRequest {
    pub session_id: String,
    pub entrypoint_path: String,
    pub files: Vec<WorkspaceFileRequest>,
}

#[derive(Debug, Serialize)]
pub struct SessionResponse {
    pub session_id: String,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub ok: bool,
}

#[derive(Debug, Serialize)]
pub struct ExampleSummaryResponse {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct ExamplesListResponse {
    pub examples: Vec<ExampleSummaryResponse>,
}

#[derive(Debug, Serialize)]
pub struct ExampleWorkspaceResponse {
    pub id: String,
    pub name: String,
    pub entrypoint_path: String,
    pub files: Vec<WorkspaceFileRequest>,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub kind: String,
    pub message: String,
    pub details: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CheckResponse {
    pub ok: bool,
    pub diagnostics: Vec<RenderedDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorResponse>,
}

#[derive(Debug, Serialize)]
pub struct RunResponse {
    pub ok: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub diagnostics: Vec<RenderedDiagnostic>,
    pub timed_out: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorResponse>,
}

#[must_use]
pub fn failure_response(error: &CompilerFailure) -> ErrorResponse {
    ErrorResponse {
        kind: format!("{:?}", error.kind),
        message: error.message.clone(),
        details: error
            .details
            .iter()
            .map(|detail| detail.message.clone())
            .collect(),
    }
}
