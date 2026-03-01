use std::collections::{BTreeMap, HashMap};

use compiler__diagnostics::PhaseDiagnostic;
use compiler__packages::PackageId;
use compiler__phase_results::{PhaseOutput, PhaseStatus};
use compiler__safe_autofix::SafeAutofix;
use compiler__semantic_program::{
    SemanticAssignTarget, SemanticBinaryOperator, SemanticConstantDeclaration, SemanticDeclaration,
    SemanticExpression, SemanticExpressionId, SemanticFile, SemanticFunctionDeclaration,
    SemanticNameReferenceKind, SemanticStatement, SemanticTypeDeclaration, SemanticTypeName,
    SemanticUnaryOperator,
};
use compiler__semantic_types::{
    GenericTypeParameter, ImportedBinding, ImportedSymbol, ImportedTypeDeclaration, NominalTypeId,
    NominalTypeRef, Type, type_from_builtin_name,
};
use compiler__source::Span;
use compiler__type_annotated_program::{
    TypeAnnotatedAssignTarget, TypeAnnotatedBinaryOperator, TypeAnnotatedCallTarget,
    TypeAnnotatedCallableReference, TypeAnnotatedConstantDeclaration,
    TypeAnnotatedConstantReference, TypeAnnotatedEnumVariantReference, TypeAnnotatedExpression,
    TypeAnnotatedFunctionDeclaration, TypeAnnotatedInterfaceDeclaration,
    TypeAnnotatedInterfaceMethodDeclaration, TypeAnnotatedInterfaceReference,
    TypeAnnotatedMatchArm, TypeAnnotatedMatchPattern, TypeAnnotatedMethodDeclaration,
    TypeAnnotatedNameReferenceKind, TypeAnnotatedNominalTypeReference,
    TypeAnnotatedParameterDeclaration, TypeAnnotatedResolvedTypeArgument, TypeAnnotatedStatement,
    TypeAnnotatedStringInterpolationPart, TypeAnnotatedStructDeclaration,
    TypeAnnotatedStructFieldDeclaration, TypeAnnotatedStructLiteralField,
    TypeAnnotatedStructReference, TypeAnnotatedTypeName, TypeAnnotatedTypeNameSegment,
    TypeAnnotatedTypeParameter, TypeAnnotatedUnaryOperator, TypeResolvedDeclarations,
};

mod assignability;
mod declarations;
mod expressions;
mod naming_rules;
mod statements;
mod type_narrowing;
mod unused_bindings;

struct TypeAnalysisSummary {
    resolved_type_by_expression_id: BTreeMap<SemanticExpressionId, Type>,
    call_target_by_expression_id: BTreeMap<SemanticExpressionId, TypeAnnotatedCallTarget>,
    constant_reference_by_expression_id:
        BTreeMap<SemanticExpressionId, TypeAnnotatedConstantReference>,
    resolved_type_argument_types_by_expression_id:
        BTreeMap<SemanticExpressionId, Vec<TypeAnnotatedResolvedTypeArgument>>,
    struct_reference_by_expression_id: BTreeMap<SemanticExpressionId, TypeAnnotatedStructReference>,
    enum_variant_reference_by_expression_id:
        BTreeMap<SemanticExpressionId, TypeAnnotatedEnumVariantReference>,
    nominal_type_reference_by_local_name: HashMap<String, TypeAnnotatedNominalTypeReference>,
    implemented_interface_references_by_struct_name:
        HashMap<String, Vec<TypeAnnotatedInterfaceReference>>,
    type_declarations_for_annotations: Vec<SemanticTypeDeclaration>,
    constant_declarations_for_annotations: Vec<SemanticConstantDeclaration>,
    function_declarations_for_annotations: Vec<SemanticFunctionDeclaration>,
    resolved_declarations: ResolvedDeclarations,
}

#[derive(Clone)]
pub enum TypeAnalysisBlockingReason {
    TypeErrorsPresent,
}

struct ResolvedDeclarations {
    constants_by_name: HashMap<String, ConstantInfo>,
    functions_by_name: HashMap<String, FunctionInfo>,
    types_by_name: HashMap<String, TypeInfo>,
    methods_by_key: HashMap<MethodKey, MethodInfo>,
}

#[must_use]
pub fn check_package_unit(
    package_id: PackageId,
    package_path: &str,
    source_text: &str,
    package_unit: &SemanticFile,
    imported_bindings: &[ImportedBinding],
) -> PhaseOutput<Result<TypeResolvedDeclarations, TypeAnalysisBlockingReason>> {
    let mut diagnostics = Vec::new();
    let mut safe_autofixes = Vec::new();
    let summary = analyze_package_unit(
        package_id,
        package_path,
        source_text,
        package_unit,
        imported_bindings,
        &mut diagnostics,
        &mut safe_autofixes,
    );
    let status = if diagnostics.is_empty() {
        PhaseStatus::Ok
    } else {
        PhaseStatus::PreventsDownstreamExecution
    };

    let value = if matches!(status, PhaseStatus::Ok) {
        Ok(build_resolved_declarations(
            package_path,
            &summary,
            &summary.nominal_type_reference_by_local_name,
        ))
    } else {
        Err(TypeAnalysisBlockingReason::TypeErrorsPresent)
    };

    PhaseOutput {
        value,
        diagnostics,
        safe_autofixes,
        status,
    }
}

fn build_resolved_declarations(
    package_path: &str,
    summary: &TypeAnalysisSummary,
    nominal_type_reference_by_local_name: &HashMap<String, TypeAnnotatedNominalTypeReference>,
) -> TypeResolvedDeclarations {
    let mut resolved_declarations = TypeResolvedDeclarations {
        constant_declarations: build_constant_declaration_annotations(
            package_path,
            &summary.constant_declarations_for_annotations,
            &summary.resolved_declarations,
            &summary.resolved_type_by_expression_id,
            &summary.call_target_by_expression_id,
            &summary.resolved_type_argument_types_by_expression_id,
            &summary.struct_reference_by_expression_id,
            &summary.enum_variant_reference_by_expression_id,
            &summary.constant_reference_by_expression_id,
        ),
        interface_declarations: build_interface_declaration_annotations(
            package_path,
            &summary.type_declarations_for_annotations,
            &summary.resolved_declarations,
        ),
        struct_declarations: build_struct_declaration_annotations(
            package_path,
            &summary.type_declarations_for_annotations,
            &summary.implemented_interface_references_by_struct_name,
            &summary.resolved_declarations,
            &summary.resolved_type_by_expression_id,
            &summary.call_target_by_expression_id,
            &summary.resolved_type_argument_types_by_expression_id,
            &summary.struct_reference_by_expression_id,
            &summary.enum_variant_reference_by_expression_id,
            &summary.constant_reference_by_expression_id,
        ),
        function_declarations: build_function_declaration_annotations(
            package_path,
            &summary.function_declarations_for_annotations,
            &summary.resolved_declarations,
            &summary.resolved_type_by_expression_id,
            &summary.call_target_by_expression_id,
            &summary.resolved_type_argument_types_by_expression_id,
            &summary.struct_reference_by_expression_id,
            &summary.enum_variant_reference_by_expression_id,
            &summary.constant_reference_by_expression_id,
        ),
    };
    annotate_nominal_type_references(
        &mut resolved_declarations,
        nominal_type_reference_by_local_name,
    );
    resolved_declarations
}

fn build_constant_declaration_annotations(
    package_path: &str,
    constant_declarations: &[SemanticConstantDeclaration],
    resolved_declarations: &ResolvedDeclarations,
    resolved_type_by_expression_id: &BTreeMap<SemanticExpressionId, Type>,
    call_target_by_expression_id: &BTreeMap<SemanticExpressionId, TypeAnnotatedCallTarget>,
    resolved_type_argument_types_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        Vec<TypeAnnotatedResolvedTypeArgument>,
    >,
    struct_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedStructReference,
    >,
    enum_variant_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedEnumVariantReference,
    >,
    constant_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedConstantReference,
    >,
) -> Vec<TypeAnnotatedConstantDeclaration> {
    constant_declarations
        .iter()
        .map(|constant_declaration| {
            let resolved_type = resolved_declarations
                .constants_by_name
                .get(&constant_declaration.name)
                .map(|constant_info| constant_info.value_type.clone())
                .expect("constant declaration must have resolved type info");
            TypeAnnotatedConstantDeclaration {
                name: constant_declaration.name.clone(),
                constant_reference: TypeAnnotatedConstantReference {
                    package_path: package_path.to_string(),
                    symbol_name: constant_declaration.name.clone(),
                },
                type_reference: type_annotated_resolved_type_argument_from_type(&resolved_type)
                    .expect("constant type must be fully resolved"),
                initializer: type_annotated_expression_from_semantic_expression(
                    &constant_declaration.expression,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                ),
                span: constant_declaration.span.clone(),
            }
        })
        .collect()
}

fn build_function_declaration_annotations(
    package_path: &str,
    function_declarations: &[SemanticFunctionDeclaration],
    resolved_declarations: &ResolvedDeclarations,
    resolved_type_by_expression_id: &BTreeMap<SemanticExpressionId, Type>,
    call_target_by_expression_id: &BTreeMap<SemanticExpressionId, TypeAnnotatedCallTarget>,
    resolved_type_argument_types_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        Vec<TypeAnnotatedResolvedTypeArgument>,
    >,
    struct_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedStructReference,
    >,
    enum_variant_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedEnumVariantReference,
    >,
    constant_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedConstantReference,
    >,
) -> Vec<TypeAnnotatedFunctionDeclaration> {
    function_declarations
        .iter()
        .map(|function_declaration| {
            let function_info = resolved_declarations
                .functions_by_name
                .get(&function_declaration.name)
                .expect("function declaration must have resolved signature");
            TypeAnnotatedFunctionDeclaration {
                name: function_declaration.name.clone(),
                callable_reference: TypeAnnotatedCallableReference {
                    package_path: package_path.to_string(),
                    symbol_name: function_declaration.name.clone(),
                },
                type_parameters: function_declaration
                    .type_parameters
                    .iter()
                    .zip(function_info.type_parameters.iter())
                    .map(
                        |(type_parameter, resolved_type_parameter)| TypeAnnotatedTypeParameter {
                            name: type_parameter.name.clone(),
                            constraint_interface_reference: resolved_type_parameter
                                .constraint
                                .as_ref()
                                .and_then(|constraint| {
                                    type_annotated_interface_reference_from_type(
                                        &resolved_declarations.types_by_name,
                                        constraint,
                                    )
                                }),
                            span: type_parameter.span.clone(),
                        },
                    )
                    .collect(),
                parameters: function_declaration
                    .parameters
                    .iter()
                    .zip(function_info.parameter_types.iter())
                    .map(
                        |(parameter, resolved_parameter_type)| TypeAnnotatedParameterDeclaration {
                            name: parameter.name.clone(),
                            mutable: parameter.mutable,
                            type_reference: type_annotated_resolved_type_argument_from_type(
                                resolved_parameter_type,
                            )
                            .expect("function parameter types must be fully resolved"),
                            span: parameter.span.clone(),
                        },
                    )
                    .collect(),
                return_type_reference: type_annotated_resolved_type_argument_from_type(
                    &function_info.return_type,
                )
                .expect("function return type must be fully resolved"),
                span: function_declaration.span.clone(),
                statements: function_declaration
                    .body
                    .statements
                    .iter()
                    .map(|statement| {
                        type_annotated_statement_from_semantic_statement(
                            statement,
                            resolved_type_by_expression_id,
                            call_target_by_expression_id,
                            resolved_type_argument_types_by_expression_id,
                            struct_reference_by_expression_id,
                            enum_variant_reference_by_expression_id,
                            constant_reference_by_expression_id,
                        )
                    })
                    .collect(),
            }
        })
        .collect()
}

