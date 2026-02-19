use std::collections::{BTreeMap, HashMap};

use compiler__diagnostics::PhaseDiagnostic;
use compiler__packages::PackageId;
use compiler__phase_results::{PhaseOutput, PhaseStatus};
use compiler__semantic_program::{
    SemanticBinaryOperator, SemanticConstantDeclaration, SemanticDeclaration, SemanticExpression,
    SemanticExpressionId, SemanticFile, SemanticFunctionDeclaration, SemanticNameReferenceKind,
    SemanticStatement, SemanticTypeDeclaration, SemanticTypeName, SemanticUnaryOperator,
};
use compiler__semantic_types::{
    FileTypecheckSummary, GenericTypeParameter, ImportedBinding, ImportedSymbol,
    ImportedTypeDeclaration, NominalTypeId, NominalTypeRef, Type, TypedFunctionSignature,
    TypedSymbol, type_from_builtin_name,
};
use compiler__source::Span;
use compiler__type_annotated_program::{
    TypeAnnotatedBinaryOperator, TypeAnnotatedCallTarget, TypeAnnotatedCallableReference,
    TypeAnnotatedExpression, TypeAnnotatedFile, TypeAnnotatedFunctionDeclaration,
    TypeAnnotatedFunctionSignature, TypeAnnotatedMatchArm, TypeAnnotatedMatchPattern,
    TypeAnnotatedMethodDeclaration, TypeAnnotatedNameReferenceKind,
    TypeAnnotatedParameterDeclaration, TypeAnnotatedStatement, TypeAnnotatedStructDeclaration,
    TypeAnnotatedStructFieldDeclaration, TypeAnnotatedStructLiteralField,
    TypeAnnotatedStructReference, TypeAnnotatedTypeName, TypeAnnotatedTypeNameSegment,
    TypeAnnotatedUnaryOperator,
};

mod assignability;
mod declarations;
mod expressions;
mod naming_rules;
mod statements;
mod type_narrowing;
mod unused_bindings;

struct TypeAnalysisSummary {
    file_typecheck_summary: FileTypecheckSummary,
    call_target_by_expression_id: BTreeMap<SemanticExpressionId, TypeAnnotatedCallTarget>,
    struct_reference_by_expression_id: BTreeMap<SemanticExpressionId, TypeAnnotatedStructReference>,
    type_declarations_for_annotations: Vec<SemanticTypeDeclaration>,
    function_declarations_for_annotations: Vec<SemanticFunctionDeclaration>,
}

#[must_use]
pub fn check_package_unit(
    package_id: PackageId,
    package_path: &str,
    package_unit: &SemanticFile,
    imported_bindings: &[ImportedBinding],
) -> PhaseOutput<TypeAnnotatedFile> {
    let mut diagnostics = Vec::new();
    let summary = analyze_package_unit(
        package_id,
        package_path,
        package_unit,
        imported_bindings,
        &mut diagnostics,
    );
    let status = if diagnostics.is_empty() {
        PhaseStatus::Ok
    } else {
        PhaseStatus::PreventsDownstreamExecution
    };

    PhaseOutput {
        value: TypeAnnotatedFile {
            function_signature_by_name: function_signature_by_name_from_summary(
                &summary.file_typecheck_summary,
            ),
            struct_declarations: build_struct_declaration_annotations(
                package_path,
                &summary.type_declarations_for_annotations,
                &summary.call_target_by_expression_id,
                &summary.struct_reference_by_expression_id,
            ),
            function_declarations: build_function_declaration_annotations(
                package_path,
                &summary.function_declarations_for_annotations,
                &summary.call_target_by_expression_id,
                &summary.struct_reference_by_expression_id,
            ),
        },
        diagnostics,
        status,
    }
}

