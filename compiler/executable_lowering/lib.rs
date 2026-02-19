use compiler__diagnostics::PhaseDiagnostic;
use compiler__executable_program::{
    ExecutableBinaryOperator, ExecutableCallTarget, ExecutableCallableReference,
    ExecutableExpression, ExecutableFunctionDeclaration, ExecutableMatchArm,
    ExecutableMatchPattern, ExecutableMethodDeclaration, ExecutableParameterDeclaration,
    ExecutableProgram, ExecutableStatement, ExecutableStructDeclaration,
    ExecutableStructFieldDeclaration, ExecutableStructLiteralField, ExecutableStructReference,
    ExecutableTypeReference, ExecutableUnaryOperator,
};
use compiler__phase_results::{PhaseOutput, PhaseStatus};
use compiler__source::Span;
use compiler__type_annotated_program::{
    TypeAnnotatedBinaryOperator, TypeAnnotatedCallTarget, TypeAnnotatedExpression,
    TypeAnnotatedFile, TypeAnnotatedFunctionDeclaration, TypeAnnotatedMatchArm,
    TypeAnnotatedMatchPattern, TypeAnnotatedMethodDeclaration, TypeAnnotatedStatement,
    TypeAnnotatedStructDeclaration, TypeAnnotatedTypeName, TypeAnnotatedUnaryOperator,
};

#[must_use]
pub fn lower_type_annotated_file(
    type_annotated_file: &TypeAnnotatedFile,
) -> PhaseOutput<ExecutableProgram> {
    lower_type_annotated_build_unit(type_annotated_file, &[])
}

#[must_use]
pub fn lower_type_annotated_build_unit(
    binary_entrypoint_file: &TypeAnnotatedFile,
    dependency_library_files: &[&TypeAnnotatedFile],
) -> PhaseOutput<ExecutableProgram> {
    let mut diagnostics = Vec::new();

    let entrypoint_callable_reference =
        validate_main_signature_from_type_analysis(binary_entrypoint_file, &mut diagnostics);

    let mut all_struct_declarations = Vec::new();
    let mut all_function_declarations = Vec::new();
    all_struct_declarations.extend(binary_entrypoint_file.struct_declarations.iter().cloned());
    all_function_declarations.extend(binary_entrypoint_file.function_declarations.iter().cloned());
    for dependency_file in dependency_library_files {
        all_struct_declarations.extend(dependency_file.struct_declarations.iter().cloned());
        all_function_declarations.extend(dependency_file.function_declarations.iter().cloned());
    }

    let struct_declarations = lower_struct_declarations(&all_struct_declarations, &mut diagnostics);
    let function_declarations =
        lower_function_declarations(&all_function_declarations, &mut diagnostics);

    let status = if diagnostics.is_empty() {
        PhaseStatus::Ok
    } else {
        PhaseStatus::PreventsDownstreamExecution
    };

    let entrypoint_callable_reference =
        entrypoint_callable_reference.unwrap_or_else(|| ExecutableCallableReference {
            package_path: String::new(),
            symbol_name: "main".to_string(),
        });

    PhaseOutput {
        value: ExecutableProgram {
            entrypoint_callable_reference,
            struct_declarations,
            function_declarations,
        },
        diagnostics,
        status,
    }
}