fn build_struct_declaration_annotations(
    package_path: &str,
    type_declarations: &[SemanticTypeDeclaration],
    implemented_interface_references_by_struct_name: &HashMap<
        String,
        Vec<TypeAnnotatedInterfaceReference>,
    >,
    resolved_declarations: &ResolvedDeclarations,
    resolved_type_by_expression_id: &BTreeMap<SemanticExpressionId, Type>,
    call_target_by_expression_id: &BTreeMap<SemanticExpressionId, TypeAnnotatedCallTarget>,
    resolved_type_argument_types_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        Vec<TypeAnnotatedResolvedTypeArgument>,
    >,
    struct_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedStructReference,
    >,
    enum_variant_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedEnumVariantReference,
    >,
    constant_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedConstantReference,
    >,
) -> Vec<TypeAnnotatedStructDeclaration> {
    type_declarations
        .iter()
        .filter_map(|type_declaration| match &type_declaration.kind {
            compiler__semantic_program::SemanticTypeDeclarationKind::Struct {
                fields: semantic_fields,
                methods,
            } => {
                let type_info = resolved_declarations
                    .types_by_name
                    .get(&type_declaration.name)
                    .expect("struct declaration must have resolved type info");
                let TypeKind::Struct { fields } = &type_info.kind else {
                    panic!("resolved struct declaration must have struct kind");
                };
                Some(TypeAnnotatedStructDeclaration {
                    name: type_declaration.name.clone(),
                    struct_reference: TypeAnnotatedStructReference {
                        package_path: package_path.to_string(),
                        symbol_name: type_declaration.name.clone(),
                    },
                    type_parameters: type_declaration
                        .type_parameters
                        .iter()
                        .zip(type_info.type_parameters.iter())
                        .map(|(type_parameter, resolved_type_parameter)| {
                            TypeAnnotatedTypeParameter {
                                name: type_parameter.name.clone(),
                                constraint_interface_reference: resolved_type_parameter
                                    .constraint
                                    .as_ref()
                                    .and_then(|constraint| {
                                        type_annotated_interface_reference_from_type(
                                            &resolved_declarations.types_by_name,
                                            constraint,
                                        )
                                    }),
                                span: type_parameter.span.clone(),
                            }
                        })
                        .collect(),
                    implemented_interfaces: implemented_interface_references_by_struct_name
                        .get(&type_declaration.name)
                        .cloned()
                        .unwrap_or_default(),
                    fields: fields
                        .iter()
                        .zip(semantic_fields.iter())
                        .map(|((field_name, field_type), semantic_field)| {
                            TypeAnnotatedStructFieldDeclaration {
                                name: field_name.clone(),
                                type_reference: type_annotated_resolved_type_argument_from_type(
                                    field_type,
                                )
                                .expect("struct field types must be fully resolved"),
                                span: semantic_field.span.clone(),
                            }
                        })
                        .collect(),
                    methods: methods
                        .iter()
                        .map(|method| {
                            let method_key = MethodKey {
                                receiver_type_id: type_info.nominal_type_id.clone(),
                                method_name: method.name.clone(),
                            };
                            let method_info = resolved_declarations
                                .methods_by_key
                                .get(&method_key)
                                .expect("struct method must have resolved signature");
                            TypeAnnotatedMethodDeclaration {
                                name: method.name.clone(),
                                self_mutable: method_info.self_mutable,
                                parameters: method
                                    .parameters
                                    .iter()
                                    .zip(method_info.parameter_types.iter())
                                    .map(|(parameter, resolved_parameter_type)| {
                                        TypeAnnotatedParameterDeclaration {
                                            name: parameter.name.clone(),
                                            mutable: parameter.mutable,
                                            type_reference:
                                                type_annotated_resolved_type_argument_from_type(
                                                    resolved_parameter_type,
                                                )
                                                .expect(
                                                    "method parameter types must be fully resolved",
                                                ),
                                            span: parameter.span.clone(),
                                        }
                                    })
                                    .collect(),
                                return_type_reference:
                                    type_annotated_resolved_type_argument_from_type(
                                        &method_info.return_type,
                                    )
                                    .expect("method return type must be fully resolved"),
                                span: method.span.clone(),
                                statements: method
                                    .body
                                    .statements
                                    .iter()
                                    .map(|statement| {
                                        type_annotated_statement_from_semantic_statement(
                                            statement,
                                            resolved_type_by_expression_id,
                                            call_target_by_expression_id,
                                            resolved_type_argument_types_by_expression_id,
                                            struct_reference_by_expression_id,
                                            enum_variant_reference_by_expression_id,
                                            constant_reference_by_expression_id,
                                        )
                                    })
                                    .collect(),
                            }
                        })
                        .collect(),
                    span: type_declaration.span.clone(),
                })
            }
            compiler__semantic_program::SemanticTypeDeclarationKind::Enum { .. }
            | compiler__semantic_program::SemanticTypeDeclarationKind::Interface { .. }
            | compiler__semantic_program::SemanticTypeDeclarationKind::Union { .. } => None,
        })
        .collect()
}

fn build_interface_declaration_annotations(
    package_path: &str,
    type_declarations: &[SemanticTypeDeclaration],
    resolved_declarations: &ResolvedDeclarations,
) -> Vec<TypeAnnotatedInterfaceDeclaration> {
    type_declarations
        .iter()
        .filter_map(|type_declaration| match &type_declaration.kind {
            compiler__semantic_program::SemanticTypeDeclarationKind::Interface { methods } => {
                let type_info = resolved_declarations
                    .types_by_name
                    .get(&type_declaration.name)
                    .expect("interface declaration must have resolved type info");
                let TypeKind::Interface {
                    methods: interface_methods,
                } = &type_info.kind
                else {
                    panic!("resolved interface declaration must have interface kind");
                };
                Some(TypeAnnotatedInterfaceDeclaration {
                    name: type_declaration.name.clone(),
                    interface_reference: TypeAnnotatedInterfaceReference {
                        package_path: package_path.to_string(),
                        symbol_name: type_declaration.name.clone(),
                    },
                    methods: methods
                        .iter()
                        .zip(interface_methods.iter())
                        .map(|(method, resolved_method)| TypeAnnotatedInterfaceMethodDeclaration {
                            name: method.name.clone(),
                            self_mutable: resolved_method.self_mutable,
                            parameters: method
                                .parameters
                                .iter()
                                .zip(resolved_method.parameter_types.iter())
                                .map(|(parameter, resolved_parameter_type)| {
                                    TypeAnnotatedParameterDeclaration {
                                        name: parameter.name.clone(),
                                        mutable: parameter.mutable,
                                        type_reference: type_annotated_resolved_type_argument_from_type(
                                            resolved_parameter_type,
                                        )
                                        .expect("interface method parameter types must be fully resolved"),
                                        span: parameter.span.clone(),
                                    }
                                })
                                .collect(),
                            return_type_reference: type_annotated_resolved_type_argument_from_type(
                                &resolved_method.return_type,
                            )
                            .expect("interface method return type must be fully resolved"),
                            span: method.span.clone(),
                        })
                        .collect(),
                    span: type_declaration.span.clone(),
                })
            }
            compiler__semantic_program::SemanticTypeDeclarationKind::Struct { .. }
            | compiler__semantic_program::SemanticTypeDeclarationKind::Enum { .. }
            | compiler__semantic_program::SemanticTypeDeclarationKind::Union { .. } => None,
        })
        .collect()
}

fn type_annotated_interface_reference_from_type(
    types_by_name: &HashMap<String, TypeInfo>,
    value_type: &Type,
) -> Option<TypeAnnotatedInterfaceReference> {
    let nominal_type_id = match value_type {
        Type::Named(named) => Some(named.id.clone()),
        Type::Applied { base, .. } => Some(base.id.clone()),
        _ => None,
    }?;
    let type_info = types_by_name
        .values()
        .find(|info| info.nominal_type_id == nominal_type_id)?;
    if !matches!(type_info.kind, TypeKind::Interface { .. }) {
        return None;
    }
    Some(TypeAnnotatedInterfaceReference {
        package_path: type_info.package_path.clone(),
        symbol_name: nominal_type_id.symbol_name,
    })
}