fn function_signature_by_name_from_summary(
    summary: &FileTypecheckSummary,
) -> HashMap<String, TypeAnnotatedFunctionSignature> {
    let mut function_signature_by_name = HashMap::new();
    for (name, typed_symbol) in &summary.typed_symbol_by_name {
        let TypedSymbol::Function(function_signature) = typed_symbol else {
            continue;
        };
        function_signature_by_name.insert(
            name.clone(),
            TypeAnnotatedFunctionSignature {
                type_parameter_count: function_signature.type_parameters.len(),
                parameter_count: function_signature.parameter_types.len(),
                returns_nil: function_signature.return_type == Type::Nil,
            },
        );
    }
    function_signature_by_name
}

fn build_function_declaration_annotations(
    package_path: &str,
    function_declarations: &[SemanticFunctionDeclaration],
    call_target_by_expression_id: &BTreeMap<SemanticExpressionId, TypeAnnotatedCallTarget>,
    struct_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedStructReference,
    >,
) -> Vec<TypeAnnotatedFunctionDeclaration> {
    function_declarations
        .iter()
        .map(|function_declaration| TypeAnnotatedFunctionDeclaration {
            name: function_declaration.name.clone(),
            callable_reference: TypeAnnotatedCallableReference {
                package_path: package_path.to_string(),
                symbol_name: function_declaration.name.clone(),
            },
            parameters: function_declaration
                .parameters
                .iter()
                .map(|parameter| TypeAnnotatedParameterDeclaration {
                    name: parameter.name.clone(),
                    type_name: type_annotated_type_name_from_semantic_type_name(
                        &parameter.type_name,
                    ),
                    span: parameter.span.clone(),
                })
                .collect(),
            return_type: type_annotated_type_name_from_semantic_type_name(
                &function_declaration.return_type,
            ),
            span: function_declaration.span.clone(),
            statements: function_declaration
                .body
                .statements
                .iter()
                .map(|statement| {
                    type_annotated_statement_from_semantic_statement(
                        statement,
                        call_target_by_expression_id,
                        struct_reference_by_expression_id,
                    )
                })
                .collect(),
        })
        .collect()
}

