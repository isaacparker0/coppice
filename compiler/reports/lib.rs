use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use compiler__source::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportFormat {
    Text,
    Json,
}

impl ReportFormat {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Json => "json",
        }
    }
}

impl fmt::Display for ReportFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ReportFormat {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            _ => Err(format!("invalid report format '{value}'")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticPhase {
    Parsing,
    SyntaxRules,
    FileRoleRules,
    Resolution,
    SemanticLowering,
    TypeAnalysis,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderedDiagnostic {
    pub phase: DiagnosticPhase,
    pub path: String,
    pub message: String,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompilerFailure {
    pub kind: CompilerFailureKind,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<CompilerFailureDetail>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompilerFailureKind {
    CheckFailed,
    ReadSource,
    WriteSource,
    InvalidWorkspaceRoot,
    WorkspaceRootNotDirectory,
    WorkspaceRootMissingManifest,
    InvalidCheckTarget,
    TargetOutsideWorkspace,
    PackageNotFound,
    WorkspaceDiscoveryFailed,
    BuildFailed,
    RunFailed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompilerFailureDetail {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompilerCheckJsonOutput {
    pub ok: bool,
    pub diagnostics: Vec<RenderedDiagnostic>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub safe_fixes: Vec<CompilerCheckSafeFix>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<CompilerFailure>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompilerCheckSafeFix {
    pub path: String,
    pub edit_count: usize,
}