fn lower_function_declarations(
    function_declarations: &[TypeAnnotatedFunctionDeclaration],
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> Vec<ExecutableFunctionDeclaration> {
    let mut lowered = Vec::new();
    for function_declaration in function_declarations {
        let mut function_supported = true;
        let mut executable_parameters = Vec::new();
        for parameter in &function_declaration.parameters {
            let Some(type_reference) =
                lower_type_name_to_type_reference(&parameter.type_name, true, diagnostics)
            else {
                function_supported = false;
                continue;
            };
            executable_parameters.push(ExecutableParameterDeclaration {
                name: parameter.name.clone(),
                type_reference,
            });
        }
        let Some(return_type) =
            lower_type_name_to_type_reference(&function_declaration.return_type, true, diagnostics)
        else {
            continue;
        };
        if !function_supported {
            continue;
        }
        lowered.push(ExecutableFunctionDeclaration {
            name: function_declaration.name.clone(),
            callable_reference: ExecutableCallableReference {
                package_path: function_declaration.callable_reference.package_path.clone(),
                symbol_name: function_declaration.callable_reference.symbol_name.clone(),
            },
            parameters: executable_parameters,
            return_type,
            statements: lower_statements(&function_declaration.statements, diagnostics),
        });
    }
    lowered
}

fn lower_struct_declarations(
    struct_declarations: &[TypeAnnotatedStructDeclaration],
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> Vec<ExecutableStructDeclaration> {
    let mut lowered = Vec::new();
    for struct_declaration in struct_declarations {
        let mut executable_fields = Vec::new();
        let mut struct_supported = true;
        for field in &struct_declaration.fields {
            let Some(type_reference) =
                lower_type_name_to_type_reference(&field.type_name, false, diagnostics)
            else {
                struct_supported = false;
                continue;
            };
            executable_fields.push(ExecutableStructFieldDeclaration {
                name: field.name.clone(),
                type_reference,
            });
        }
        if struct_supported {
            lowered.push(ExecutableStructDeclaration {
                name: struct_declaration.name.clone(),
                struct_reference: ExecutableStructReference {
                    package_path: struct_declaration.struct_reference.package_path.clone(),
                    symbol_name: struct_declaration.struct_reference.symbol_name.clone(),
                },
                fields: executable_fields,
                methods: lower_method_declarations(&struct_declaration.methods, diagnostics),
            });
        }
    }
    lowered
}

fn lower_method_declarations(
    method_declarations: &[TypeAnnotatedMethodDeclaration],
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> Vec<ExecutableMethodDeclaration> {
    let mut lowered = Vec::new();
    for method_declaration in method_declarations {
        let mut method_supported = true;
        let mut executable_parameters = Vec::new();
        for parameter in &method_declaration.parameters {
            let Some(type_reference) =
                lower_type_name_to_type_reference(&parameter.type_name, true, diagnostics)
            else {
                method_supported = false;
                continue;
            };
            executable_parameters.push(ExecutableParameterDeclaration {
                name: parameter.name.clone(),
                type_reference,
            });
        }
        let Some(return_type) =
            lower_type_name_to_type_reference(&method_declaration.return_type, true, diagnostics)
        else {
            continue;
        };
        if !method_supported {
            continue;
        }
        lowered.push(ExecutableMethodDeclaration {
            name: method_declaration.name.clone(),
            self_mutable: method_declaration.self_mutable,
            parameters: executable_parameters,
            return_type,
            statements: lower_statements(&method_declaration.statements, diagnostics),
        });
    }
    lowered
}

fn validate_main_signature_from_type_analysis(
    type_annotated_file: &TypeAnnotatedFile,
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> Option<ExecutableCallableReference> {
    let fallback_span_for_diagnostic = type_annotated_file
        .function_declarations
        .iter()
        .find(|function_declaration| function_declaration.name == "main")
        .map_or_else(fallback_span, |main_function_declaration| {
            main_function_declaration.span.clone()
        });
    let Some(main_signature) = type_annotated_file.function_signature_by_name.get("main") else {
        diagnostics.push(PhaseDiagnostic::new(
            "build mode requires type analysis information for main",
            fallback_span_for_diagnostic,
        ));
        return None;
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

    type_annotated_file
        .function_declarations
        .iter()
        .find(|function_declaration| function_declaration.name == "main")
        .map(|function_declaration| ExecutableCallableReference {
            package_path: function_declaration.callable_reference.package_path.clone(),
            symbol_name: function_declaration.callable_reference.symbol_name.clone(),
        })
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
        TypeAnnotatedStatement::Break { .. } => Some(ExecutableStatement::Break),
        TypeAnnotatedStatement::Continue { .. } => Some(ExecutableStatement::Continue),
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
        TypeAnnotatedExpression::NameReference { name, .. } => {
            ExecutableExpression::Identifier { name: name.clone() }
        }
        TypeAnnotatedExpression::StructLiteral {
            struct_reference,
            fields,
            span,
            ..
        } => {
            let Some(struct_reference) = struct_reference else {
                diagnostics.push(PhaseDiagnostic::new(
                    "build mode requires resolved struct reference metadata for struct literals",
                    span.clone(),
                ));
                return ExecutableExpression::NilLiteral;
            };
            let executable_fields = fields
                .iter()
                .map(|field| ExecutableStructLiteralField {
                    name: field.name.clone(),
                    value: lower_expression(&field.value, diagnostics),
                })
                .collect();
            ExecutableExpression::StructLiteral {
                struct_reference: ExecutableStructReference {
                    package_path: struct_reference.package_path.clone(),
                    symbol_name: struct_reference.symbol_name.clone(),
                },
                fields: executable_fields,
            }
        }
        TypeAnnotatedExpression::FieldAccess { target, field, .. } => {
            ExecutableExpression::FieldAccess {
                target: Box::new(lower_expression(target, diagnostics)),
                field: field.clone(),
            }
        }
        TypeAnnotatedExpression::Unary {
            operator,
            expression,
            ..
        } => ExecutableExpression::Unary {
            operator: match operator {
                TypeAnnotatedUnaryOperator::Not => ExecutableUnaryOperator::Not,
                TypeAnnotatedUnaryOperator::Negate => ExecutableUnaryOperator::Negate,
            },
            expression: Box::new(lower_expression(expression, diagnostics)),
        },
        TypeAnnotatedExpression::Binary {
            operator,
            left,
            right,
            ..
        } => ExecutableExpression::Binary {
            operator: match operator {
                TypeAnnotatedBinaryOperator::Add => ExecutableBinaryOperator::Add,
                TypeAnnotatedBinaryOperator::Subtract => ExecutableBinaryOperator::Subtract,
                TypeAnnotatedBinaryOperator::Multiply => ExecutableBinaryOperator::Multiply,
                TypeAnnotatedBinaryOperator::Divide => ExecutableBinaryOperator::Divide,
                TypeAnnotatedBinaryOperator::EqualEqual => ExecutableBinaryOperator::EqualEqual,
                TypeAnnotatedBinaryOperator::NotEqual => ExecutableBinaryOperator::NotEqual,
                TypeAnnotatedBinaryOperator::LessThan => ExecutableBinaryOperator::LessThan,
                TypeAnnotatedBinaryOperator::LessThanOrEqual => {
                    ExecutableBinaryOperator::LessThanOrEqual
                }
                TypeAnnotatedBinaryOperator::GreaterThan => ExecutableBinaryOperator::GreaterThan,
                TypeAnnotatedBinaryOperator::GreaterThanOrEqual => {
                    ExecutableBinaryOperator::GreaterThanOrEqual
                }
                TypeAnnotatedBinaryOperator::And => ExecutableBinaryOperator::And,
                TypeAnnotatedBinaryOperator::Or => ExecutableBinaryOperator::Or,
            },
            left: Box::new(lower_expression(left, diagnostics)),
            right: Box::new(lower_expression(right, diagnostics)),
        },
        TypeAnnotatedExpression::Call {
            callee,
            call_target,
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
                call_target: call_target.as_ref().map(|call_target| match call_target {
                    TypeAnnotatedCallTarget::BuiltinFunction { function_name } => {
                        ExecutableCallTarget::BuiltinFunction {
                            function_name: function_name.clone(),
                        }
                    }
                    TypeAnnotatedCallTarget::UserDefinedFunction { callable_reference } => {
                        ExecutableCallTarget::UserDefinedFunction {
                            callable_reference: ExecutableCallableReference {
                                package_path: callable_reference.package_path.clone(),
                                symbol_name: callable_reference.symbol_name.clone(),
                            },
                        }
                    }
                }),
                arguments: lowered_arguments,
            }
        }
        TypeAnnotatedExpression::Match { target, arms, .. } => {
            let Some(lowered_arms) = lower_match_arms(arms, diagnostics) else {
                return ExecutableExpression::NilLiteral;
            };
            ExecutableExpression::Match {
                target: Box::new(lower_expression(target, diagnostics)),
                arms: lowered_arms,
            }
        }
        TypeAnnotatedExpression::Matches {
            value, type_name, ..
        } => {
            let Some(type_reference) =
                lower_type_name_to_type_reference(type_name, true, diagnostics)
            else {
                return ExecutableExpression::NilLiteral;
            };
            ExecutableExpression::Matches {
                value: Box::new(lower_expression(value, diagnostics)),
                type_reference,
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

fn lower_type_name_to_type_reference(
    type_name: &TypeAnnotatedTypeName,
    allow_nil: bool,
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> Option<ExecutableTypeReference> {
    let name = lower_type_name_to_identifier(type_name, diagnostics)?;
    match name.as_str() {
        "int64" => Some(ExecutableTypeReference::Int64),
        "boolean" => Some(ExecutableTypeReference::Boolean),
        "string" => Some(ExecutableTypeReference::String),
        "nil" => {
            if allow_nil {
                Some(ExecutableTypeReference::Nil)
            } else {
                diagnostics.push(PhaseDiagnostic::new(
                    "build mode does not support nil as a struct field type yet",
                    type_name.span.clone(),
                ));
                None
            }
        }
        "never" => Some(ExecutableTypeReference::Never),
        _ => Some(ExecutableTypeReference::Named { name }),
    }
}

fn lower_type_name_to_identifier(
    type_name: &TypeAnnotatedTypeName,
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> Option<String> {
    if type_name.names.len() != 1 {
        diagnostics.push(PhaseDiagnostic::new(
            "build mode currently supports only single-segment type names",
            type_name.span.clone(),
        ));
        return None;
    }
    let segment = &type_name.names[0];
    if segment.has_type_arguments {
        diagnostics.push(PhaseDiagnostic::new(
            "build mode currently supports only non-generic type names",
            segment.span.clone(),
        ));
        return None;
    }
    Some(segment.name.clone())
}

fn lower_match_arms(
    arms: &[TypeAnnotatedMatchArm],
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> Option<Vec<ExecutableMatchArm>> {
    let mut lowered_arms = Vec::new();
    for arm in arms {
        let pattern = lower_match_pattern(&arm.pattern, diagnostics)?;
        lowered_arms.push(ExecutableMatchArm {
            pattern,
            value: lower_expression(&arm.value, diagnostics),
        });
    }
    Some(lowered_arms)
}

fn lower_match_pattern(
    pattern: &TypeAnnotatedMatchPattern,
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> Option<ExecutableMatchPattern> {
    match pattern {
        TypeAnnotatedMatchPattern::Type { type_name, .. } => {
            let type_reference = lower_type_name_to_type_reference(type_name, true, diagnostics)?;
            Some(ExecutableMatchPattern::Type { type_reference })
        }
        TypeAnnotatedMatchPattern::Binding {
            name, type_name, ..
        } => {
            let type_reference = lower_type_name_to_type_reference(type_name, true, diagnostics)?;
            Some(ExecutableMatchPattern::Binding {
                binding_name: name.clone(),
                type_reference,
            })
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
