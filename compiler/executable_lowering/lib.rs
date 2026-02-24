use compiler__diagnostics::PhaseDiagnostic;
use compiler__executable_program::{
    ExecutableBinaryOperator, ExecutableCallTarget, ExecutableCallableReference,
    ExecutableConstantDeclaration, ExecutableConstantReference, ExecutableEnumVariantReference,
    ExecutableExpression, ExecutableFunctionDeclaration, ExecutableMatchArm,
    ExecutableMatchPattern, ExecutableMethodDeclaration, ExecutableParameterDeclaration,
    ExecutableProgram, ExecutableStatement, ExecutableStructDeclaration,
    ExecutableStructFieldDeclaration, ExecutableStructLiteralField, ExecutableStructReference,
    ExecutableTypeReference, ExecutableUnaryOperator,
};
use compiler__phase_results::{PhaseOutput, PhaseStatus};
use compiler__source::Span;
use compiler__type_annotated_program::{
    TypeAnnotatedBinaryOperator, TypeAnnotatedCallTarget, TypeAnnotatedConstantDeclaration,
    TypeAnnotatedExpression, TypeAnnotatedFile, TypeAnnotatedFunctionDeclaration,
    TypeAnnotatedMatchArm, TypeAnnotatedMatchPattern, TypeAnnotatedMethodDeclaration,
    TypeAnnotatedResolvedTypeArgument, TypeAnnotatedStatement, TypeAnnotatedStructDeclaration,
    TypeAnnotatedTypeName, TypeAnnotatedUnaryOperator,
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
    let mut all_constant_declarations = Vec::new();
    let mut all_function_declarations = Vec::new();
    all_struct_declarations.extend(binary_entrypoint_file.struct_declarations.iter().cloned());
    all_constant_declarations.extend(binary_entrypoint_file.constant_declarations.iter().cloned());
    all_function_declarations.extend(binary_entrypoint_file.function_declarations.iter().cloned());
    for dependency_file in dependency_library_files {
        all_struct_declarations.extend(dependency_file.struct_declarations.iter().cloned());
        all_constant_declarations.extend(dependency_file.constant_declarations.iter().cloned());
        all_function_declarations.extend(dependency_file.function_declarations.iter().cloned());
    }

    let constant_declarations =
        lower_constant_declarations(&all_constant_declarations, &mut diagnostics);
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
            constant_declarations,
            struct_declarations,
            function_declarations,
        },
        diagnostics,
        status,
    }
}