fn type_annotated_statement_from_semantic_statement(
    statement: &SemanticStatement,
    resolved_type_by_expression_id: &BTreeMap<SemanticExpressionId, Type>,
    call_target_by_expression_id: &BTreeMap<SemanticExpressionId, TypeAnnotatedCallTarget>,
    resolved_type_argument_types_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        Vec<TypeAnnotatedResolvedTypeArgument>,
    >,
    struct_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedStructReference,
    >,
    enum_variant_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedEnumVariantReference,
    >,
    constant_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedConstantReference,
    >,
) -> TypeAnnotatedStatement {
    match statement {
        SemanticStatement::Binding {
            name,
            mutable,
            initializer,
            span,
            ..
        } => TypeAnnotatedStatement::Binding {
            name: name.clone(),
            mutable: *mutable,
            initializer: type_annotated_expression_from_semantic_expression(
                initializer,
                resolved_type_by_expression_id,
                call_target_by_expression_id,
                resolved_type_argument_types_by_expression_id,
                struct_reference_by_expression_id,
                enum_variant_reference_by_expression_id,
                constant_reference_by_expression_id,
            ),
            span: span.clone(),
        },
        SemanticStatement::Assign {
            target,
            value,
            span,
        } => TypeAnnotatedStatement::Assign {
            target: type_annotated_assign_target_from_semantic_assign_target(
                target,
                resolved_type_by_expression_id,
                call_target_by_expression_id,
                resolved_type_argument_types_by_expression_id,
                struct_reference_by_expression_id,
                enum_variant_reference_by_expression_id,
                constant_reference_by_expression_id,
            ),
            value: type_annotated_expression_from_semantic_expression(
                value,
                resolved_type_by_expression_id,
                call_target_by_expression_id,
                resolved_type_argument_types_by_expression_id,
                struct_reference_by_expression_id,
                enum_variant_reference_by_expression_id,
                constant_reference_by_expression_id,
            ),
            span: span.clone(),
        },
        SemanticStatement::If {
            condition,
            then_block,
            else_block,
            span,
        } => TypeAnnotatedStatement::If {
            condition: type_annotated_expression_from_semantic_expression(
                condition,
                resolved_type_by_expression_id,
                call_target_by_expression_id,
                resolved_type_argument_types_by_expression_id,
                struct_reference_by_expression_id,
                enum_variant_reference_by_expression_id,
                constant_reference_by_expression_id,
            ),
            then_statements: then_block
                .statements
                .iter()
                .map(|statement| {
                    type_annotated_statement_from_semantic_statement(
                        statement,
                        resolved_type_by_expression_id,
                        call_target_by_expression_id,
                        resolved_type_argument_types_by_expression_id,
                        struct_reference_by_expression_id,
                        enum_variant_reference_by_expression_id,
                        constant_reference_by_expression_id,
                    )
                })
                .collect(),
            else_statements: else_block.as_ref().map(|block| {
                block
                    .statements
                    .iter()
                    .map(|statement| {
                        type_annotated_statement_from_semantic_statement(
                            statement,
                            resolved_type_by_expression_id,
                            call_target_by_expression_id,
                            resolved_type_argument_types_by_expression_id,
                            struct_reference_by_expression_id,
                            enum_variant_reference_by_expression_id,
                            constant_reference_by_expression_id,
                        )
                    })
                    .collect()
            }),
            span: span.clone(),
        },
        SemanticStatement::For {
            condition,
            body,
            span,
        } => TypeAnnotatedStatement::For {
            condition: condition.as_ref().map(|expression| {
                type_annotated_expression_from_semantic_expression(
                    expression,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )
            }),
            body_statements: body
                .statements
                .iter()
                .map(|statement| {
                    type_annotated_statement_from_semantic_statement(
                        statement,
                        resolved_type_by_expression_id,
                        call_target_by_expression_id,
                        resolved_type_argument_types_by_expression_id,
                        struct_reference_by_expression_id,
                        enum_variant_reference_by_expression_id,
                        constant_reference_by_expression_id,
                    )
                })
                .collect(),
            span: span.clone(),
        },
        SemanticStatement::Break { span } => TypeAnnotatedStatement::Break { span: span.clone() },
        SemanticStatement::Continue { span } => {
            TypeAnnotatedStatement::Continue { span: span.clone() }
        }
        SemanticStatement::Expression { value, span } => TypeAnnotatedStatement::Expression {
            value: type_annotated_expression_from_semantic_expression(
                value,
                resolved_type_by_expression_id,
                call_target_by_expression_id,
                resolved_type_argument_types_by_expression_id,
                struct_reference_by_expression_id,
                enum_variant_reference_by_expression_id,
                constant_reference_by_expression_id,
            ),
            span: span.clone(),
        },
        SemanticStatement::Return { value, span } => TypeAnnotatedStatement::Return {
            value: value.as_ref().map_or_else(
                || TypeAnnotatedExpression::NilLiteral { span: span.clone() },
                |value| {
                    type_annotated_expression_from_semantic_expression(
                        value,
                        resolved_type_by_expression_id,
                        call_target_by_expression_id,
                        resolved_type_argument_types_by_expression_id,
                        struct_reference_by_expression_id,
                        enum_variant_reference_by_expression_id,
                        constant_reference_by_expression_id,
                    )
                },
            ),
            span: span.clone(),
        },
    }
}

fn type_annotated_assign_target_from_semantic_assign_target(
    target: &SemanticAssignTarget,
    resolved_type_by_expression_id: &BTreeMap<SemanticExpressionId, Type>,
    call_target_by_expression_id: &BTreeMap<SemanticExpressionId, TypeAnnotatedCallTarget>,
    resolved_type_argument_types_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        Vec<TypeAnnotatedResolvedTypeArgument>,
    >,
    struct_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedStructReference,
    >,
    enum_variant_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedEnumVariantReference,
    >,
    constant_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedConstantReference,
    >,
) -> TypeAnnotatedAssignTarget {
    match target {
        SemanticAssignTarget::Name { name, span, .. } => TypeAnnotatedAssignTarget::Name {
            name: name.clone(),
            span: span.clone(),
        },
        SemanticAssignTarget::Index {
            target,
            index,
            span,
        } => TypeAnnotatedAssignTarget::Index {
            target: Box::new(type_annotated_expression_from_semantic_expression(
                target,
                resolved_type_by_expression_id,
                call_target_by_expression_id,
                resolved_type_argument_types_by_expression_id,
                struct_reference_by_expression_id,
                enum_variant_reference_by_expression_id,
                constant_reference_by_expression_id,
            )),
            index: Box::new(type_annotated_expression_from_semantic_expression(
                index,
                resolved_type_by_expression_id,
                call_target_by_expression_id,
                resolved_type_argument_types_by_expression_id,
                struct_reference_by_expression_id,
                enum_variant_reference_by_expression_id,
                constant_reference_by_expression_id,
            )),
            span: span.clone(),
        },
    }
}

