use std::collections::BTreeMap;
use std::path::PathBuf;

use compiler__diagnostics::{FileScopedDiagnostic, PhaseDiagnostic};

pub struct PhaseOutput<T> {
    pub value: T,
    pub diagnostics: Vec<PhaseDiagnostic>,
    pub status: PhaseStatus,
}

pub struct FileScopedPhaseOutput<T> {
    pub value: T,
    pub diagnostics: Vec<FileScopedDiagnostic>,
    pub status_by_file: BTreeMap<PathBuf, PhaseStatus>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhaseStatus {
    Ok,
    PreventsDownstreamExecution,
}
