use compiler__diagnostics::Diagnostic;

pub struct PhaseResult {
    pub diagnostics: Vec<Diagnostic>,
    pub semantic_analysis_eligibility: SemanticAnalysisEligibility,
}

pub enum SemanticAnalysisEligibility {
    Eligible,
    Ineligible {
        reason: SemanticAnalysisIneligibilityReason,
    },
}

pub enum SemanticAnalysisIneligibilityReason {
    StructuralValidityViolation,
    FileRolePolicyViolation,
}