fn type_annotated_expression_from_semantic_expression(
    expression: &SemanticExpression,
    resolved_type_by_expression_id: &BTreeMap<SemanticExpressionId, Type>,
    call_target_by_expression_id: &BTreeMap<SemanticExpressionId, TypeAnnotatedCallTarget>,
    resolved_type_argument_types_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        Vec<TypeAnnotatedResolvedTypeArgument>,
    >,
    struct_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedStructReference,
    >,
    enum_variant_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedEnumVariantReference,
    >,
    constant_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedConstantReference,
    >,
) -> TypeAnnotatedExpression {
    match expression {
        SemanticExpression::IntegerLiteral { value, span, .. } => {
            TypeAnnotatedExpression::IntegerLiteral {
                value: *value,
                span: span.clone(),
            }
        }
        SemanticExpression::BooleanLiteral { value, span, .. } => {
            TypeAnnotatedExpression::BooleanLiteral {
                value: *value,
                span: span.clone(),
            }
        }
        SemanticExpression::NilLiteral { span, .. } => {
            TypeAnnotatedExpression::NilLiteral { span: span.clone() }
        }
        SemanticExpression::StringLiteral { value, span, .. } => {
            TypeAnnotatedExpression::StringLiteral {
                value: value.clone(),
                span: span.clone(),
            }
        }
        SemanticExpression::ListLiteral { elements, span, .. } => {
            TypeAnnotatedExpression::ListLiteral {
                elements: elements
                    .iter()
                    .map(|element| {
                        type_annotated_expression_from_semantic_expression(
                            element,
                            resolved_type_by_expression_id,
                            call_target_by_expression_id,
                            resolved_type_argument_types_by_expression_id,
                            struct_reference_by_expression_id,
                            enum_variant_reference_by_expression_id,
                            constant_reference_by_expression_id,
                        )
                    })
                    .collect(),
                element_type: resolved_type_by_expression_id
                    .get(&semantic_expression_id(expression))
                    .and_then(|resolved_type| match resolved_type {
                        Type::List(element_type) => {
                            type_annotated_resolved_type_argument_from_type(element_type)
                        }
                        _ => None,
                    })
                    .expect("list literal element types must be fully resolved"),
                span: span.clone(),
            }
        }
        SemanticExpression::NameReference {
            name, kind, span, ..
        } => TypeAnnotatedExpression::NameReference {
            name: name.clone(),
            kind: match kind {
                SemanticNameReferenceKind::UserDefined => {
                    TypeAnnotatedNameReferenceKind::UserDefined
                }
                SemanticNameReferenceKind::Builtin => TypeAnnotatedNameReferenceKind::Builtin,
            },
            constant_reference: constant_reference_by_expression_id
                .get(&semantic_expression_id(expression))
                .cloned(),
            callable_reference: call_target_by_expression_id
                .get(&semantic_expression_id(expression))
                .and_then(|call_target| match call_target {
                    TypeAnnotatedCallTarget::UserDefinedFunction { callable_reference } => {
                        Some(callable_reference.clone())
                    }
                    TypeAnnotatedCallTarget::BuiltinFunction { .. } => None,
                }),
            type_reference: resolved_type_by_expression_id
                .get(&semantic_expression_id(expression))
                .and_then(type_annotated_resolved_type_argument_from_type)
                .expect("name reference types must be fully resolved"),
            span: span.clone(),
        },
        SemanticExpression::FieldAccess { span, .. }
            if enum_variant_reference_by_expression_id
                .contains_key(&semantic_expression_id(expression)) =>
        {
            TypeAnnotatedExpression::EnumVariantLiteral {
                enum_variant_reference: enum_variant_reference_by_expression_id
                    .get(&semantic_expression_id(expression))
                    .cloned()
                    .expect("checked by contains_key"),
                span: span.clone(),
            }
        }
        SemanticExpression::StructLiteral {
            type_name,
            fields,
            span,
            ..
        } => TypeAnnotatedExpression::StructLiteral {
            type_name: type_annotated_type_name_from_semantic_type_name(type_name),
            fields: fields
                .iter()
                .map(|field| TypeAnnotatedStructLiteralField {
                    name: field.name.clone(),
                    value: type_annotated_expression_from_semantic_expression(
                        &field.value,
                        resolved_type_by_expression_id,
                        call_target_by_expression_id,
                        resolved_type_argument_types_by_expression_id,
                        struct_reference_by_expression_id,
                        enum_variant_reference_by_expression_id,
                        constant_reference_by_expression_id,
                    ),
                    span: field.span.clone(),
                })
                .collect(),
            struct_reference: struct_reference_by_expression_id
                .get(&semantic_expression_id(expression))
                .cloned(),
            span: span.clone(),
        },
        SemanticExpression::FieldAccess {
            target,
            field,
            span,
            ..
        } => TypeAnnotatedExpression::FieldAccess {
            target: Box::new(type_annotated_expression_from_semantic_expression(
                target,
                resolved_type_by_expression_id,
                call_target_by_expression_id,
                resolved_type_argument_types_by_expression_id,
                struct_reference_by_expression_id,
                enum_variant_reference_by_expression_id,
                constant_reference_by_expression_id,
            )),
            field: field.clone(),
            span: span.clone(),
        },
        SemanticExpression::IndexAccess {
            target,
            index,
            span,
            ..
        } => TypeAnnotatedExpression::IndexAccess {
            target: Box::new(type_annotated_expression_from_semantic_expression(
                target,
                resolved_type_by_expression_id,
                call_target_by_expression_id,
                resolved_type_argument_types_by_expression_id,
                struct_reference_by_expression_id,
                enum_variant_reference_by_expression_id,
                constant_reference_by_expression_id,
            )),
            index: Box::new(type_annotated_expression_from_semantic_expression(
                index,
                resolved_type_by_expression_id,
                call_target_by_expression_id,
                resolved_type_argument_types_by_expression_id,
                struct_reference_by_expression_id,
                enum_variant_reference_by_expression_id,
                constant_reference_by_expression_id,
            )),
            span: span.clone(),
        },
        SemanticExpression::Unary {
            operator,
            expression,
            span,
            ..
        } => TypeAnnotatedExpression::Unary {
            operator: match operator {
                SemanticUnaryOperator::Not => TypeAnnotatedUnaryOperator::Not,
                SemanticUnaryOperator::Negate => TypeAnnotatedUnaryOperator::Negate,
            },
            expression: Box::new(type_annotated_expression_from_semantic_expression(
                expression,
                resolved_type_by_expression_id,
                call_target_by_expression_id,
                resolved_type_argument_types_by_expression_id,
                struct_reference_by_expression_id,
                enum_variant_reference_by_expression_id,
                constant_reference_by_expression_id,
            )),
            span: span.clone(),
        },
        SemanticExpression::Binary {
            operator,
            left,
            right,
            span,
            ..
        } => match operator {
            SemanticBinaryOperator::Add => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::Add,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::Subtract => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::Subtract,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::Multiply => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::Multiply,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::Divide => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::Divide,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::Modulo => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::Modulo,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::EqualEqual => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::EqualEqual,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::NotEqual => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::NotEqual,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::LessThan => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::LessThan,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::LessThanOrEqual => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::LessThanOrEqual,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::GreaterThan => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::GreaterThan,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::GreaterThanOrEqual => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::GreaterThanOrEqual,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::And => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::And,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::Or => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::Or,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    resolved_type_by_expression_id,
                    call_target_by_expression_id,
                    resolved_type_argument_types_by_expression_id,
                    struct_reference_by_expression_id,
                    enum_variant_reference_by_expression_id,
                    constant_reference_by_expression_id,
                )),
                span: span.clone(),
            },
        },
        SemanticExpression::Call {
            callee,
            type_arguments,
            arguments,
            span,
            ..
        } => TypeAnnotatedExpression::Call {
            callee: Box::new(type_annotated_expression_from_semantic_expression(
                callee,
                resolved_type_by_expression_id,
                call_target_by_expression_id,
                resolved_type_argument_types_by_expression_id,
                struct_reference_by_expression_id,
                enum_variant_reference_by_expression_id,
                constant_reference_by_expression_id,
            )),
            call_target: call_target_by_expression_id
                .get(&semantic_expression_id(expression))
                .cloned(),
            arguments: arguments
                .iter()
                .map(|argument| {
                    type_annotated_expression_from_semantic_expression(
                        argument,
                        resolved_type_by_expression_id,
                        call_target_by_expression_id,
                        resolved_type_argument_types_by_expression_id,
                        struct_reference_by_expression_id,
                        enum_variant_reference_by_expression_id,
                        constant_reference_by_expression_id,
                    )
                })
                .collect(),
            type_arguments: type_arguments
                .iter()
                .map(type_annotated_type_name_from_semantic_type_name)
                .collect(),
            resolved_type_arguments: resolved_type_argument_types_by_expression_id
                .get(&semantic_expression_id(expression))
                .cloned()
                .unwrap_or_default(),
            span: span.clone(),
        },
        SemanticExpression::Match {
            target, arms, span, ..
        } => TypeAnnotatedExpression::Match {
            target: Box::new(type_annotated_expression_from_semantic_expression(
                target,
                resolved_type_by_expression_id,
                call_target_by_expression_id,
                resolved_type_argument_types_by_expression_id,
                struct_reference_by_expression_id,
                enum_variant_reference_by_expression_id,
                constant_reference_by_expression_id,
            )),
            arms: arms
                .iter()
                .map(|arm| {
                    type_annotated_match_arm_from_semantic_match_arm(
                        arm,
                        resolved_type_by_expression_id,
                        call_target_by_expression_id,
                        resolved_type_argument_types_by_expression_id,
                        struct_reference_by_expression_id,
                        enum_variant_reference_by_expression_id,
                        constant_reference_by_expression_id,
                    )
                })
                .collect(),
            span: span.clone(),
        },
        SemanticExpression::Matches {
            value,
            type_name,
            span,
            ..
        } => TypeAnnotatedExpression::Matches {
            value: Box::new(type_annotated_expression_from_semantic_expression(
                value,
                resolved_type_by_expression_id,
                call_target_by_expression_id,
                resolved_type_argument_types_by_expression_id,
                struct_reference_by_expression_id,
                enum_variant_reference_by_expression_id,
                constant_reference_by_expression_id,
            )),
            type_name: type_annotated_type_name_from_semantic_type_name(type_name),
            span: span.clone(),
        },
        SemanticExpression::StringInterpolation { parts, span, .. } => {
            use compiler__semantic_program::SemanticStringInterpolationPart;
            TypeAnnotatedExpression::StringInterpolation {
                parts: parts
                    .iter()
                    .map(|part| match part {
                        SemanticStringInterpolationPart::Literal(text) => {
                            TypeAnnotatedStringInterpolationPart::Literal(text.clone())
                        }
                        SemanticStringInterpolationPart::Expression(expression) => {
                            TypeAnnotatedStringInterpolationPart::Expression(Box::new(
                                type_annotated_expression_from_semantic_expression(
                                    expression,
                                    resolved_type_by_expression_id,
                                    call_target_by_expression_id,
                                    resolved_type_argument_types_by_expression_id,
                                    struct_reference_by_expression_id,
                                    enum_variant_reference_by_expression_id,
                                    constant_reference_by_expression_id,
                                ),
                            ))
                        }
                    })
                    .collect(),
                span: span.clone(),
            }
        }
    }
}

fn type_annotated_match_arm_from_semantic_match_arm(
    arm: &compiler__semantic_program::SemanticMatchArm,
    resolved_type_by_expression_id: &BTreeMap<SemanticExpressionId, Type>,
    call_target_by_expression_id: &BTreeMap<SemanticExpressionId, TypeAnnotatedCallTarget>,
    resolved_type_argument_types_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        Vec<TypeAnnotatedResolvedTypeArgument>,
    >,
    struct_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedStructReference,
    >,
    enum_variant_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedEnumVariantReference,
    >,
    constant_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedConstantReference,
    >,
) -> TypeAnnotatedMatchArm {
    TypeAnnotatedMatchArm {
        pattern: type_annotated_match_pattern_from_semantic_match_pattern(&arm.pattern),
        value: type_annotated_expression_from_semantic_expression(
            &arm.value,
            resolved_type_by_expression_id,
            call_target_by_expression_id,
            resolved_type_argument_types_by_expression_id,
            struct_reference_by_expression_id,
            enum_variant_reference_by_expression_id,
            constant_reference_by_expression_id,
        ),
        span: arm.span.clone(),
    }
}

fn type_annotated_match_pattern_from_semantic_match_pattern(
    pattern: &compiler__semantic_program::SemanticMatchPattern,
) -> TypeAnnotatedMatchPattern {
    match pattern {
        compiler__semantic_program::SemanticMatchPattern::Type { type_name, span } => {
            TypeAnnotatedMatchPattern::Type {
                type_name: type_annotated_type_name_from_semantic_type_name(type_name),
                span: span.clone(),
            }
        }
        compiler__semantic_program::SemanticMatchPattern::Binding {
            name,
            type_name,
            span,
            ..
        } => TypeAnnotatedMatchPattern::Binding {
            name: name.clone(),
            type_name: type_annotated_type_name_from_semantic_type_name(type_name),
            span: span.clone(),
        },
    }
}

fn semantic_expression_id(expression: &SemanticExpression) -> SemanticExpressionId {
    match expression {
        SemanticExpression::IntegerLiteral { id, .. }
        | SemanticExpression::NilLiteral { id, .. }
        | SemanticExpression::BooleanLiteral { id, .. }
        | SemanticExpression::StringLiteral { id, .. }
        | SemanticExpression::ListLiteral { id, .. }
        | SemanticExpression::NameReference { id, .. }
        | SemanticExpression::StructLiteral { id, .. }
        | SemanticExpression::FieldAccess { id, .. }
        | SemanticExpression::IndexAccess { id, .. }
        | SemanticExpression::Call { id, .. }
        | SemanticExpression::Unary { id, .. }
        | SemanticExpression::Binary { id, .. }
        | SemanticExpression::Match { id, .. }
        | SemanticExpression::Matches { id, .. }
        | SemanticExpression::StringInterpolation { id, .. } => *id,
    }
}

fn type_annotated_type_name_from_semantic_type_name(
    type_name: &SemanticTypeName,
) -> TypeAnnotatedTypeName {
    TypeAnnotatedTypeName {
        names: type_name
            .names
            .iter()
            .map(|name_segment| TypeAnnotatedTypeNameSegment {
                name: name_segment.name.clone(),
                nominal_type_reference: None,
                type_arguments: name_segment
                    .type_arguments
                    .iter()
                    .map(type_annotated_type_name_from_semantic_type_name)
                    .collect(),
                span: name_segment.span.clone(),
            })
            .collect(),
        span: type_name.span.clone(),
    }
}

