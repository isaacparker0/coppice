use std::path::PathBuf;

use compiler__fix_edits::TextEdit;
use compiler__source::Span;

#[derive(Clone, Debug)]
pub struct PhaseDiagnostic {
    pub message: String,
    pub span: Span,
}

impl PhaseDiagnostic {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SafeAutofix {
    pub text_edits: Vec<TextEdit>,
}

impl SafeAutofix {
    #[must_use]
    pub fn from_text_edit(text_edit: TextEdit) -> Self {
        Self {
            text_edits: vec![text_edit],
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
