use compiler__diagnostics::PhaseDiagnostic;
use compiler__phase_results::{PhaseOutput, PhaseStatus};
use compiler__source::Span;
use compiler__syntax::{
    Block, BlockItem, Declaration, FileItem, ParsedFile, Statement, StructMemberItem,
    TypeDeclarationKind,
};

#[derive(Clone, Copy)]
enum SyntaxRuleViolationKind {
    ImportAfterDeclaration,
    DocCommentMustDocumentDeclaration,
}

struct SyntaxRuleViolation {
    kind: SyntaxRuleViolationKind,
    span: Span,
}

#[must_use]
pub fn check_file(file: &ParsedFile) -> PhaseOutput<()> {
    let mut violations = Vec::new();
    check_import_order(file, &mut violations);
    check_doc_comment_placement(file, &mut violations);
    let diagnostics = render_diagnostics(&violations);
    let status = if diagnostics.is_empty() {
        PhaseStatus::Ok
    } else {
        PhaseStatus::PreventsDownstreamExecution
    };

    PhaseOutput {
        value: (),
        diagnostics,
        status,
    }
}

fn render_diagnostics(violations: &[SyntaxRuleViolation]) -> Vec<PhaseDiagnostic> {
    violations
        .iter()
        .map(|violation| {
            let message = match violation.kind {
                SyntaxRuleViolationKind::ImportAfterDeclaration => {
                    "import declarations must appear before top-level declarations"
                }
                SyntaxRuleViolationKind::DocCommentMustDocumentDeclaration => {
                    "doc comment must document a declaration"
                }
            };
            PhaseDiagnostic::new(message, violation.span.clone())
        })
        .collect()
}

fn check_import_order(file: &ParsedFile, violations: &mut Vec<SyntaxRuleViolation>) {
    let mut saw_non_import_declaration = false;
    for declaration in file.top_level_declarations() {
        match declaration {
            Declaration::Import(import_declaration) => {
                if saw_non_import_declaration {
                    violations.push(SyntaxRuleViolation {
                        kind: SyntaxRuleViolationKind::ImportAfterDeclaration,
                        span: import_declaration.span.clone(),
                    });
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

fn check_doc_comment_placement(file: &ParsedFile, violations: &mut Vec<SyntaxRuleViolation>) {
    check_file_item_doc_comments(&file.items, violations);
    for declaration in file.top_level_declarations() {
        match declaration {
            Declaration::Type(type_declaration) => {
                let TypeDeclarationKind::Struct { items } = &type_declaration.kind else {
                    continue;
                };
                check_struct_member_doc_comments(items, violations);
                for item in items {
                    let StructMemberItem::Method(method_declaration) = item else {
                        continue;
                    };
                    check_block_doc_comments(&method_declaration.body, violations);
                }
            }
            Declaration::Function(function_declaration) => {
                check_block_doc_comments(&function_declaration.body, violations);
            }
            Declaration::Import(_) | Declaration::Exports(_) | Declaration::Constant(_) => {}
        }
    }
}

fn check_file_item_doc_comments(items: &[FileItem], violations: &mut Vec<SyntaxRuleViolation>) {
    for (index, item) in items.iter().enumerate() {
        let FileItem::DocComment(doc_comment) = item else {
            continue;
        };
        let Some(FileItem::Declaration(declaration)) = items.get(index + 1) else {
            violations.push(SyntaxRuleViolation {
                kind: SyntaxRuleViolationKind::DocCommentMustDocumentDeclaration,
                span: doc_comment.span.clone(),
            });
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
            violations.push(SyntaxRuleViolation {
                kind: SyntaxRuleViolationKind::DocCommentMustDocumentDeclaration,
                span: doc_comment.span.clone(),
            });
        }
    }
}

fn check_struct_member_doc_comments(
    items: &[StructMemberItem],
    violations: &mut Vec<SyntaxRuleViolation>,
) {
    for (index, item) in items.iter().enumerate() {
        let StructMemberItem::DocComment(doc_comment) = item else {
            continue;
        };
        let Some(next_item) = items.get(index + 1) else {
            violations.push(SyntaxRuleViolation {
                kind: SyntaxRuleViolationKind::DocCommentMustDocumentDeclaration,
                span: doc_comment.span.clone(),
            });
            continue;
        };
        let declaration_line = match next_item {
            StructMemberItem::Field(field_declaration) => field_declaration.as_ref().span.line,
            StructMemberItem::Method(method_declaration) => method_declaration.as_ref().span.line,
            StructMemberItem::DocComment(_) => {
                violations.push(SyntaxRuleViolation {
                    kind: SyntaxRuleViolationKind::DocCommentMustDocumentDeclaration,
                    span: doc_comment.span.clone(),
                });
                continue;
            }
        };
        if declaration_line != doc_comment.end_line + 1 {
            violations.push(SyntaxRuleViolation {
                kind: SyntaxRuleViolationKind::DocCommentMustDocumentDeclaration,
                span: doc_comment.span.clone(),
            });
        }
    }
}

fn check_block_doc_comments(block: &Block, violations: &mut Vec<SyntaxRuleViolation>) {
    for item in &block.items {
        match item {
            BlockItem::DocComment(doc_comment) => violations.push(SyntaxRuleViolation {
                kind: SyntaxRuleViolationKind::DocCommentMustDocumentDeclaration,
                span: doc_comment.span.clone(),
            }),
            BlockItem::Statement(statement) => match statement {
                Statement::If {
                    then_block,
                    else_block,
                    ..
                } => {
                    check_block_doc_comments(then_block, violations);
                    if let Some(block) = else_block {
                        check_block_doc_comments(block, violations);
                    }
                }
                Statement::For { body, .. } => {
                    check_block_doc_comments(body, violations);
                }
                Statement::Binding { .. }
                | Statement::Assign { .. }
                | Statement::Return { .. }
                | Statement::Abort { .. }
                | Statement::Break { .. }
                | Statement::Continue { .. }
                | Statement::Expression { .. } => {}
            },
        }
    }
}