fn annotate_nominal_type_references(
    resolved_declarations: &mut TypeResolvedDeclarations,
    nominal_type_reference_by_local_name: &HashMap<String, TypeAnnotatedNominalTypeReference>,
) {
    for constant_declaration in &mut resolved_declarations.constant_declarations {
        annotate_resolved_type_argument_nominal_references(
            &mut constant_declaration.type_reference,
            nominal_type_reference_by_local_name,
        );
        annotate_expression_nominal_references(
            &mut constant_declaration.initializer,
            nominal_type_reference_by_local_name,
        );
    }

    for interface_declaration in &mut resolved_declarations.interface_declarations {
        for method in &mut interface_declaration.methods {
            for parameter in &mut method.parameters {
                annotate_resolved_type_argument_nominal_references(
                    &mut parameter.type_reference,
                    nominal_type_reference_by_local_name,
                );
            }
            annotate_resolved_type_argument_nominal_references(
                &mut method.return_type_reference,
                nominal_type_reference_by_local_name,
            );
        }
    }

    for struct_declaration in &mut resolved_declarations.struct_declarations {
        for field in &mut struct_declaration.fields {
            annotate_resolved_type_argument_nominal_references(
                &mut field.type_reference,
                nominal_type_reference_by_local_name,
            );
        }
        for method in &mut struct_declaration.methods {
            for parameter in &mut method.parameters {
                annotate_resolved_type_argument_nominal_references(
                    &mut parameter.type_reference,
                    nominal_type_reference_by_local_name,
                );
            }
            annotate_resolved_type_argument_nominal_references(
                &mut method.return_type_reference,
                nominal_type_reference_by_local_name,
            );
            for statement in &mut method.statements {
                annotate_statement_nominal_references(
                    statement,
                    nominal_type_reference_by_local_name,
                );
            }
        }
    }

    for function_declaration in &mut resolved_declarations.function_declarations {
        for parameter in &mut function_declaration.parameters {
            annotate_resolved_type_argument_nominal_references(
                &mut parameter.type_reference,
                nominal_type_reference_by_local_name,
            );
        }
        annotate_resolved_type_argument_nominal_references(
            &mut function_declaration.return_type_reference,
            nominal_type_reference_by_local_name,
        );
        for statement in &mut function_declaration.statements {
            annotate_statement_nominal_references(statement, nominal_type_reference_by_local_name);
        }
    }
}

fn annotate_statement_nominal_references(
    statement: &mut TypeAnnotatedStatement,
    nominal_type_reference_by_local_name: &HashMap<String, TypeAnnotatedNominalTypeReference>,
) {
    match statement {
        TypeAnnotatedStatement::Binding { initializer, .. } => {
            annotate_expression_nominal_references(
                initializer,
                nominal_type_reference_by_local_name,
            );
        }
        TypeAnnotatedStatement::Assign { target, value, .. } => {
            match target {
                TypeAnnotatedAssignTarget::Name { .. } => {}
                TypeAnnotatedAssignTarget::Index { target, index, .. } => {
                    annotate_expression_nominal_references(
                        target,
                        nominal_type_reference_by_local_name,
                    );
                    annotate_expression_nominal_references(
                        index,
                        nominal_type_reference_by_local_name,
                    );
                }
            }
            annotate_expression_nominal_references(value, nominal_type_reference_by_local_name);
        }
        TypeAnnotatedStatement::Expression { value, .. }
        | TypeAnnotatedStatement::Return { value, .. } => {
            annotate_expression_nominal_references(value, nominal_type_reference_by_local_name);
        }
        TypeAnnotatedStatement::If {
            condition,
            then_statements,
            else_statements,
            ..
        } => {
            annotate_expression_nominal_references(condition, nominal_type_reference_by_local_name);
            for statement in then_statements {
                annotate_statement_nominal_references(
                    statement,
                    nominal_type_reference_by_local_name,
                );
            }
            if let Some(else_statements) = else_statements {
                for statement in else_statements {
                    annotate_statement_nominal_references(
                        statement,
                        nominal_type_reference_by_local_name,
                    );
                }
            }
        }
        TypeAnnotatedStatement::For {
            condition,
            body_statements,
            ..
        } => {
            if let Some(condition) = condition {
                annotate_expression_nominal_references(
                    condition,
                    nominal_type_reference_by_local_name,
                );
            }
            for statement in body_statements {
                annotate_statement_nominal_references(
                    statement,
                    nominal_type_reference_by_local_name,
                );
            }
        }
        TypeAnnotatedStatement::Break { .. } | TypeAnnotatedStatement::Continue { .. } => {}
    }
}

fn annotate_expression_nominal_references(
    expression: &mut TypeAnnotatedExpression,
    nominal_type_reference_by_local_name: &HashMap<String, TypeAnnotatedNominalTypeReference>,
) {
    match expression {
        TypeAnnotatedExpression::IntegerLiteral { .. }
        | TypeAnnotatedExpression::BooleanLiteral { .. }
        | TypeAnnotatedExpression::NilLiteral { .. }
        | TypeAnnotatedExpression::StringLiteral { .. }
        | TypeAnnotatedExpression::EnumVariantLiteral { .. } => {}
        TypeAnnotatedExpression::NameReference { type_reference, .. } => {
            annotate_resolved_type_argument_nominal_references(
                type_reference,
                nominal_type_reference_by_local_name,
            );
        }
        TypeAnnotatedExpression::ListLiteral { elements, .. } => {
            for element in elements {
                annotate_expression_nominal_references(
                    element,
                    nominal_type_reference_by_local_name,
                );
            }
        }
        TypeAnnotatedExpression::StructLiteral {
            type_name, fields, ..
        } => {
            annotate_type_name_nominal_references(type_name, nominal_type_reference_by_local_name);
            for field in fields {
                annotate_expression_nominal_references(
                    &mut field.value,
                    nominal_type_reference_by_local_name,
                );
            }
        }
        TypeAnnotatedExpression::FieldAccess { target, .. } => {
            annotate_expression_nominal_references(target, nominal_type_reference_by_local_name);
        }
        TypeAnnotatedExpression::IndexAccess { target, index, .. } => {
            annotate_expression_nominal_references(target, nominal_type_reference_by_local_name);
            annotate_expression_nominal_references(index, nominal_type_reference_by_local_name);
        }
        TypeAnnotatedExpression::Unary { expression, .. } => {
            annotate_expression_nominal_references(
                expression,
                nominal_type_reference_by_local_name,
            );
        }
        TypeAnnotatedExpression::Binary { left, right, .. } => {
            annotate_expression_nominal_references(left, nominal_type_reference_by_local_name);
            annotate_expression_nominal_references(right, nominal_type_reference_by_local_name);
        }
        TypeAnnotatedExpression::Call {
            callee,
            arguments,
            type_arguments,
            resolved_type_arguments,
            ..
        } => {
            annotate_expression_nominal_references(callee, nominal_type_reference_by_local_name);
            for argument in arguments {
                annotate_expression_nominal_references(
                    argument,
                    nominal_type_reference_by_local_name,
                );
            }
            for type_argument in type_arguments {
                annotate_type_name_nominal_references(
                    type_argument,
                    nominal_type_reference_by_local_name,
                );
            }
            for resolved_type_argument in resolved_type_arguments {
                annotate_resolved_type_argument_nominal_references(
                    resolved_type_argument,
                    nominal_type_reference_by_local_name,
                );
            }
        }
        TypeAnnotatedExpression::Match { target, arms, .. } => {
            annotate_expression_nominal_references(target, nominal_type_reference_by_local_name);
            for arm in arms {
                annotate_match_pattern_nominal_references(
                    &mut arm.pattern,
                    nominal_type_reference_by_local_name,
                );
                annotate_expression_nominal_references(
                    &mut arm.value,
                    nominal_type_reference_by_local_name,
                );
            }
        }
        TypeAnnotatedExpression::Matches {
            value, type_name, ..
        } => {
            annotate_expression_nominal_references(value, nominal_type_reference_by_local_name);
            annotate_type_name_nominal_references(type_name, nominal_type_reference_by_local_name);
        }
        TypeAnnotatedExpression::StringInterpolation { parts, .. } => {
            for part in parts {
                if let TypeAnnotatedStringInterpolationPart::Expression(expression) = part {
                    annotate_expression_nominal_references(
                        expression,
                        nominal_type_reference_by_local_name,
                    );
                }
            }
        }
    }
}

fn annotate_resolved_type_argument_nominal_references(
    resolved_type_argument: &mut TypeAnnotatedResolvedTypeArgument,
    nominal_type_reference_by_local_name: &HashMap<String, TypeAnnotatedNominalTypeReference>,
) {
    match resolved_type_argument {
        TypeAnnotatedResolvedTypeArgument::Int64
        | TypeAnnotatedResolvedTypeArgument::Boolean
        | TypeAnnotatedResolvedTypeArgument::String
        | TypeAnnotatedResolvedTypeArgument::Nil
        | TypeAnnotatedResolvedTypeArgument::Never
        | TypeAnnotatedResolvedTypeArgument::TypeParameter { .. } => {}
        TypeAnnotatedResolvedTypeArgument::List { element_type } => {
            annotate_resolved_type_argument_nominal_references(
                element_type,
                nominal_type_reference_by_local_name,
            );
        }
        TypeAnnotatedResolvedTypeArgument::Function {
            parameter_types,
            return_type,
        } => {
            for parameter_type in parameter_types {
                annotate_resolved_type_argument_nominal_references(
                    parameter_type,
                    nominal_type_reference_by_local_name,
                );
            }
            annotate_resolved_type_argument_nominal_references(
                return_type,
                nominal_type_reference_by_local_name,
            );
        }
        TypeAnnotatedResolvedTypeArgument::Union { members } => {
            for member in members {
                annotate_resolved_type_argument_nominal_references(
                    member,
                    nominal_type_reference_by_local_name,
                );
            }
        }
        TypeAnnotatedResolvedTypeArgument::NominalTypeApplication {
            base_nominal_type_reference,
            base_name,
            arguments,
        } => {
            *base_nominal_type_reference =
                nominal_type_reference_by_local_name.get(base_name).cloned();
            for argument in arguments {
                annotate_resolved_type_argument_nominal_references(
                    argument,
                    nominal_type_reference_by_local_name,
                );
            }
        }
        TypeAnnotatedResolvedTypeArgument::NominalType {
            nominal_type_reference,
            name,
        } => {
            *nominal_type_reference = nominal_type_reference_by_local_name.get(name).cloned();
        }
    }
}

fn annotate_match_pattern_nominal_references(
    pattern: &mut TypeAnnotatedMatchPattern,
    nominal_type_reference_by_local_name: &HashMap<String, TypeAnnotatedNominalTypeReference>,
) {
    match pattern {
        TypeAnnotatedMatchPattern::Type { type_name, .. }
        | TypeAnnotatedMatchPattern::Binding { type_name, .. } => {
            annotate_type_name_nominal_references(type_name, nominal_type_reference_by_local_name);
        }
    }
}

