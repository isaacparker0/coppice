use compiler__diagnostics::Diagnostic;

pub struct PhaseOutput<T> {
    pub value: T,
    pub diagnostics: Vec<Diagnostic>,
    pub status: PhaseStatus,
}

pub struct PhaseResult {
    pub diagnostics: Vec<Diagnostic>,
    pub status: PhaseStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhaseStatus {
    Ok,
    PreventsDownstreamExecution,
}
