use compiler__diagnostics::Diagnostic;

pub struct PhaseResult {
    pub diagnostics: Vec<Diagnostic>,
    pub status: PhaseStatus,
}

pub enum PhaseStatus {
    Ok,
    PreventsDownstreamExecution,
}
