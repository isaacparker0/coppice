use compiler__diagnostics::PhaseDiagnostic;
use compiler__phase_results::{PhaseOutput, PhaseStatus};
use compiler__source::Span;
use compiler__syntax::{
    SyntaxBlock, SyntaxBlockItem, SyntaxDeclaration, SyntaxFileItem, SyntaxParsedFile,
    SyntaxStatement, SyntaxStructMemberItem, SyntaxTypeDeclarationKind,
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
pub fn check_file(file: &SyntaxParsedFile) -> PhaseOutput<()> {
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
        safe_autofixes: Vec::new(),
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

fn check_import_order(file: &SyntaxParsedFile, violations: &mut Vec<SyntaxRuleViolation>) {
    let mut saw_non_import_declaration = false;
    for declaration in file.top_level_declarations() {
        match declaration {
            SyntaxDeclaration::Import(import_declaration) => {
                if saw_non_import_declaration {
                    violations.push(SyntaxRuleViolation {
                        kind: SyntaxRuleViolationKind::ImportAfterDeclaration,
                        span: import_declaration.span.clone(),
                    });
                }
            }
            SyntaxDeclaration::Exports(_)
            | SyntaxDeclaration::Type(_)
            | SyntaxDeclaration::Constant(_)
            | SyntaxDeclaration::Function(_)
            | SyntaxDeclaration::Group(_)
            | SyntaxDeclaration::Test(_) => {
                saw_non_import_declaration = true;
            }
        }
    }
}

fn check_doc_comment_placement(file: &SyntaxParsedFile, violations: &mut Vec<SyntaxRuleViolation>) {
    check_file_item_doc_comments(&file.items, violations);
    for declaration in file.top_level_declarations() {
        match declaration {
            SyntaxDeclaration::Type(type_declaration) => {
                let SyntaxTypeDeclarationKind::Struct { items } = &type_declaration.kind else {
                    continue;
                };
                check_struct_member_doc_comments(items, violations);
                for item in items {
                    let SyntaxStructMemberItem::Method(method_declaration) = item else {
                        continue;
                    };
                    check_block_doc_comments(&method_declaration.body, violations);
                }
            }
            SyntaxDeclaration::Function(function_declaration) => {
                check_block_doc_comments(&function_declaration.body, violations);
            }
            SyntaxDeclaration::Group(group_declaration) => {
                for test_declaration in &group_declaration.tests {
                    check_block_doc_comments(&test_declaration.body, violations);
                }
            }
            SyntaxDeclaration::Test(test_declaration) => {
                check_block_doc_comments(&test_declaration.body, violations);
            }
            SyntaxDeclaration::Import(_)
            | SyntaxDeclaration::Exports(_)
            | SyntaxDeclaration::Constant(_) => {}
        }
    }
}

fn check_file_item_doc_comments(
    items: &[SyntaxFileItem],
    violations: &mut Vec<SyntaxRuleViolation>,
) {
    for (index, item) in items.iter().enumerate() {
        let SyntaxFileItem::DocComment(doc_comment) = item else {
            continue;
        };
        let Some(SyntaxFileItem::Declaration(declaration)) = items.get(index + 1) else {
            violations.push(SyntaxRuleViolation {
                kind: SyntaxRuleViolationKind::DocCommentMustDocumentDeclaration,
                span: doc_comment.span.clone(),
            });
            continue;
        };
        let declaration_line = match declaration.as_ref() {
            SyntaxDeclaration::Import(import_declaration) => import_declaration.span.line,
            SyntaxDeclaration::Exports(exports_declaration) => exports_declaration.span.line,
            SyntaxDeclaration::Type(type_declaration) => type_declaration.span.line,
            SyntaxDeclaration::Constant(constant_declaration) => constant_declaration.span.line,
            SyntaxDeclaration::Function(function_declaration) => function_declaration.span.line,
            SyntaxDeclaration::Group(group_declaration) => group_declaration.span.line,
            SyntaxDeclaration::Test(test_declaration) => test_declaration.span.line,
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
    items: &[SyntaxStructMemberItem],
    violations: &mut Vec<SyntaxRuleViolation>,
) {
    for (index, item) in items.iter().enumerate() {
        let SyntaxStructMemberItem::DocComment(doc_comment) = item else {
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
            SyntaxStructMemberItem::Field(field_declaration) => {
                field_declaration.as_ref().span.line
            }
            SyntaxStructMemberItem::Method(method_declaration) => {
                method_declaration.as_ref().span.line
            }
            SyntaxStructMemberItem::DocComment(_) => {
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

fn check_block_doc_comments(block: &SyntaxBlock, violations: &mut Vec<SyntaxRuleViolation>) {
    for item in &block.items {
        match item {
            SyntaxBlockItem::DocComment(doc_comment) => violations.push(SyntaxRuleViolation {
                kind: SyntaxRuleViolationKind::DocCommentMustDocumentDeclaration,
                span: doc_comment.span.clone(),
            }),
            SyntaxBlockItem::Statement(statement) => match statement {
                SyntaxStatement::If {
                    then_block,
                    else_block,
                    ..
                } => {
                    check_block_doc_comments(then_block, violations);
                    if let Some(block) = else_block {
                        check_block_doc_comments(block, violations);
                    }
                }
                SyntaxStatement::For { body, .. } => {
                    check_block_doc_comments(body, violations);
                }
                SyntaxStatement::Binding { .. }
                | SyntaxStatement::Assign { .. }
                | SyntaxStatement::Return { .. }
                | SyntaxStatement::Break { .. }
                | SyntaxStatement::Continue { .. }
                | SyntaxStatement::Expression { .. } => {}
            },
        }
    }
}
