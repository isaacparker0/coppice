use compiler__diagnostics::Diagnostic;
use compiler__syntax::{Declaration, FileItem, ParsedFile, StructMemberItem, TypeDeclarationKind};

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
    check_doc_comment_placement(file, &mut diagnostics);
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

fn check_doc_comment_placement(file: &ParsedFile, diagnostics: &mut Vec<Diagnostic>) {
    check_file_item_doc_comments(&file.items, diagnostics);
    for declaration in &file.declarations {
        let Declaration::Type(type_declaration) = declaration else {
            continue;
        };
        let TypeDeclarationKind::Struct { items } = &type_declaration.kind else {
            continue;
        };
        check_struct_member_doc_comments(items, diagnostics);
    }
}

fn check_file_item_doc_comments(items: &[FileItem], diagnostics: &mut Vec<Diagnostic>) {
    for (index, item) in items.iter().enumerate() {
        let FileItem::DocComment(doc_comment) = item else {
            continue;
        };
        let Some(FileItem::Declaration(declaration)) = items.get(index + 1) else {
            diagnostics.push(Diagnostic::new(
                "doc comment must document a declaration",
                doc_comment.span.clone(),
            ));
            continue;
        };
        let declaration_line = match declaration.as_ref() {
            Declaration::Import(import_declaration) => import_declaration.span.line,
            Declaration::Exports(exports_declaration) => exports_declaration.span.line,
            Declaration::Type(type_declaration) => type_declaration.span.line,
            Declaration::Constant(constant_declaration) => constant_declaration.span.line,
            Declaration::Function(function_declaration) => function_declaration.span.line,
        };
        if declaration_line != doc_comment.end_line + 1 {
            diagnostics.push(Diagnostic::new(
                "doc comment must document a declaration",
                doc_comment.span.clone(),
            ));
        }
    }
}

fn check_struct_member_doc_comments(items: &[StructMemberItem], diagnostics: &mut Vec<Diagnostic>) {
    for (index, item) in items.iter().enumerate() {
        let StructMemberItem::DocComment(doc_comment) = item else {
            continue;
        };
        let Some(next_item) = items.get(index + 1) else {
            diagnostics.push(Diagnostic::new(
                "doc comment must document a declaration",
                doc_comment.span.clone(),
            ));
            continue;
        };
        let declaration_line = match next_item {
            StructMemberItem::Field(field_declaration) => field_declaration.as_ref().span.line,
            StructMemberItem::Method(method_declaration) => method_declaration.as_ref().span.line,
            StructMemberItem::DocComment(_) => {
                diagnostics.push(Diagnostic::new(
                    "doc comment must document a declaration",
                    doc_comment.span.clone(),
                ));
                continue;
            }
        };
        if declaration_line != doc_comment.end_line + 1 {
            diagnostics.push(Diagnostic::new(
                "doc comment must document a declaration",
                doc_comment.span.clone(),
            ));
        }
    }
}
