use std::path::PathBuf;

use compiler__source::Span;

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub message: String,
    pub span: Span,
}

impl Diagnostic {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }
}

pub struct FileScopedDiagnostic {
    pub path: PathBuf,
    pub message: String,
    pub span: Span,
}

impl FileScopedDiagnostic {
    pub fn new(path: PathBuf, message: impl Into<String>, span: Span) -> Self {
        Self {
            path,
            message: message.into(),
            span,
        }
    }
}