fn annotate_type_name_nominal_references(
    type_name: &mut TypeAnnotatedTypeName,
    nominal_type_reference_by_local_name: &HashMap<String, TypeAnnotatedNominalTypeReference>,
) {
    for segment in &mut type_name.names {
        segment.nominal_type_reference = nominal_type_reference_by_local_name
            .get(&segment.name)
            .cloned();
        for type_argument in &mut segment.type_arguments {
            annotate_type_name_nominal_references(
                type_argument,
                nominal_type_reference_by_local_name,
            );
        }
    }
}

fn type_annotated_resolved_type_argument_from_type(
    value_type: &Type,
) -> Option<TypeAnnotatedResolvedTypeArgument> {
    Some(match value_type {
        Type::Integer64 => TypeAnnotatedResolvedTypeArgument::Int64,
        Type::Boolean => TypeAnnotatedResolvedTypeArgument::Boolean,
        Type::String => TypeAnnotatedResolvedTypeArgument::String,
        Type::Nil => TypeAnnotatedResolvedTypeArgument::Nil,
        Type::Never => TypeAnnotatedResolvedTypeArgument::Never,
        Type::List(element_type) => TypeAnnotatedResolvedTypeArgument::List {
            element_type: Box::new(type_annotated_resolved_type_argument_from_type(
                element_type,
            )?),
        },
        Type::Function {
            parameter_types,
            return_type,
        } => TypeAnnotatedResolvedTypeArgument::Function {
            parameter_types: parameter_types
                .iter()
                .map(type_annotated_resolved_type_argument_from_type)
                .collect::<Option<Vec<_>>>()?,
            return_type: Box::new(type_annotated_resolved_type_argument_from_type(
                return_type,
            )?),
        },
        Type::Named(named) => TypeAnnotatedResolvedTypeArgument::NominalType {
            nominal_type_reference: None,
            name: named.display_name.clone(),
        },
        Type::TypeParameter(name) => {
            TypeAnnotatedResolvedTypeArgument::TypeParameter { name: name.clone() }
        }
        Type::Applied { base, arguments } => {
            TypeAnnotatedResolvedTypeArgument::NominalTypeApplication {
                base_nominal_type_reference: None,
                base_name: base.display_name.clone(),
                arguments: arguments
                    .iter()
                    .map(type_annotated_resolved_type_argument_from_type)
                    .collect::<Option<Vec<_>>>()?,
            }
        }
        Type::Union(members) => TypeAnnotatedResolvedTypeArgument::Union {
            members: members
                .iter()
                .map(type_annotated_resolved_type_argument_from_type)
                .collect::<Option<Vec<_>>>()?,
        },
        Type::Unknown => return None,
    })
}

fn analyze_package_unit(
    package_id: PackageId,
    package_path: &str,
    source_text: &str,
    package_unit: &SemanticFile,
    imported_bindings: &[ImportedBinding],
    diagnostics: &mut Vec<PhaseDiagnostic>,
    safe_autofixes: &mut Vec<SafeAutofix>,
) -> TypeAnalysisSummary {
    check_package_unit_declarations(
        package_id,
        package_path,
        source_text,
        package_unit,
        imported_bindings,
        diagnostics,
        safe_autofixes,
    )
}

fn check_package_unit_declarations(
    package_id: PackageId,
    package_path: &str,
    source_text: &str,
    package_unit: &SemanticFile,
    imported_bindings: &[ImportedBinding],
    diagnostics: &mut Vec<PhaseDiagnostic>,
    safe_autofixes: &mut Vec<SafeAutofix>,
) -> TypeAnalysisSummary {
    let mut type_declarations = Vec::new();
    let mut constant_declarations = Vec::new();
    let mut function_declarations = Vec::new();
    for declaration in &package_unit.declarations {
        match declaration {
            SemanticDeclaration::Type(type_declaration) => {
                type_declarations.push(type_declaration.clone());
            }
            SemanticDeclaration::Constant(constant_declaration) => {
                constant_declarations.push(constant_declaration.clone());
            }
            SemanticDeclaration::Function(function_declaration) => {
                function_declarations.push(function_declaration.clone());
            }
        }
    }

    let mut summary = check_declarations(
        package_id,
        package_path,
        source_text,
        diagnostics,
        safe_autofixes,
        &type_declarations,
        &constant_declarations,
        &function_declarations,
        imported_bindings,
    );
    summary.type_declarations_for_annotations = type_declarations;
    summary.constant_declarations_for_annotations = constant_declarations;
    summary.function_declarations_for_annotations = function_declarations;
    summary
}

fn check_declarations(
    package_id: PackageId,
    package_path: &str,
    source_text: &str,
    diagnostics: &mut Vec<PhaseDiagnostic>,
    safe_autofixes: &mut Vec<SafeAutofix>,
    type_declarations: &[SemanticTypeDeclaration],
    constant_declarations: &[SemanticConstantDeclaration],
    function_declarations: &[SemanticFunctionDeclaration],
    imported_bindings: &[ImportedBinding],
) -> TypeAnalysisSummary {
    let mut type_checker = TypeChecker::new(
        package_id,
        package_path,
        source_text,
        imported_bindings,
        diagnostics,
        safe_autofixes,
    );
    type_checker.collect_imported_type_declarations();
    type_checker.collect_type_declarations(type_declarations);
    type_checker.collect_imported_function_signatures();
    type_checker.collect_function_signatures(function_declarations);
    type_checker.collect_imported_method_signatures();
    type_checker.collect_method_signatures(type_declarations);
    type_checker.check_type_interface_conformance(type_declarations);
    type_checker.check_constant_declarations(constant_declarations);
    for function in function_declarations {
        type_checker.check_function(function);
    }
    type_checker.check_methods(type_declarations);
    type_checker.check_unused_imports();
    type_checker.build_summary(
        type_declarations,
        function_declarations,
        constant_declarations,
    )
}

struct VariableInfo {
    value_type: Type,
    used: bool,
    mutable: bool,
    name_span: Span,
}

struct ConstantInfo {
    value_type: Type,
}

struct ImportedBindingInfo {
    symbol: ImportedSymbol,
    span: Span,
    imported_package_path: String,
    imported_symbol_name: String,
    used: bool,
}

#[derive(Clone)]
struct TypeInfo {
    nominal_type_id: NominalTypeId,
    package_path: String,
    type_parameters: Vec<GenericTypeParameter>,
    implemented_interface_entries: Vec<ImplementedInterfaceEntry>,
    kind: TypeKind,
}

#[derive(Clone)]
enum TypeKind {
    Struct {
        fields: Vec<(String, Type)>,
    },
    Interface {
        methods: Vec<InterfaceMethodSignature>,
    },
    Union {
        variants: Vec<Type>,
    },
}

#[derive(Clone)]
struct ImplementedInterfaceEntry {
    source_span: Option<Span>,
    resolved_type: Type,
}

#[derive(Clone)]
struct InterfaceMethodSignature {
    name: String,
    self_mutable: bool,
    parameter_types: Vec<Type>,
    return_type: Type,
}

#[derive(Clone)]
struct FunctionInfo {
    type_parameters: Vec<GenericTypeParameter>,
    parameter_types: Vec<Type>,
    return_type: Type,
    call_target: TypeAnnotatedCallTarget,
}

struct MethodInfo {
    self_mutable: bool,
    parameter_types: Vec<Type>,
    return_type: Type,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct MethodKey {
    receiver_type_id: NominalTypeId,
    method_name: String,
}

struct TypeChecker<'a> {
    package_id: PackageId,
    package_path: String,
    source_text: &'a str,
    constants: HashMap<String, ConstantInfo>,
    types: HashMap<String, TypeInfo>,
    functions: HashMap<String, FunctionInfo>,
    imported_functions: HashMap<String, FunctionInfo>,
    imported_bindings: HashMap<String, ImportedBindingInfo>,
    methods: HashMap<MethodKey, MethodInfo>,
    scopes: Vec<HashMap<String, VariableInfo>>,
    type_parameter_scopes: Vec<HashMap<String, Span>>,
    diagnostics: &'a mut Vec<PhaseDiagnostic>,
    safe_autofixes: &'a mut Vec<SafeAutofix>,
    current_return_type: Type,
    loop_depth: usize,
    resolved_type_by_expression_id: BTreeMap<SemanticExpressionId, Type>,
    call_target_by_expression_id: BTreeMap<SemanticExpressionId, TypeAnnotatedCallTarget>,
    constant_reference_by_expression_id:
        BTreeMap<SemanticExpressionId, TypeAnnotatedConstantReference>,
    resolved_type_argument_types_by_expression_id:
        BTreeMap<SemanticExpressionId, Vec<TypeAnnotatedResolvedTypeArgument>>,
    struct_reference_by_expression_id: BTreeMap<SemanticExpressionId, TypeAnnotatedStructReference>,
    enum_variant_reference_by_expression_id:
        BTreeMap<SemanticExpressionId, TypeAnnotatedEnumVariantReference>,
}

struct BranchNarrowing {
    name: String,
    when_true: Type,
    when_false: Type,
}

struct StatementOutcome {
    terminates: bool,
    fallthrough_narrowing: Option<FallthroughNarrowing>,
}

struct FallthroughNarrowing {
    variable_name: String,
    narrowed_type: Type,
}

trait ExpressionSpan {
    fn span(&self) -> Span;
}

trait StatementSpan {
    fn span(&self) -> Span;
}

impl<'a> TypeChecker<'a> {
    fn new(
        package_id: PackageId,
        package_path: &str,
        source_text: &'a str,
        imported_bindings: &[ImportedBinding],
        diagnostics: &'a mut Vec<PhaseDiagnostic>,
        safe_autofixes: &'a mut Vec<SafeAutofix>,
    ) -> Self {
        let mut imported_binding_map = HashMap::new();
        for imported in imported_bindings {
            imported_binding_map.insert(
                imported.local_name.clone(),
                ImportedBindingInfo {
                    symbol: imported.symbol.clone(),
                    span: imported.span.clone(),
                    imported_package_path: imported.imported_package_path.clone(),
                    imported_symbol_name: imported.imported_symbol_name.clone(),
                    used: false,
                },
            );
        }
        Self {
            package_id,
            package_path: package_path.to_string(),
            source_text,
            constants: HashMap::new(),
            types: HashMap::new(),
            functions: builtin_functions(),
            imported_functions: HashMap::new(),
            imported_bindings: imported_binding_map,
            methods: HashMap::new(),
            scopes: Vec::new(),
            type_parameter_scopes: Vec::new(),
            diagnostics,
            safe_autofixes,
            current_return_type: Type::Unknown,
            loop_depth: 0,
            resolved_type_by_expression_id: BTreeMap::new(),
            call_target_by_expression_id: BTreeMap::new(),
            constant_reference_by_expression_id: BTreeMap::new(),
            resolved_type_argument_types_by_expression_id: BTreeMap::new(),
            struct_reference_by_expression_id: BTreeMap::new(),
            enum_variant_reference_by_expression_id: BTreeMap::new(),
        }
    }

