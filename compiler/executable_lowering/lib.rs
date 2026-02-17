use compiler__diagnostics::PhaseDiagnostic;
use compiler__executable_program::{ExecutableExpression, ExecutableProgram, ExecutableStatement};
use compiler__phase_results::{PhaseOutput, PhaseStatus};
use compiler__source::Span;
use compiler__type_annotated_program::{
    TypeAnnotatedExpression, TypeAnnotatedFile, TypeAnnotatedStatement,
};

#[must_use]
pub fn lower_type_annotated_file(
    type_annotated_file: &TypeAnnotatedFile,
) -> PhaseOutput<ExecutableProgram> {
    let mut diagnostics = Vec::new();
    let mut statements = Vec::new();

    validate_main_signature_from_type_analysis(type_annotated_file, &mut diagnostics);

    if let Some(main_function) = &type_annotated_file.main_function {
        for statement in &main_function.statements {
            match statement {
                TypeAnnotatedStatement::Expression { value, .. } => {
                    let executable_expression = lower_expression(value, &mut diagnostics);
                    statements.push(ExecutableStatement::Expression {
                        expression: executable_expression,
                    });
                }
                TypeAnnotatedStatement::Return { value, .. } => {
                    let executable_expression = lower_expression(value, &mut diagnostics);
                    statements.push(ExecutableStatement::Return {
                        value: executable_expression,
                    });
                }
                TypeAnnotatedStatement::Unsupported { span } => {
                    diagnostics.push(PhaseDiagnostic::new(
                        "build mode currently supports only print(\"...\") and return nil in main",
                        span.clone(),
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

fn lower_expression(
    expression: &TypeAnnotatedExpression,
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> ExecutableExpression {
    match expression {
        TypeAnnotatedExpression::NilLiteral { .. } => ExecutableExpression::NilLiteral,
        TypeAnnotatedExpression::StringLiteral { value, .. } => {
            ExecutableExpression::StringLiteral {
                value: value.clone(),
            }
        }
        TypeAnnotatedExpression::Identifier { name, .. } => {
            ExecutableExpression::Identifier { name: name.clone() }
        }
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
                "build mode currently supports only nil, string literals, identifiers, and call expressions",
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
