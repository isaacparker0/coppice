use compiler__diagnostics::Diagnostic;
use compiler__syntax::{Declaration, ParsedFile};

pub struct SyntaxRulesResult {
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
}

#[must_use]
pub fn check_file(file: &ParsedFile) -> SyntaxRulesResult {
    let mut diagnostics = Vec::new();
    check_import_order(file, &mut diagnostics);
    let semantic_analysis_eligibility = if diagnostics.is_empty() {
        SemanticAnalysisEligibility::Eligible
    } else {
        SemanticAnalysisEligibility::Ineligible {
            reason: SemanticAnalysisIneligibilityReason::StructuralValidityViolation,
        }
    };

    SyntaxRulesResult {
        diagnostics,
        semantic_analysis_eligibility,
    }
}

fn check_import_order(file: &ParsedFile, diagnostics: &mut Vec<Diagnostic>) {
    let mut saw_non_import_declaration = false;
    for declaration in &file.declarations {
        match declaration {
            Declaration::Import(import_declaration) => {
                if saw_non_import_declaration {
                    diagnostics.push(Diagnostic::new(
                        "import declarations must appear before top-level declarations",
                        import_declaration.span.clone(),
                    ));
                }
            }
            Declaration::Exports(_)
            | Declaration::Type(_)
            | Declaration::Constant(_)
            | Declaration::Function(_) => {
                saw_non_import_declaration = true;
            }
        }
    }
}
