use compiler__diagnostics::PhaseDiagnostic;
use compiler__executable_program::{ExecutableExpression, ExecutableProgram, ExecutableStatement};
use compiler__phase_results::{PhaseOutput, PhaseStatus};
use compiler__source::Span;
use compiler__type_annotated_program::{
    TypeAnnotatedBinaryOperator, TypeAnnotatedExpression, TypeAnnotatedFile, TypeAnnotatedStatement,
};

#[must_use]
pub fn lower_type_annotated_file(
    type_annotated_file: &TypeAnnotatedFile,
) -> PhaseOutput<ExecutableProgram> {
    let mut diagnostics = Vec::new();

    validate_main_signature_from_type_analysis(type_annotated_file, &mut diagnostics);

    let statements = if let Some(main_function) = &type_annotated_file.main_function {
        lower_statements(&main_function.statements, &mut diagnostics)
    } else {
        diagnostics.push(PhaseDiagnostic::new(
            "main function not found in binary entrypoint",
            fallback_span(),
        ));
        Vec::new()
    };

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

fn validate_main_signature_from_type_analysis(
    type_annotated_file: &TypeAnnotatedFile,
    diagnostics: &mut Vec<PhaseDiagnostic>,
) {
    let fallback_span_for_diagnostic = type_annotated_file
        .main_function
        .as_ref()
        .map_or_else(fallback_span, |main_function| main_function.span.clone());
    let Some(main_signature) = type_annotated_file.function_signature_by_name.get("main") else {
        diagnostics.push(PhaseDiagnostic::new(
            "build mode requires type analysis information for main",
            fallback_span_for_diagnostic,
        ));
        return;
    };
    if main_signature.type_parameter_count != 0 {
        diagnostics.push(PhaseDiagnostic::new(
            "build mode currently supports only non-generic main()",
            fallback_span_for_diagnostic.clone(),
        ));
    }
    if main_signature.parameter_count != 0 {
        diagnostics.push(PhaseDiagnostic::new(
            "build mode currently supports only parameterless main()",
            fallback_span_for_diagnostic.clone(),
        ));
    }
    if !main_signature.returns_nil {
        diagnostics.push(PhaseDiagnostic::new(
            "build mode currently supports only main() -> nil",
            fallback_span_for_diagnostic,
        ));
    }
}

fn lower_statements(
    statements: &[TypeAnnotatedStatement],
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> Vec<ExecutableStatement> {
    statements
        .iter()
        .filter_map(|statement| lower_statement(statement, diagnostics))
        .collect()
}

fn lower_statement(
    statement: &TypeAnnotatedStatement,
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> Option<ExecutableStatement> {
    match statement {
        TypeAnnotatedStatement::Binding {
            name,
            mutable,
            initializer,
            ..
        } => {
            let executable_initializer = lower_expression(initializer, diagnostics);
            Some(ExecutableStatement::Binding {
                name: name.clone(),
                mutable: *mutable,
                initializer: executable_initializer,
            })
        }
        TypeAnnotatedStatement::Assign { name, value, .. } => {
            let executable_value = lower_expression(value, diagnostics);
            Some(ExecutableStatement::Assign {
                name: name.clone(),
                value: executable_value,
            })
        }
        TypeAnnotatedStatement::If {
            condition,
            then_statements,
            else_statements,
            ..
        } => Some(ExecutableStatement::If {
            condition: lower_expression(condition, diagnostics),
            then_statements: lower_statements(then_statements, diagnostics),
            else_statements: else_statements
                .as_ref()
                .map(|statements| lower_statements(statements, diagnostics)),
        }),
        TypeAnnotatedStatement::For {
            condition,
            body_statements,
            ..
        } => Some(ExecutableStatement::For {
            condition: condition
                .as_ref()
                .map(|expression| lower_expression(expression, diagnostics)),
            body_statements: lower_statements(body_statements, diagnostics),
        }),
        TypeAnnotatedStatement::Expression { value, .. } => {
            let executable_expression = lower_expression(value, diagnostics);
            Some(ExecutableStatement::Expression {
                expression: executable_expression,
            })
        }
        TypeAnnotatedStatement::Return { value, .. } => {
            let executable_expression = lower_expression(value, diagnostics);
            Some(ExecutableStatement::Return {
                value: executable_expression,
            })
        }
        TypeAnnotatedStatement::Unsupported { span } => {
            diagnostics.push(PhaseDiagnostic::new(
                "build mode does not support this statement yet",
                span.clone(),
            ));
            None
        }
    }
}

fn lower_expression(
    expression: &TypeAnnotatedExpression,
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> ExecutableExpression {
    match expression {
        TypeAnnotatedExpression::IntegerLiteral { value, .. } => {
            ExecutableExpression::IntegerLiteral { value: *value }
        }
        TypeAnnotatedExpression::BooleanLiteral { value, .. } => {
            ExecutableExpression::BooleanLiteral { value: *value }
        }
        TypeAnnotatedExpression::NilLiteral { .. } => ExecutableExpression::NilLiteral,
        TypeAnnotatedExpression::StringLiteral { value, .. } => {
            ExecutableExpression::StringLiteral {
                value: value.clone(),
            }
        }
        TypeAnnotatedExpression::Identifier { name, .. } => {
            ExecutableExpression::Identifier { name: name.clone() }
        }
        TypeAnnotatedExpression::Binary {
            operator,
            left,
            right,
            ..
        } => match operator {
            TypeAnnotatedBinaryOperator::Add => ExecutableExpression::Add {
                left: Box::new(lower_expression(left, diagnostics)),
                right: Box::new(lower_expression(right, diagnostics)),
            },
        },
        TypeAnnotatedExpression::Call {
            callee,
            arguments,
            has_type_arguments,
            span,
        } => {
            if *has_type_arguments {
                diagnostics.push(PhaseDiagnostic::new(
                    "build mode currently supports calls without type arguments",
                    span.clone(),
                ));
            }
            let lowered_arguments = arguments
                .iter()
                .map(|argument| lower_expression(argument, diagnostics))
                .collect();
            ExecutableExpression::Call {
                callee: Box::new(lower_expression(callee, diagnostics)),
                arguments: lowered_arguments,
            }
        }
        TypeAnnotatedExpression::Unsupported { span } => {
            diagnostics.push(PhaseDiagnostic::new(
                "build mode does not support this expression yet",
                span.clone(),
            ));
            ExecutableExpression::NilLiteral
        }
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
