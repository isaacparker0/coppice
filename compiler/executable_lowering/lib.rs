use compiler__diagnostics::PhaseDiagnostic;
use compiler__executable_program::{ExecutableExpression, ExecutableProgram, ExecutableStatement};
use compiler__phase_results::{PhaseOutput, PhaseStatus};
use compiler__semantic_program::{Declaration, Expression, SemanticFile, Statement};
use compiler__source::Span;

#[must_use]
pub fn lower_semantic_file(semantic_file: &SemanticFile) -> PhaseOutput<ExecutableProgram> {
    let mut diagnostics = Vec::new();
    let mut statements = Vec::new();

    let main_function =
        semantic_file
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "main" => Some(function),
                _ => None,
            });

    if let Some(main_function) = main_function {
        if !main_function.type_parameters.is_empty() {
            diagnostics.push(PhaseDiagnostic::new(
                "build mode currently supports only non-generic main()",
                main_function.span.clone(),
            ));
        }
        if !main_function.parameters.is_empty() {
            diagnostics.push(PhaseDiagnostic::new(
                "build mode currently supports only parameterless main()",
                main_function.span.clone(),
            ));
        }
        if !is_nil_type(&main_function.return_type) {
            diagnostics.push(PhaseDiagnostic::new(
                "build mode currently supports only main() -> nil",
                main_function.return_type.span.clone(),
            ));
        }

        for statement in &main_function.body.statements {
            match statement {
                Statement::Expression { value, span } => {
                    let executable_expression = lower_expression(value, &mut diagnostics, span);
                    statements.push(ExecutableStatement::Expression {
                        expression: executable_expression,
                    });
                }
                Statement::Return { value, span } => {
                    let executable_expression = lower_expression(value, &mut diagnostics, span);
                    statements.push(ExecutableStatement::Return {
                        value: executable_expression,
                    });
                }
                _ => {
                    diagnostics.push(PhaseDiagnostic::new(
                        "build mode currently supports only print(\"...\") and return nil in main",
                        statement_span(statement),
                    ));
                }
            }
        }
    } else {
        diagnostics.push(PhaseDiagnostic::new(
            "main function not found in binary entrypoint",
            fallback_span(),
        ));
    }

    let status = if diagnostics.is_empty() {
        PhaseStatus::Ok
    } else {
        PhaseStatus::PreventsDownstreamExecution
    };

    PhaseOutput {
        value: ExecutableProgram { statements },
        diagnostics,
        status,
    }
}

fn lower_expression(
    expression: &Expression,
    diagnostics: &mut Vec<PhaseDiagnostic>,
    span: &Span,
) -> ExecutableExpression {
    match expression {
        Expression::NilLiteral { .. } => ExecutableExpression::NilLiteral,
        Expression::StringLiteral { value, .. } => ExecutableExpression::StringLiteral {
            value: value.clone(),
        },
        Expression::Identifier { name, .. } => {
            ExecutableExpression::Identifier { name: name.clone() }
        }
        Expression::Call {
            callee,
            type_arguments,
            arguments,
            ..
        } => {
            if !type_arguments.is_empty() {
                diagnostics.push(PhaseDiagnostic::new(
                    "build mode currently supports calls without type arguments",
                    span.clone(),
                ));
            }
            let lowered_arguments = arguments
                .iter()
                .map(|argument| lower_expression(argument, diagnostics, span))
                .collect();
            ExecutableExpression::Call {
                callee: Box::new(lower_expression(callee, diagnostics, span)),
                arguments: lowered_arguments,
            }
        }
        _ => {
            diagnostics.push(PhaseDiagnostic::new(
                "build mode currently supports only nil, string literals, identifiers, and call expressions",
                span.clone(),
            ));
            ExecutableExpression::NilLiteral
        }
    }
}

fn is_nil_type(type_name: &compiler__semantic_program::TypeName) -> bool {
    type_name.names.len() == 1
        && type_name.names[0].name == "nil"
        && type_name.names[0].type_arguments.is_empty()
}

fn statement_span(statement: &Statement) -> Span {
    match statement {
        Statement::Let { span, .. }
        | Statement::Assign { span, .. }
        | Statement::Return { span, .. }
        | Statement::Abort { span, .. }
        | Statement::Break { span, .. }
        | Statement::Continue { span, .. }
        | Statement::If { span, .. }
        | Statement::For { span, .. }
        | Statement::Expression { span, .. } => span.clone(),
    }
}

fn fallback_span() -> Span {
    Span {
        start: 0,
        end: 0,
        line: 1,
        column: 1,
    }
}