fn lower_constant_declarations(
    constant_declarations: &[TypeAnnotatedConstantDeclaration],
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> Vec<ExecutableConstantDeclaration> {
    let mut lowered = Vec::new();
    for constant_declaration in constant_declarations {
        let Some(type_reference) = lower_type_name_to_type_reference(
            &constant_declaration.type_name,
            true,
            &[],
            diagnostics,
        ) else {
            continue;
        };
        lowered.push(ExecutableConstantDeclaration {
            name: constant_declaration.name.clone(),
            constant_reference: ExecutableConstantReference {
                package_path: constant_declaration.constant_reference.package_path.clone(),
                symbol_name: constant_declaration.constant_reference.symbol_name.clone(),
            },
            type_reference,
            initializer: lower_expression(&constant_declaration.initializer, &[], diagnostics),
        });
    }
    lowered
}

fn lower_function_declarations(
    function_declarations: &[TypeAnnotatedFunctionDeclaration],
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> Vec<ExecutableFunctionDeclaration> {
    let mut lowered = Vec::new();
    for function_declaration in function_declarations {
        let mut function_supported = true;
        let mut executable_parameters = Vec::new();
        let type_parameter_names = function_declaration
            .type_parameters
            .iter()
            .map(|type_parameter| type_parameter.name.clone())
            .collect::<Vec<_>>();
        for parameter in &function_declaration.parameters {
            let Some(type_reference) = lower_type_name_to_type_reference(
                &parameter.type_name,
                true,
                &type_parameter_names,
                diagnostics,
            ) else {
                function_supported = false;
                continue;
            };
            executable_parameters.push(ExecutableParameterDeclaration {
                name: parameter.name.clone(),
                type_reference,
            });
        }
        let Some(return_type) = lower_type_name_to_type_reference(
            &function_declaration.return_type,
            true,
            &type_parameter_names,
            diagnostics,
        ) else {
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
            type_parameter_names: type_parameter_names.clone(),
            parameters: executable_parameters,
            return_type,
            statements: lower_statements(
                &function_declaration.statements,
                &type_parameter_names,
                diagnostics,
            ),
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
        let type_parameter_names = struct_declaration
            .type_parameters
            .iter()
            .map(|type_parameter| type_parameter.name.clone())
            .collect::<Vec<_>>();
        let mut executable_fields = Vec::new();
        let mut struct_supported = true;
        for field in &struct_declaration.fields {
            let Some(type_reference) = lower_type_name_to_type_reference(
                &field.type_name,
                false,
                &type_parameter_names,
                diagnostics,
            ) else {
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
                type_parameter_names: type_parameter_names.clone(),
                fields: executable_fields,
                methods: lower_method_declarations(
                    &struct_declaration.methods,
                    &type_parameter_names,
                    diagnostics,
                ),
            });
        }
    }
    lowered
}

fn lower_method_declarations(
    method_declarations: &[TypeAnnotatedMethodDeclaration],
    enclosing_type_parameter_names: &[String],
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> Vec<ExecutableMethodDeclaration> {
    let mut lowered = Vec::new();
    for method_declaration in method_declarations {
        let mut method_supported = true;
        let mut executable_parameters = Vec::new();
        for parameter in &method_declaration.parameters {
            let Some(type_reference) = lower_type_name_to_type_reference(
                &parameter.type_name,
                true,
                enclosing_type_parameter_names,
                diagnostics,
            ) else {
                method_supported = false;
                continue;
            };
            executable_parameters.push(ExecutableParameterDeclaration {
                name: parameter.name.clone(),
                type_reference,
            });
        }
        let Some(return_type) = lower_type_name_to_type_reference(
            &method_declaration.return_type,
            true,
            enclosing_type_parameter_names,
            diagnostics,
        ) else {
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
            statements: lower_statements(
                &method_declaration.statements,
                enclosing_type_parameter_names,
                diagnostics,
            ),
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
    type_parameter_names: &[String],
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> Vec<ExecutableStatement> {
    statements
        .iter()
        .map(|statement| lower_statement(statement, type_parameter_names, diagnostics))
        .collect()
}

fn lower_statement(
    statement: &TypeAnnotatedStatement,
    type_parameter_names: &[String],
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> ExecutableStatement {
    match statement {
        TypeAnnotatedStatement::Binding {
            name,
            mutable,
            initializer,
            ..
        } => {
            let executable_initializer =
                lower_expression(initializer, type_parameter_names, diagnostics);
            ExecutableStatement::Binding {
                name: name.clone(),
                mutable: *mutable,
                initializer: executable_initializer,
            }
        }
        TypeAnnotatedStatement::Assign { name, value, .. } => {
            let executable_value = lower_expression(value, type_parameter_names, diagnostics);
            ExecutableStatement::Assign {
                name: name.clone(),
                value: executable_value,
            }
        }
        TypeAnnotatedStatement::If {
            condition,
            then_statements,
            else_statements,
            ..
        } => ExecutableStatement::If {
            condition: lower_expression(condition, type_parameter_names, diagnostics),
            then_statements: lower_statements(then_statements, type_parameter_names, diagnostics),
            else_statements: else_statements
                .as_ref()
                .map(|statements| lower_statements(statements, type_parameter_names, diagnostics)),
        },
        TypeAnnotatedStatement::For {
            condition,
            body_statements,
            ..
        } => ExecutableStatement::For {
            condition: condition
                .as_ref()
                .map(|expression| lower_expression(expression, type_parameter_names, diagnostics)),
            body_statements: lower_statements(body_statements, type_parameter_names, diagnostics),
        },
        TypeAnnotatedStatement::Break { .. } => ExecutableStatement::Break,
        TypeAnnotatedStatement::Continue { .. } => ExecutableStatement::Continue,
        TypeAnnotatedStatement::Expression { value, .. } => {
            let executable_expression = lower_expression(value, type_parameter_names, diagnostics);
            ExecutableStatement::Expression {
                expression: executable_expression,
            }
        }
        TypeAnnotatedStatement::Return { value, .. } => {
            let executable_expression = lower_expression(value, type_parameter_names, diagnostics);
            ExecutableStatement::Return {
                value: executable_expression,
            }
        }
    }
}

fn lower_expression(
    expression: &TypeAnnotatedExpression,
    type_parameter_names: &[String],
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
        TypeAnnotatedExpression::NameReference {
            name,
            constant_reference,
            ..
        } => ExecutableExpression::Identifier {
            name: name.clone(),
            constant_reference: constant_reference.as_ref().map(|constant_reference| {
                ExecutableConstantReference {
                    package_path: constant_reference.package_path.clone(),
                    symbol_name: constant_reference.symbol_name.clone(),
                }
            }),
        },
        TypeAnnotatedExpression::EnumVariantLiteral {
            enum_variant_reference,
            ..
        } => ExecutableExpression::EnumVariantLiteral {
            enum_variant_reference: ExecutableEnumVariantReference {
                enum_name: enum_variant_reference.enum_name.clone(),
                variant_name: enum_variant_reference.variant_name.clone(),
            },
            type_reference: ExecutableTypeReference::NominalType {
                name: format!(
                    "{}.{}",
                    enum_variant_reference.enum_name, enum_variant_reference.variant_name
                ),
            },
        },
        TypeAnnotatedExpression::StructLiteral {
            type_name,
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
            let Some(type_reference) = lower_type_name_to_type_reference(
                type_name,
                false,
                type_parameter_names,
                diagnostics,
            ) else {
                return ExecutableExpression::NilLiteral;
            };
            let executable_fields = fields
                .iter()
                .map(|field| ExecutableStructLiteralField {
                    name: field.name.clone(),
                    value: lower_expression(&field.value, type_parameter_names, diagnostics),
                })
                .collect();
            ExecutableExpression::StructLiteral {
                struct_reference: ExecutableStructReference {
                    package_path: struct_reference.package_path.clone(),
                    symbol_name: struct_reference.symbol_name.clone(),
                },
                type_reference,
                fields: executable_fields,
            }
        }
        TypeAnnotatedExpression::FieldAccess { target, field, .. } => {
            ExecutableExpression::FieldAccess {
                target: Box::new(lower_expression(target, type_parameter_names, diagnostics)),
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
            expression: Box::new(lower_expression(
                expression,
                type_parameter_names,
                diagnostics,
            )),
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
                TypeAnnotatedBinaryOperator::Modulo => ExecutableBinaryOperator::Modulo,
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
            left: Box::new(lower_expression(left, type_parameter_names, diagnostics)),
            right: Box::new(lower_expression(right, type_parameter_names, diagnostics)),
        },
        TypeAnnotatedExpression::Call {
            callee,
            call_target,
            arguments,
            type_arguments: _,
            resolved_type_arguments,
            span: _,
        } => {
            let lowered_arguments = arguments
                .iter()
                .map(|argument| lower_expression(argument, type_parameter_names, diagnostics))
                .collect();
            let lowered_type_arguments = resolved_type_arguments
                .iter()
                .map(|type_reference| {
                    lower_type_reference_to_type_reference(type_reference, type_parameter_names)
                })
                .collect();
            ExecutableExpression::Call {
                callee: Box::new(lower_expression(callee, type_parameter_names, diagnostics)),
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
                type_arguments: lowered_type_arguments,
            }
        }
        TypeAnnotatedExpression::Match { target, arms, .. } => {
            let Some(lowered_arms) = lower_match_arms(arms, type_parameter_names, diagnostics)
            else {
                return ExecutableExpression::NilLiteral;
            };
            ExecutableExpression::Match {
                target: Box::new(lower_expression(target, type_parameter_names, diagnostics)),
                arms: lowered_arms,
            }
        }
        TypeAnnotatedExpression::Matches {
            value, type_name, ..
        } => {
            let Some(type_reference) = lower_type_name_to_type_reference(
                type_name,
                true,
                type_parameter_names,
                diagnostics,
            ) else {
                return ExecutableExpression::NilLiteral;
            };
            ExecutableExpression::Matches {
                value: Box::new(lower_expression(value, type_parameter_names, diagnostics)),
                type_reference,
            }
        }
    }
}

fn lower_type_reference_to_type_reference(
    type_reference: &TypeAnnotatedResolvedTypeArgument,
    type_parameter_names: &[String],
) -> ExecutableTypeReference {
    match type_reference {
        TypeAnnotatedResolvedTypeArgument::Int64 => ExecutableTypeReference::Int64,
        TypeAnnotatedResolvedTypeArgument::Boolean => ExecutableTypeReference::Boolean,
        TypeAnnotatedResolvedTypeArgument::String => ExecutableTypeReference::String,
        TypeAnnotatedResolvedTypeArgument::Nil => ExecutableTypeReference::Nil,
        TypeAnnotatedResolvedTypeArgument::Never => ExecutableTypeReference::Never,
        TypeAnnotatedResolvedTypeArgument::Union { members } => ExecutableTypeReference::Union {
            members: members
                .iter()
                .map(|member| lower_type_reference_to_type_reference(member, type_parameter_names))
                .collect(),
        },
        TypeAnnotatedResolvedTypeArgument::TypeParameter { name } => {
            assert!(
                type_parameter_names
                    .iter()
                    .any(|type_parameter| type_parameter == name),
                "internal invariant: unknown type parameter '{name}' in resolved type arguments"
            );
            ExecutableTypeReference::TypeParameter { name: name.clone() }
        }
        TypeAnnotatedResolvedTypeArgument::NominalTypeApplication {
            base_name,
            arguments,
        } => ExecutableTypeReference::NominalTypeApplication {
            base_name: base_name.clone(),
            arguments: arguments
                .iter()
                .map(|argument| {
                    lower_type_reference_to_type_reference(argument, type_parameter_names)
                })
                .collect(),
        },
        TypeAnnotatedResolvedTypeArgument::NominalType { name } => {
            ExecutableTypeReference::NominalType { name: name.clone() }
        }
    }
}

fn lower_type_name_to_type_reference(
    type_name: &TypeAnnotatedTypeName,
    allow_nil: bool,
    type_parameter_names: &[String],
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> Option<ExecutableTypeReference> {
    if type_name.names.is_empty() {
        diagnostics.push(PhaseDiagnostic::new(
            "build mode requires non-empty type names",
            type_name.span.clone(),
        ));
        return None;
    }

    if type_name.names.len() == 1 {
        return lower_type_name_segment_to_type_reference(
            &type_name.names[0],
            allow_nil,
            type_parameter_names,
            diagnostics,
        );
    }

    let mut union_members = Vec::new();
    for type_name_segment in &type_name.names {
        let member = lower_type_name_segment_to_type_reference(
            type_name_segment,
            allow_nil,
            type_parameter_names,
            diagnostics,
        )?;
        union_members.push(member);
    }
    Some(ExecutableTypeReference::Union {
        members: union_members,
    })
}

fn lower_type_name_segment_to_type_reference(
    type_name_segment: &compiler__type_annotated_program::TypeAnnotatedTypeNameSegment,
    allow_nil: bool,
    type_parameter_names: &[String],
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> Option<ExecutableTypeReference> {
    let has_type_arguments = !type_name_segment.type_arguments.is_empty();

    if type_parameter_names
        .iter()
        .any(|type_parameter_name| type_parameter_name == &type_name_segment.name)
    {
        if has_type_arguments {
            diagnostics.push(PhaseDiagnostic::new(
                format!(
                    "type parameter '{}' does not take type arguments",
                    type_name_segment.name
                ),
                type_name_segment.span.clone(),
            ));
            return None;
        }
        return Some(ExecutableTypeReference::TypeParameter {
            name: type_name_segment.name.clone(),
        });
    }

    match type_name_segment.name.as_str() {
        "int64" => {
            if has_type_arguments {
                diagnostics.push(PhaseDiagnostic::new(
                    "built-in type 'int64' does not take type arguments",
                    type_name_segment.span.clone(),
                ));
                return None;
            }
            Some(ExecutableTypeReference::Int64)
        }
        "boolean" => {
            if has_type_arguments {
                diagnostics.push(PhaseDiagnostic::new(
                    "built-in type 'boolean' does not take type arguments",
                    type_name_segment.span.clone(),
                ));
                return None;
            }
            Some(ExecutableTypeReference::Boolean)
        }
        "string" => {
            if has_type_arguments {
                diagnostics.push(PhaseDiagnostic::new(
                    "built-in type 'string' does not take type arguments",
                    type_name_segment.span.clone(),
                ));
                return None;
            }
            Some(ExecutableTypeReference::String)
        }
        "nil" => {
            if has_type_arguments {
                diagnostics.push(PhaseDiagnostic::new(
                    "built-in type 'nil' does not take type arguments",
                    type_name_segment.span.clone(),
                ));
                return None;
            }
            if allow_nil {
                Some(ExecutableTypeReference::Nil)
            } else {
                diagnostics.push(PhaseDiagnostic::new(
                    "build mode does not support nil as a struct field type yet",
                    type_name_segment.span.clone(),
                ));
                None
            }
        }
        "never" => {
            if has_type_arguments {
                diagnostics.push(PhaseDiagnostic::new(
                    "built-in type 'never' does not take type arguments",
                    type_name_segment.span.clone(),
                ));
                return None;
            }
            Some(ExecutableTypeReference::Never)
        }
        "function" => {
            diagnostics.push(PhaseDiagnostic::new(
                "build mode does not support function type names yet",
                type_name_segment.span.clone(),
            ));
            None
        }
        _ => {
            if has_type_arguments {
                let arguments = type_name_segment
                    .type_arguments
                    .iter()
                    .map(|type_argument| {
                        lower_type_name_to_type_reference(
                            type_argument,
                            true,
                            type_parameter_names,
                            diagnostics,
                        )
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(ExecutableTypeReference::NominalTypeApplication {
                    base_name: type_name_segment.name.clone(),
                    arguments,
                })
            } else {
                Some(ExecutableTypeReference::NominalType {
                    name: type_name_segment.name.clone(),
                })
            }
        }
    }
}

fn lower_match_arms(
    arms: &[TypeAnnotatedMatchArm],
    type_parameter_names: &[String],
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> Option<Vec<ExecutableMatchArm>> {
    let mut lowered_arms = Vec::new();
    for arm in arms {
        let pattern = lower_match_pattern(&arm.pattern, type_parameter_names, diagnostics)?;
        lowered_arms.push(ExecutableMatchArm {
            pattern,
            value: lower_expression(&arm.value, type_parameter_names, diagnostics),
        });
    }
    Some(lowered_arms)
}

fn lower_match_pattern(
    pattern: &TypeAnnotatedMatchPattern,
    type_parameter_names: &[String],
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> Option<ExecutableMatchPattern> {
    match pattern {
        TypeAnnotatedMatchPattern::Type { type_name, .. } => {
            let type_reference = lower_type_name_to_type_reference(
                type_name,
                true,
                type_parameter_names,
                diagnostics,
            )?;
            Some(ExecutableMatchPattern::Type { type_reference })
        }
        TypeAnnotatedMatchPattern::Binding {
            name, type_name, ..
        } => {
            let type_reference = lower_type_name_to_type_reference(
                type_name,
                true,
                type_parameter_names,
                diagnostics,
            )?;
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
