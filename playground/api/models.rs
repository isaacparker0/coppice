use compiler__reports::{CompilerFailure, RenderedDiagnostic};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct CheckRequest {
    pub session_id: String,
    pub source: String,
}

#[derive(Debug, Deserialize)]
pub struct RunRequest {
    pub session_id: String,
    pub source: String,
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
pub struct ErrorResponse {
    pub kind: String,
    pub message: String,
    pub details: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct DiagnosticResponse {
    pub phase: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct CheckResponse {
    pub ok: bool,
    pub diagnostics: Vec<DiagnosticResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorResponse>,
}

#[derive(Debug, Serialize)]
pub struct RunResponse {
    pub ok: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub diagnostics: Vec<DiagnosticResponse>,
    pub timed_out: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorResponse>,
}

impl DiagnosticResponse {
    #[must_use]
    pub fn from_rendered(diagnostic: &RenderedDiagnostic) -> Self {
        Self {
            phase: format!("{:?}", diagnostic.phase),
            path: diagnostic.path.clone(),
            line: diagnostic.span.line,
            column: diagnostic.span.column,
            message: diagnostic.message.clone(),
        }
    }
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
