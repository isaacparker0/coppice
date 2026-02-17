use serde::Serialize;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticPhase {
    Parsing,
    SyntaxRules,
    FileRoleRules,
    Resolution,
    SemanticLowering,
    TypeAnalysis,
}

#[derive(Clone, Debug, Serialize)]
pub struct RenderedDiagnostic {
    pub phase: DiagnosticPhase,
    pub path: String,
    pub message: String,
    pub span: Span,
}

#[derive(Clone, Debug, Serialize)]
pub struct CompilerFailure {
    pub kind: CompilerFailureKind,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<CompilerFailureDetail>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompilerFailureKind {
    ReadSource,
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

#[derive(Clone, Debug, Serialize)]
pub struct CompilerFailureDetail {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}