fn build_struct_declaration_annotations(
    package_path: &str,
    type_declarations: &[SemanticTypeDeclaration],
    call_target_by_expression_id: &BTreeMap<SemanticExpressionId, TypeAnnotatedCallTarget>,
    struct_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedStructReference,
    >,
) -> Vec<TypeAnnotatedStructDeclaration> {
    type_declarations
        .iter()
        .filter_map(|type_declaration| match &type_declaration.kind {
            compiler__semantic_program::SemanticTypeDeclarationKind::Struct { fields, methods } => {
                Some(TypeAnnotatedStructDeclaration {
                    name: type_declaration.name.clone(),
                    struct_reference: TypeAnnotatedStructReference {
                        package_path: package_path.to_string(),
                        symbol_name: type_declaration.name.clone(),
                    },
                    fields: fields
                        .iter()
                        .map(|field| TypeAnnotatedStructFieldDeclaration {
                            name: field.name.clone(),
                            type_name: type_annotated_type_name_from_semantic_type_name(
                                &field.type_name,
                            ),
                            span: field.span.clone(),
                        })
                        .collect(),
                    methods: methods
                        .iter()
                        .map(|method| TypeAnnotatedMethodDeclaration {
                            name: method.name.clone(),
                            self_mutable: method.self_mutable,
                            parameters: method
                                .parameters
                                .iter()
                                .map(|parameter| TypeAnnotatedParameterDeclaration {
                                    name: parameter.name.clone(),
                                    type_name: type_annotated_type_name_from_semantic_type_name(
                                        &parameter.type_name,
                                    ),
                                    span: parameter.span.clone(),
                                })
                                .collect(),
                            return_type: type_annotated_type_name_from_semantic_type_name(
                                &method.return_type,
                            ),
                            span: method.span.clone(),
                            statements: method
                                .body
                                .statements
                                .iter()
                                .map(|statement| {
                                    type_annotated_statement_from_semantic_statement(
                                        statement,
                                        call_target_by_expression_id,
                                        struct_reference_by_expression_id,
                                    )
                                })
                                .collect(),
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

fn type_annotated_statement_from_semantic_statement(
    statement: &SemanticStatement,
    call_target_by_expression_id: &BTreeMap<SemanticExpressionId, TypeAnnotatedCallTarget>,
    struct_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedStructReference,
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
                call_target_by_expression_id,
                struct_reference_by_expression_id,
            ),
            span: span.clone(),
        },
        SemanticStatement::Assign {
            name, value, span, ..
        } => TypeAnnotatedStatement::Assign {
            name: name.clone(),
            value: type_annotated_expression_from_semantic_expression(
                value,
                call_target_by_expression_id,
                struct_reference_by_expression_id,
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
                call_target_by_expression_id,
                struct_reference_by_expression_id,
            ),
            then_statements: then_block
                .statements
                .iter()
                .map(|statement| {
                    type_annotated_statement_from_semantic_statement(
                        statement,
                        call_target_by_expression_id,
                        struct_reference_by_expression_id,
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
                            call_target_by_expression_id,
                            struct_reference_by_expression_id,
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
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )
            }),
            body_statements: body
                .statements
                .iter()
                .map(|statement| {
                    type_annotated_statement_from_semantic_statement(
                        statement,
                        call_target_by_expression_id,
                        struct_reference_by_expression_id,
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
                call_target_by_expression_id,
                struct_reference_by_expression_id,
            ),
            span: span.clone(),
        },
        SemanticStatement::Return { value, span } => TypeAnnotatedStatement::Return {
            value: type_annotated_expression_from_semantic_expression(
                value,
                call_target_by_expression_id,
                struct_reference_by_expression_id,
            ),
            span: span.clone(),
        },
    }
}

fn type_annotated_expression_from_semantic_expression(
    expression: &SemanticExpression,
    call_target_by_expression_id: &BTreeMap<SemanticExpressionId, TypeAnnotatedCallTarget>,
    struct_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedStructReference,
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
            span: span.clone(),
        },
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
                        call_target_by_expression_id,
                        struct_reference_by_expression_id,
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
                call_target_by_expression_id,
                struct_reference_by_expression_id,
            )),
            field: field.clone(),
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
                call_target_by_expression_id,
                struct_reference_by_expression_id,
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
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::Subtract => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::Subtract,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::Multiply => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::Multiply,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::Divide => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::Divide,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::EqualEqual => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::EqualEqual,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::NotEqual => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::NotEqual,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::LessThan => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::LessThan,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::LessThanOrEqual => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::LessThanOrEqual,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::GreaterThan => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::GreaterThan,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::GreaterThanOrEqual => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::GreaterThanOrEqual,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::And => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::And,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                span: span.clone(),
            },
            SemanticBinaryOperator::Or => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::Or,
                left: Box::new(type_annotated_expression_from_semantic_expression(
                    left,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
                )),
                right: Box::new(type_annotated_expression_from_semantic_expression(
                    right,
                    call_target_by_expression_id,
                    struct_reference_by_expression_id,
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
                call_target_by_expression_id,
                struct_reference_by_expression_id,
            )),
            call_target: call_target_by_expression_id
                .get(&semantic_expression_id(expression))
                .cloned(),
            arguments: arguments
                .iter()
                .map(|argument| {
                    type_annotated_expression_from_semantic_expression(
                        argument,
                        call_target_by_expression_id,
                        struct_reference_by_expression_id,
                    )
                })
                .collect(),
            has_type_arguments: !type_arguments.is_empty(),
            span: span.clone(),
        },
        SemanticExpression::Match {
            target, arms, span, ..
        } => TypeAnnotatedExpression::Match {
            target: Box::new(type_annotated_expression_from_semantic_expression(
                target,
                call_target_by_expression_id,
                struct_reference_by_expression_id,
            )),
            arms: arms
                .iter()
                .map(|arm| {
                    type_annotated_match_arm_from_semantic_match_arm(
                        arm,
                        call_target_by_expression_id,
                        struct_reference_by_expression_id,
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
                call_target_by_expression_id,
                struct_reference_by_expression_id,
            )),
            type_name: type_annotated_type_name_from_semantic_type_name(type_name),
            span: span.clone(),
        },
    }
}

fn type_annotated_match_arm_from_semantic_match_arm(
    arm: &compiler__semantic_program::SemanticMatchArm,
    call_target_by_expression_id: &BTreeMap<SemanticExpressionId, TypeAnnotatedCallTarget>,
    struct_reference_by_expression_id: &BTreeMap<
        SemanticExpressionId,
        TypeAnnotatedStructReference,
    >,
) -> TypeAnnotatedMatchArm {
    TypeAnnotatedMatchArm {
        pattern: type_annotated_match_pattern_from_semantic_match_pattern(&arm.pattern),
        value: type_annotated_expression_from_semantic_expression(
            &arm.value,
            call_target_by_expression_id,
            struct_reference_by_expression_id,
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
        | SemanticExpression::NameReference { id, .. }
        | SemanticExpression::StructLiteral { id, .. }
        | SemanticExpression::FieldAccess { id, .. }
        | SemanticExpression::Call { id, .. }
        | SemanticExpression::Unary { id, .. }
        | SemanticExpression::Binary { id, .. }
        | SemanticExpression::Match { id, .. }
        | SemanticExpression::Matches { id, .. } => *id,
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
                has_type_arguments: !name_segment.type_arguments.is_empty(),
                span: name_segment.span.clone(),
            })
            .collect(),
        span: type_name.span.clone(),
    }
}

fn analyze_package_unit(
    package_id: PackageId,
    package_path: &str,
    package_unit: &SemanticFile,
    imported_bindings: &[ImportedBinding],
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> TypeAnalysisSummary {
    check_package_unit_declarations(
        package_id,
        package_path,
        package_unit,
        imported_bindings,
        diagnostics,
    )
}

fn check_package_unit_declarations(
    package_id: PackageId,
    package_path: &str,
    package_unit: &SemanticFile,
    imported_bindings: &[ImportedBinding],
    diagnostics: &mut Vec<PhaseDiagnostic>,
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
        diagnostics,
        &type_declarations,
        &constant_declarations,
        &function_declarations,
        imported_bindings,
    );
    summary.type_declarations_for_annotations = type_declarations;
    summary.function_declarations_for_annotations = function_declarations;
    summary
}

fn check_declarations(
    package_id: PackageId,
    package_path: &str,
    diagnostics: &mut Vec<PhaseDiagnostic>,
    type_declarations: &[SemanticTypeDeclaration],
    constant_declarations: &[SemanticConstantDeclaration],
    function_declarations: &[SemanticFunctionDeclaration],
    imported_bindings: &[ImportedBinding],
) -> TypeAnalysisSummary {
    let mut type_checker =
        TypeChecker::new(package_id, package_path, imported_bindings, diagnostics);
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
    span: Span,
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
    constants: HashMap<String, ConstantInfo>,
    types: HashMap<String, TypeInfo>,
    functions: HashMap<String, FunctionInfo>,
    imported_functions: HashMap<String, FunctionInfo>,
    imported_bindings: HashMap<String, ImportedBindingInfo>,
    methods: HashMap<MethodKey, MethodInfo>,
    scopes: Vec<HashMap<String, VariableInfo>>,
    type_parameter_scopes: Vec<HashMap<String, Span>>,
    diagnostics: &'a mut Vec<PhaseDiagnostic>,
    current_return_type: Type,
    loop_depth: usize,
    call_target_by_expression_id: BTreeMap<SemanticExpressionId, TypeAnnotatedCallTarget>,
    struct_reference_by_expression_id: BTreeMap<SemanticExpressionId, TypeAnnotatedStructReference>,
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
        imported_bindings: &[ImportedBinding],
        diagnostics: &'a mut Vec<PhaseDiagnostic>,
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
            constants: HashMap::new(),
            types: HashMap::new(),
            functions: builtin_functions(),
            imported_functions: HashMap::new(),
            imported_bindings: imported_binding_map,
            methods: HashMap::new(),
            scopes: Vec::new(),
            type_parameter_scopes: Vec::new(),
            diagnostics,
            current_return_type: Type::Unknown,
            loop_depth: 0,
            call_target_by_expression_id: BTreeMap::new(),
            struct_reference_by_expression_id: BTreeMap::new(),
        }
    }

    fn build_summary(
        &self,
        type_declarations: &[SemanticTypeDeclaration],
        function_declarations: &[SemanticFunctionDeclaration],
        constant_declarations: &[SemanticConstantDeclaration],
    ) -> TypeAnalysisSummary {
        let mut typed_symbol_by_name = HashMap::new();

        for type_declaration in type_declarations {
            typed_symbol_by_name.insert(type_declaration.name.clone(), TypedSymbol::Type);
        }
        for function_declaration in function_declarations {
            if let Some(info) = self.functions.get(&function_declaration.name) {
                typed_symbol_by_name.insert(
                    function_declaration.name.clone(),
                    TypedSymbol::Function(TypedFunctionSignature {
                        type_parameters: info.type_parameters.clone(),
                        parameter_types: info.parameter_types.clone(),
                        return_type: info.return_type.clone(),
                    }),
                );
            }
        }
        for constant_declaration in constant_declarations {
            if let Some(info) = self.constants.get(&constant_declaration.name) {
                typed_symbol_by_name.insert(
                    constant_declaration.name.clone(),
                    TypedSymbol::Constant(info.value_type.clone()),
                );
            }
        }

        TypeAnalysisSummary {
            file_typecheck_summary: FileTypecheckSummary {
                typed_symbol_by_name,
            },
            call_target_by_expression_id: self.call_target_by_expression_id.clone(),
            struct_reference_by_expression_id: self.struct_reference_by_expression_id.clone(),
            type_declarations_for_annotations: Vec::new(),
            function_declarations_for_annotations: Vec::new(),
        }
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

    fn define_variable(&mut self, name: String, value_type: Type, mutable: bool, span: Span) {
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
                    span,
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
                return Type::Function {
                    parameter_types: function_info.parameter_types,
                    return_type: Box::new(function_info.return_type),
                };
            }
        }
        self.resolve_variable(name, span)
    }

    fn resolve_variable(&mut self, name: &str, span: &Span) -> Type {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(info) = scope.get_mut(name) {
                info.used = true;
                return info.value_type.clone();
            }
        }
        if let Some(info) = self.constants.get(name) {
            return info.value_type.clone();
        }
        if let Some(value_type) = self.imported_constant_type(name) {
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
                let kind = info.kind.clone();
                let nominal_type_id = info.nominal_type_id.clone();
                let declared_type_parameters = info.type_parameters.clone();
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
                match kind {
                    TypeKind::Struct { .. } | TypeKind::Interface { .. } => {
                        if type_parameter_count == 0 {
                            resolved.push(Type::Named(nominal));
                        } else {
                            resolved.push(Type::Applied {
                                base: nominal,
                                arguments: resolved_type_arguments,
                            });
                        }
                    }
                    TypeKind::Union { variants } => {
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
                    }
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
        let variants = variants.clone();
        let variant_display = format!("{enum_name}.{variant_name}");
        if matches!(
            self.imported_bindings.get(enum_name),
            Some(ImportedBindingInfo {
                symbol: ImportedSymbol::Type(_),
                ..
            })
        ) {
            self.mark_import_used(enum_name);
        }
        variants
            .into_iter()
            .find(|variant| variant.display() == variant_display)
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
            | SemanticExpression::NameReference { span, .. }
            | SemanticExpression::StructLiteral { span, .. }
            | SemanticExpression::FieldAccess { span, .. }
            | SemanticExpression::Call { span, .. }
            | SemanticExpression::Unary { span, .. }
            | SemanticExpression::Binary { span, .. }
            | SemanticExpression::Match { span, .. }
            | SemanticExpression::Matches { span, .. } => span.clone(),
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