    fn build_summary(
        self,
        type_declarations: &[SemanticTypeDeclaration],
        _function_declarations: &[SemanticFunctionDeclaration],
        _constant_declarations: &[SemanticConstantDeclaration],
    ) -> TypeAnalysisSummary {
        let nominal_type_reference_by_local_name = self.nominal_type_reference_by_local_name();
        let implemented_interface_references_by_struct_name =
            self.implemented_interface_references_by_struct_name(type_declarations);

        TypeAnalysisSummary {
            resolved_type_by_expression_id: self.resolved_type_by_expression_id,
            call_target_by_expression_id: self.call_target_by_expression_id,
            constant_reference_by_expression_id: self.constant_reference_by_expression_id,
            resolved_type_argument_types_by_expression_id: self
                .resolved_type_argument_types_by_expression_id,
            struct_reference_by_expression_id: self.struct_reference_by_expression_id,
            enum_variant_reference_by_expression_id: self.enum_variant_reference_by_expression_id,
            nominal_type_reference_by_local_name,
            implemented_interface_references_by_struct_name,
            type_declarations_for_annotations: Vec::new(),
            constant_declarations_for_annotations: Vec::new(),
            function_declarations_for_annotations: Vec::new(),
            resolved_declarations: ResolvedDeclarations {
                constants_by_name: self.constants,
                functions_by_name: self.functions,
                types_by_name: self.types,
                methods_by_key: self.methods,
            },
        }
    }

    fn nominal_type_reference_by_local_name(
        &self,
    ) -> HashMap<String, TypeAnnotatedNominalTypeReference> {
        let mut nominal_type_reference_by_local_name = HashMap::new();
        for (local_name, type_info) in &self.types {
            nominal_type_reference_by_local_name.insert(
                local_name.clone(),
                TypeAnnotatedNominalTypeReference {
                    package_path: type_info.package_path.clone(),
                    symbol_name: type_info.nominal_type_id.symbol_name.clone(),
                },
            );
        }
        nominal_type_reference_by_local_name
    }

    fn implemented_interface_references_by_struct_name(
        &self,
        type_declarations: &[SemanticTypeDeclaration],
    ) -> HashMap<String, Vec<TypeAnnotatedInterfaceReference>> {
        let mut implemented_interface_references_by_struct_name = HashMap::new();
        for type_declaration in type_declarations {
            let compiler__semantic_program::SemanticTypeDeclarationKind::Struct { .. } =
                &type_declaration.kind
            else {
                continue;
            };
            let Some(type_info) = self.types.get(&type_declaration.name) else {
                continue;
            };
            let interface_references = type_info
                .implemented_interface_entries
                .iter()
                .filter_map(|entry| {
                    self.type_annotated_interface_reference_from_type(&entry.resolved_type)
                })
                .collect::<Vec<_>>();
            implemented_interface_references_by_struct_name
                .insert(type_declaration.name.clone(), interface_references);
        }
        implemented_interface_references_by_struct_name
    }

    fn type_annotated_interface_reference_from_type(
        &self,
        value_type: &Type,
    ) -> Option<TypeAnnotatedInterfaceReference> {
        let nominal_type_id = Self::nominal_type_id_for_type(value_type)?;
        let type_info = self.type_info_by_nominal_type_id(&nominal_type_id)?;
        if !matches!(type_info.kind, TypeKind::Interface { .. }) {
            return None;
        }
        Some(TypeAnnotatedInterfaceReference {
            package_path: type_info.package_path.clone(),
            symbol_name: nominal_type_id.symbol_name.clone(),
        })
    }

    fn imported_constant_type(&self, name: &str) -> Option<Type> {
        let binding = self.imported_bindings.get(name)?;
        match &binding.symbol {
            ImportedSymbol::Constant(value_type) => Some(value_type.clone()),
            ImportedSymbol::Type(_) | ImportedSymbol::Function(_) => None,
        }
    }

    fn type_info_by_nominal_type_id(&self, nominal_type_id: &NominalTypeId) -> Option<&TypeInfo> {
        self.types
            .values()
            .find(|info| info.nominal_type_id == *nominal_type_id)
    }

    fn nominal_type_id_for_type(value_type: &Type) -> Option<NominalTypeId> {
        match value_type {
            Type::Named(named) => Some(named.id.clone()),
            Type::Applied { base, .. } => Some(base.id.clone()),
            _ => None,
        }
    }

    fn mark_import_used(&mut self, name: &str) {
        if let Some(binding) = self.imported_bindings.get_mut(name) {
            binding.used = true;
        }
    }

