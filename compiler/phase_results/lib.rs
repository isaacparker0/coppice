use compiler__diagnostics::Diagnostic;

pub struct PhaseResult {
    pub diagnostics: Vec<Diagnostic>,
    pub status: PhaseStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhaseStatus {
    Ok,
    PreventsDownstreamExecution,
}