    fn define_variable(
        &mut self,
        name: String,
        value_type: Type,
        mutable: bool,
        span: &Span,
        name_span: Span,
    ) {
        let duplicate = self
            .scopes
            .last()
            .is_some_and(|scope| scope.contains_key(&name));
        if duplicate {
            self.error(format!("duplicate binding '{name}'"), span.clone());
        }
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(
                name,
                VariableInfo {
                    value_type,
                    used: false,
                    mutable,
                    name_span,
                },
            );
        }
    }

    fn name_reference_expression_is_callable(
        &self,
        name: &str,
        kind: SemanticNameReferenceKind,
    ) -> bool {
        kind == SemanticNameReferenceKind::Builtin
            || self.functions.contains_key(name)
            || self.imported_functions.contains_key(name)
    }

    fn name_reference_resolves_to_value_binding(&self, name: &str) -> bool {
        self.lookup_variable_type(name).is_some()
            || self.constants.contains_key(name)
            || self.imported_constant_type(name).is_some()
    }

    fn check_name_reference_expression(
        &mut self,
        expression_id: SemanticExpressionId,
        name: &str,
        kind: SemanticNameReferenceKind,
        span: &Span,
    ) -> Type {
        if self.name_reference_expression_is_callable(name, kind) {
            let function_info = if kind == SemanticNameReferenceKind::Builtin {
                self.functions.get(name).cloned()
            } else if let Some(imported_function_info) = self.imported_functions.get(name).cloned()
            {
                self.mark_import_used(name);
                Some(imported_function_info)
            } else {
                self.functions.get(name).cloned()
            };
            if let Some(function_info) = function_info {
                if !function_info.type_parameters.is_empty() {
                    self.error(
                        format!("generic function '{name}' cannot be used as a value"),
                        span.clone(),
                    );
                    return Type::Unknown;
                }
                self.call_target_by_expression_id
                    .insert(expression_id, function_info.call_target.clone());
                return Type::Function {
                    parameter_types: function_info.parameter_types,
                    return_type: Box::new(function_info.return_type),
                };
            }
        }
        self.resolve_variable(expression_id, name, span)
    }

    fn resolve_variable(
        &mut self,
        expression_id: SemanticExpressionId,
        name: &str,
        span: &Span,
    ) -> Type {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(info) = scope.get_mut(name) {
                info.used = true;
                return info.value_type.clone();
            }
        }
        if let Some(info) = self.constants.get(name) {
            self.constant_reference_by_expression_id.insert(
                expression_id,
                TypeAnnotatedConstantReference {
                    package_path: self.package_path.clone(),
                    symbol_name: name.to_string(),
                },
            );
            return info.value_type.clone();
        }
        if let Some(imported_binding) = self.imported_bindings.get(name) {
            let ImportedSymbol::Constant(value_type) = &imported_binding.symbol else {
                // Not a constant binding; continue to unknown-name handling.
                if self.imported_bindings.contains_key(name) {
                    self.mark_import_used(name);
                }
                self.error(format!("unknown name '{name}'"), span.clone());
                return Type::Unknown;
            };
            let imported_package_path = imported_binding.imported_package_path.clone();
            let imported_symbol_name = imported_binding.imported_symbol_name.clone();
            let value_type = value_type.clone();
            self.constant_reference_by_expression_id.insert(
                expression_id,
                TypeAnnotatedConstantReference {
                    package_path: imported_package_path,
                    symbol_name: imported_symbol_name,
                },
            );
            self.mark_import_used(name);
            return value_type;
        }
        if self.imported_bindings.contains_key(name) {
            self.mark_import_used(name);
        }
        self.error(format!("unknown name '{name}'"), span.clone());
        Type::Unknown
    }

    fn lookup_variable_for_assignment(&mut self, name: &str) -> Option<(bool, Type)> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(info) = scope.get_mut(name) {
                info.used = true;
                return Some((info.mutable, info.value_type.clone()));
            }
        }
        None
    }

    fn error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics.push(PhaseDiagnostic::new(message, span));
    }

    fn push_safe_autofix(&mut self, safe_autofix: SafeAutofix) {
        self.safe_autofixes.push(safe_autofix);
    }

    fn enclosing_interpolation_expression_range(
        &self,
        expression_span: &Span,
    ) -> Option<(usize, usize)> {
        if expression_span.start > expression_span.end
            || expression_span.end > self.source_text.len()
        {
            return None;
        }

        let bytes = self.source_text.as_bytes();
        let mut start = expression_span.start;
        while start > 0 && bytes[start - 1].is_ascii_whitespace() {
            start -= 1;
        }
        if start == 0 || bytes[start - 1] != b'{' {
            return None;
        }
        let replacement_start = start - 1;

        let mut end = expression_span.end;
        while end < bytes.len() && bytes[end].is_ascii_whitespace() {
            end += 1;
        }
        if end >= bytes.len() || bytes[end] != b'}' {
            return None;
        }
        let replacement_end = end + 1;
        Some((replacement_start, replacement_end))
    }

    fn push_type_parameters(&mut self, names_and_spans: &[(String, Span)]) {
        let mut scope = HashMap::new();
        for (name, span) in names_and_spans {
            self.check_type_name(name, span);
            if scope.contains_key(name) {
                self.error(format!("duplicate type parameter '{name}'"), span.clone());
                continue;
            }
            scope.insert(name.clone(), span.clone());
        }
        self.type_parameter_scopes.push(scope);
    }

    fn pop_type_parameters(&mut self) {
        self.type_parameter_scopes.pop();
    }

    fn resolve_type_parameter(&self, name: &str) -> Option<Type> {
        for scope in self.type_parameter_scopes.iter().rev() {
            if scope.contains_key(name) {
                return Some(Type::TypeParameter(name.to_string()));
            }
        }
        None
    }

    fn instantiate_type(value_type: &Type, substitutions: &HashMap<String, Type>) -> Type {
        match value_type {
            Type::TypeParameter(name) => substitutions
                .get(name)
                .cloned()
                .unwrap_or_else(|| value_type.clone()),
            Type::Function {
                parameter_types,
                return_type,
            } => Type::Function {
                parameter_types: parameter_types
                    .iter()
                    .map(|parameter_type| Self::instantiate_type(parameter_type, substitutions))
                    .collect(),
                return_type: Box::new(Self::instantiate_type(return_type, substitutions)),
            },
            Type::Union(inner) => {
                let instantiated = inner
                    .iter()
                    .map(|inner_type| Self::instantiate_type(inner_type, substitutions))
                    .collect();
                Self::normalize_union(instantiated)
            }
            Type::Applied { base, arguments } => Type::Applied {
                base: base.clone(),
                arguments: arguments
                    .iter()
                    .map(|argument| Self::instantiate_type(argument, substitutions))
                    .collect(),
            },
            _ => value_type.clone(),
        }
    }

    fn check_type_argument_constraints(
        &mut self,
        context_name: &str,
        type_parameters: &[GenericTypeParameter],
        resolved_type_arguments: &[Type],
        span: &Span,
    ) {
        for (type_parameter, type_argument) in type_parameters.iter().zip(resolved_type_arguments) {
            let Some(constraint) = &type_parameter.constraint else {
                continue;
            };
            if *type_argument == Type::Unknown || *constraint == Type::Unknown {
                continue;
            }
            if !self.is_assignable(type_argument, constraint) {
                self.error(
                    format!(
                        "type argument '{}' does not satisfy constraint '{}' for type parameter '{}' on '{}'",
                        type_argument.display(),
                        constraint.display(),
                        type_parameter.name,
                        context_name
                    ),
                    span.clone(),
                );
            }
        }
    }

    fn resolve_type_name(&mut self, type_name: &SemanticTypeName) -> Type {
        let mut resolved = Vec::new();
        let mut has_unknown = false;
        for segment in &type_name.names {
            let name = segment.name.as_str();
            if name == "function" {
                if segment.type_arguments.is_empty() {
                    self.error(
                        "function type must include a return type",
                        segment.span.clone(),
                    );
                    has_unknown = true;
                    continue;
                }
                let resolved_type_arguments = segment
                    .type_arguments
                    .iter()
                    .map(|argument| self.resolve_type_name(argument))
                    .collect::<Vec<_>>();
                if resolved_type_arguments.contains(&Type::Unknown) {
                    has_unknown = true;
                    continue;
                }
                let mut resolved_type_arguments = resolved_type_arguments;
                let return_type = resolved_type_arguments
                    .pop()
                    .expect("function type arguments must include return type");
                resolved.push(Type::Function {
                    parameter_types: resolved_type_arguments,
                    return_type: Box::new(return_type),
                });
                continue;
            }
            if let Some(type_parameter) = self.resolve_type_parameter(name) {
                if !segment.type_arguments.is_empty() {
                    self.error(
                        format!("type parameter '{name}' does not take type arguments"),
                        segment.span.clone(),
                    );
                    has_unknown = true;
                    continue;
                }
                resolved.push(type_parameter);
                continue;
            }
            if name == "List" {
                if segment.type_arguments.len() != 1 {
                    self.error(
                        format!(
                            "built-in type 'List' expects 1 type argument, got {}",
                            segment.type_arguments.len()
                        ),
                        segment.span.clone(),
                    );
                    has_unknown = true;
                    continue;
                }
                let element_type = self.resolve_type_name(&segment.type_arguments[0]);
                if element_type == Type::Unknown {
                    has_unknown = true;
                    continue;
                }
                resolved.push(Type::List(Box::new(element_type)));
                continue;
            }
            if let Some(builtin) = type_from_builtin_name(name) {
                if !segment.type_arguments.is_empty() {
                    self.error(
                        format!("built-in type '{name}' does not take type arguments"),
                        segment.span.clone(),
                    );
                    has_unknown = true;
                    continue;
                }
                resolved.push(builtin);
                continue;
            }
            if let Some(info) = self.types.get(name) {
                let nominal_type_id = info.nominal_type_id.clone();
                let declared_type_parameters = info.type_parameters.clone();
                let union_variants = match &info.kind {
                    TypeKind::Union { variants } => Some(variants.clone()),
                    TypeKind::Struct { .. } | TypeKind::Interface { .. } => None,
                };
                let type_parameter_count = declared_type_parameters.len();
                if matches!(
                    self.imported_bindings.get(name),
                    Some(ImportedBindingInfo {
                        symbol: ImportedSymbol::Type(_),
                        ..
                    })
                ) {
                    self.mark_import_used(name);
                }
                let resolved_type_arguments = segment
                    .type_arguments
                    .iter()
                    .map(|argument| self.resolve_type_name(argument))
                    .collect::<Vec<_>>();
                if segment.type_arguments.len() != type_parameter_count {
                    if type_parameter_count == 0 {
                        self.error(
                            format!("type '{name}' does not take type arguments"),
                            segment.span.clone(),
                        );
                    } else {
                        self.error(
                            format!(
                                "type '{name}' expects {type_parameter_count} type arguments, got {}",
                                segment.type_arguments.len()
                            ),
                            segment.span.clone(),
                        );
                    }
                    has_unknown = true;
                    continue;
                }
                self.check_type_argument_constraints(
                    name,
                    &declared_type_parameters,
                    &resolved_type_arguments,
                    &segment.span,
                );
                let nominal = NominalTypeRef {
                    id: nominal_type_id,
                    display_name: name.to_string(),
                };
                let Some(variants) = union_variants else {
                    if type_parameter_count == 0 {
                        resolved.push(Type::Named(nominal));
                    } else {
                        resolved.push(Type::Applied {
                            base: nominal,
                            arguments: resolved_type_arguments,
                        });
                    }
                    continue;
                };
                if type_parameter_count == 0 {
                    resolved.push(Self::normalize_union(variants));
                } else {
                    let substitutions: HashMap<String, Type> = declared_type_parameters
                        .iter()
                        .map(|parameter| parameter.name.clone())
                        .zip(resolved_type_arguments.iter().cloned())
                        .collect();
                    let instantiated_variants = variants
                        .iter()
                        .map(|variant| Self::instantiate_type(variant, &substitutions))
                        .collect();
                    resolved.push(Self::normalize_union(instantiated_variants));
                }
                continue;
            }
            if let Some((enum_name, variant_name)) = name.split_once('.')
                && let Some(variant_type) = self.resolve_enum_variant_type(enum_name, variant_name)
            {
                if !segment.type_arguments.is_empty() {
                    self.error(
                        format!("enum variant '{name}' does not take type arguments"),
                        segment.span.clone(),
                    );
                    has_unknown = true;
                    continue;
                }
                resolved.push(variant_type);
                continue;
            }
            self.error(format!("unknown type '{name}'"), segment.span.clone());
            has_unknown = true;
        }

        if has_unknown {
            return Type::Unknown;
        }

        if resolved.len() == 1 {
            return resolved.remove(0);
        }
        Self::normalize_union(resolved)
    }

    pub(crate) fn resolve_enum_variant_type(
        &mut self,
        enum_name: &str,
        variant_name: &str,
    ) -> Option<Type> {
        let info = self.types.get(enum_name)?;
        let TypeKind::Union { variants } = &info.kind else {
            return None;
        };
        let variant_display = format!("{enum_name}.{variant_name}");
        let resolved_variant = variants
            .iter()
            .find(|variant| variant.display() == variant_display)
            .cloned();
        if matches!(
            self.imported_bindings.get(enum_name),
            Some(ImportedBindingInfo {
                symbol: ImportedSymbol::Type(_),
                ..
            })
        ) {
            self.mark_import_used(enum_name);
        }
        resolved_variant
    }

    fn check_unused_imports(&mut self) {
        let mut unused = Vec::new();
        for (name, binding) in &self.imported_bindings {
            if !binding.used {
                unused.push((name.clone(), binding.span.clone()));
            }
        }
        for (name, span) in unused {
            self.error(format!("unused import '{name}'"), span);
        }
    }
}

fn builtin_functions() -> HashMap<String, FunctionInfo> {
    let mut functions = HashMap::new();
    functions.insert(
        "abort".to_string(),
        FunctionInfo {
            type_parameters: Vec::new(),
            parameter_types: vec![Type::String],
            return_type: Type::Never,
            call_target: TypeAnnotatedCallTarget::BuiltinFunction {
                function_name: "abort".to_string(),
            },
        },
    );
    functions.insert(
        "assert".to_string(),
        FunctionInfo {
            type_parameters: Vec::new(),
            parameter_types: vec![Type::Boolean],
            return_type: Type::Nil,
            call_target: TypeAnnotatedCallTarget::BuiltinFunction {
                function_name: "assert".to_string(),
            },
        },
    );
    functions.insert(
        "print".to_string(),
        FunctionInfo {
            type_parameters: Vec::new(),
            parameter_types: vec![Type::String],
            return_type: Type::Nil,
            call_target: TypeAnnotatedCallTarget::BuiltinFunction {
                function_name: "print".to_string(),
            },
        },
    );
    functions
}

impl ExpressionSpan for SemanticExpression {
    fn span(&self) -> Span {
        match self {
            SemanticExpression::IntegerLiteral { span, .. }
            | SemanticExpression::NilLiteral { span, .. }
            | SemanticExpression::BooleanLiteral { span, .. }
            | SemanticExpression::StringLiteral { span, .. }
            | SemanticExpression::ListLiteral { span, .. }
            | SemanticExpression::NameReference { span, .. }
            | SemanticExpression::StructLiteral { span, .. }
            | SemanticExpression::FieldAccess { span, .. }
            | SemanticExpression::IndexAccess { span, .. }
            | SemanticExpression::Call { span, .. }
            | SemanticExpression::Unary { span, .. }
            | SemanticExpression::Binary { span, .. }
            | SemanticExpression::Match { span, .. }
            | SemanticExpression::Matches { span, .. }
            | SemanticExpression::StringInterpolation { span, .. } => span.clone(),
        }
    }
}

impl StatementSpan for SemanticStatement {
    fn span(&self) -> Span {
        match self {
            SemanticStatement::Binding { span, .. }
            | SemanticStatement::Assign { span, .. }
            | SemanticStatement::Return { span, .. }
            | SemanticStatement::If { span, .. }
            | SemanticStatement::For { span, .. }
            | SemanticStatement::Break { span, .. }
            | SemanticStatement::Continue { span, .. }
            | SemanticStatement::Expression { span, .. } => span.clone(),
        }
    }
}
