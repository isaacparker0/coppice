use compiler__phase_results::{PhaseOutput, PhaseStatus};
use compiler__semantic_program as semantic;
use compiler__syntax as syntax;

#[must_use]
pub fn lower_parsed_file(
    parsed_file: &syntax::SyntaxParsedFile,
) -> PhaseOutput<semantic::SemanticFile> {
    let mut declarations = Vec::new();
    let mut pending_doc_comment: Option<semantic::SemanticDocComment> = None;

    for item in &parsed_file.items {
        match item {
            syntax::SyntaxFileItem::DocComment(doc_comment) => {
                pending_doc_comment = Some(lower_doc_comment(doc_comment));
            }
            syntax::SyntaxFileItem::Declaration(declaration) => match declaration.as_ref() {
                syntax::SyntaxDeclaration::Type(type_declaration) => {
                    let lowered =
                        lower_type_declaration(type_declaration, pending_doc_comment.take());
                    declarations.push(semantic::SemanticDeclaration::Type(lowered.clone()));
                }
                syntax::SyntaxDeclaration::Constant(constant_declaration) => {
                    let lowered = lower_constant_declaration(
                        constant_declaration,
                        pending_doc_comment.take(),
                    );
                    declarations.push(semantic::SemanticDeclaration::Constant(lowered.clone()));
                }
                syntax::SyntaxDeclaration::Function(function_declaration) => {
                    let lowered = lower_function_declaration(
                        function_declaration,
                        pending_doc_comment.take(),
                    );
                    declarations.push(semantic::SemanticDeclaration::Function(lowered.clone()));
                }
                syntax::SyntaxDeclaration::Import(_) | syntax::SyntaxDeclaration::Exports(_) => {}
            },
        }
    }

    PhaseOutput {
        value: semantic::SemanticFile {
            role: parsed_file.role,
            declarations,
        },
        diagnostics: Vec::new(),
        status: PhaseStatus::Ok,
    }
}

fn lower_doc_comment(doc_comment: &syntax::SyntaxDocComment) -> semantic::SemanticDocComment {
    semantic::SemanticDocComment {
        lines: doc_comment.lines.clone(),
        span: doc_comment.span.clone(),
        end_line: doc_comment.end_line,
    }
}

fn lower_top_level_visibility(
    visibility: syntax::SyntaxTopLevelVisibility,
) -> semantic::SemanticTopLevelVisibility {
    match visibility {
        syntax::SyntaxTopLevelVisibility::Private => semantic::SemanticTopLevelVisibility::Private,
        syntax::SyntaxTopLevelVisibility::Visible => semantic::SemanticTopLevelVisibility::Visible,
    }
}

fn lower_member_visibility(
    visibility: syntax::SyntaxMemberVisibility,
) -> semantic::SemanticMemberVisibility {
    match visibility {
        syntax::SyntaxMemberVisibility::Private => semantic::SemanticMemberVisibility::Private,
        syntax::SyntaxMemberVisibility::Public => semantic::SemanticMemberVisibility::Public,
    }
}

fn lower_type_declaration(
    type_declaration: &syntax::SyntaxTypeDeclaration,
    doc: Option<semantic::SemanticDocComment>,
) -> semantic::SemanticTypeDeclaration {
    semantic::SemanticTypeDeclaration {
        name: type_declaration.name.clone(),
        type_parameters: type_declaration
            .type_parameters
            .iter()
            .map(lower_type_parameter)
            .collect(),
        kind: lower_type_declaration_kind(&type_declaration.kind),
        doc,
        visibility: lower_top_level_visibility(type_declaration.visibility),
        span: type_declaration.span.clone(),
    }
}

fn lower_type_declaration_kind(
    kind: &syntax::SyntaxTypeDeclarationKind,
) -> semantic::SemanticTypeDeclarationKind {
    match kind {
        syntax::SyntaxTypeDeclarationKind::Struct { items } => {
            let mut fields = Vec::new();
            let mut methods = Vec::new();
            let mut pending_doc_comment: Option<semantic::SemanticDocComment> = None;
            for item in items {
                match item {
                    syntax::SyntaxStructMemberItem::DocComment(doc_comment) => {
                        pending_doc_comment = Some(lower_doc_comment(doc_comment));
                    }
                    syntax::SyntaxStructMemberItem::Field(field) => {
                        fields.push(lower_field_declaration(field, pending_doc_comment.take()));
                    }
                    syntax::SyntaxStructMemberItem::Method(method) => {
                        methods.push(lower_method_declaration(method, pending_doc_comment.take()));
                    }
                }
            }
            semantic::SemanticTypeDeclarationKind::Struct { fields, methods }
        }
        syntax::SyntaxTypeDeclarationKind::Enum { variants } => {
            semantic::SemanticTypeDeclarationKind::Enum {
                variants: variants.iter().map(lower_enum_variant).collect(),
            }
        }
        syntax::SyntaxTypeDeclarationKind::Union { variants } => {
            semantic::SemanticTypeDeclarationKind::Union {
                variants: variants.iter().map(lower_type_name).collect(),
            }
        }
    }
}

fn lower_enum_variant(variant: &syntax::SyntaxEnumVariant) -> semantic::SemanticEnumVariant {
    semantic::SemanticEnumVariant {
        name: variant.name.clone(),
        span: variant.span.clone(),
    }
}

fn lower_field_declaration(
    field: &syntax::SyntaxFieldDeclaration,
    doc: Option<semantic::SemanticDocComment>,
) -> semantic::SemanticFieldDeclaration {
    semantic::SemanticFieldDeclaration {
        name: field.name.clone(),
        type_name: lower_type_name(&field.type_name),
        doc,
        visibility: lower_member_visibility(field.visibility),
        span: field.span.clone(),
    }
}

fn lower_method_declaration(
    method: &syntax::SyntaxMethodDeclaration,
    doc: Option<semantic::SemanticDocComment>,
) -> semantic::SemanticMethodDeclaration {
    semantic::SemanticMethodDeclaration {
        name: method.name.clone(),
        name_span: method.name_span.clone(),
        self_span: method.self_span.clone(),
        self_mutable: method.self_mutable,
        parameters: method
            .parameters
            .iter()
            .map(lower_parameter_declaration)
            .collect(),
        return_type: lower_type_name(&method.return_type),
        body: lower_block(&method.body),
        doc,
        visibility: lower_member_visibility(method.visibility),
        span: method.span.clone(),
    }
}

fn lower_constant_declaration(
    constant: &syntax::SyntaxConstantDeclaration,
    doc: Option<semantic::SemanticDocComment>,
) -> semantic::SemanticConstantDeclaration {
    semantic::SemanticConstantDeclaration {
        name: constant.name.clone(),
        type_name: lower_type_name(&constant.type_name),
        expression: lower_expression(&constant.expression),
        doc,
        visibility: lower_top_level_visibility(constant.visibility),
        span: constant.span.clone(),
    }
}

fn lower_function_declaration(
    function: &syntax::SyntaxFunctionDeclaration,
    doc: Option<semantic::SemanticDocComment>,
) -> semantic::SemanticFunctionDeclaration {
    semantic::SemanticFunctionDeclaration {
        name: function.name.clone(),
        name_span: function.name_span.clone(),
        type_parameters: function
            .type_parameters
            .iter()
            .map(lower_type_parameter)
            .collect(),
        parameters: function
            .parameters
            .iter()
            .map(lower_parameter_declaration)
            .collect(),
        return_type: lower_type_name(&function.return_type),
        body: lower_block(&function.body),
        doc,
        visibility: lower_top_level_visibility(function.visibility),
        span: function.span.clone(),
    }
}

fn lower_parameter_declaration(
    parameter: &syntax::SyntaxParameterDeclaration,
) -> semantic::SemanticParameterDeclaration {
    semantic::SemanticParameterDeclaration {
        name: parameter.name.clone(),
        type_name: lower_type_name(&parameter.type_name),
        span: parameter.span.clone(),
    }
}

fn lower_block(block: &syntax::SyntaxBlock) -> semantic::SemanticBlock {
    semantic::SemanticBlock {
        statements: block
            .items
            .iter()
            .filter_map(|item| match item {
                syntax::SyntaxBlockItem::DocComment(_) => None,
                syntax::SyntaxBlockItem::Statement(statement) => Some(lower_statement(statement)),
            })
            .collect(),
        span: block.span.clone(),
    }
}

fn lower_statement(statement: &syntax::SyntaxStatement) -> semantic::SemanticStatement {
    match statement {
        syntax::SyntaxStatement::Binding {
            name,
            mutable,
            type_name,
            initializer,
            span,
        } => semantic::SemanticStatement::Binding {
            name: name.clone(),
            mutable: *mutable,
            type_name: type_name.as_ref().map(lower_type_name),
            initializer: lower_expression(initializer),
            span: span.clone(),
        },
        syntax::SyntaxStatement::Assign {
            name,
            name_span,
            value,
            span,
        } => semantic::SemanticStatement::Assign {
            name: name.clone(),
            name_span: name_span.clone(),
            value: lower_expression(value),
            span: span.clone(),
        },
        syntax::SyntaxStatement::Return { value, span } => semantic::SemanticStatement::Return {
            value: lower_expression(value),
            span: span.clone(),
        },
        syntax::SyntaxStatement::Break { span } => {
            semantic::SemanticStatement::Break { span: span.clone() }
        }
        syntax::SyntaxStatement::Continue { span } => {
            semantic::SemanticStatement::Continue { span: span.clone() }
        }
        syntax::SyntaxStatement::If {
            condition,
            then_block,
            else_block,
            span,
        } => semantic::SemanticStatement::If {
            condition: lower_expression(condition),
            then_block: lower_block(then_block),
            else_block: else_block.as_ref().map(lower_block),
            span: span.clone(),
        },
        syntax::SyntaxStatement::For {
            condition,
            body,
            span,
        } => semantic::SemanticStatement::For {
            condition: condition.as_ref().map(lower_expression),
            body: lower_block(body),
            span: span.clone(),
        },
        syntax::SyntaxStatement::Expression { value, span } => {
            semantic::SemanticStatement::Expression {
                value: lower_expression(value),
                span: span.clone(),
            }
        }
    }
}

fn lower_expression(expression: &syntax::SyntaxExpression) -> semantic::SemanticExpression {
    match expression {
        syntax::SyntaxExpression::IntegerLiteral { value, span } => {
            semantic::SemanticExpression::IntegerLiteral {
                value: *value,
                span: span.clone(),
            }
        }
        syntax::SyntaxExpression::NilLiteral { span } => {
            semantic::SemanticExpression::NilLiteral { span: span.clone() }
        }
        syntax::SyntaxExpression::BooleanLiteral { value, span } => {
            semantic::SemanticExpression::BooleanLiteral {
                value: *value,
                span: span.clone(),
            }
        }
        syntax::SyntaxExpression::StringLiteral { value, span } => {
            semantic::SemanticExpression::StringLiteral {
                value: value.clone(),
                span: span.clone(),
            }
        }
        syntax::SyntaxExpression::Symbol { name, kind, span } => {
            semantic::SemanticExpression::Symbol {
                name: name.clone(),
                kind: match kind {
                    syntax::SyntaxSymbolKind::UserDefined => {
                        semantic::SemanticSymbolKind::UserDefined
                    }
                    syntax::SyntaxSymbolKind::Builtin => semantic::SemanticSymbolKind::Builtin,
                },
                span: span.clone(),
            }
        }
        syntax::SyntaxExpression::StructLiteral {
            type_name,
            fields,
            span,
        } => semantic::SemanticExpression::StructLiteral {
            type_name: lower_type_name(type_name),
            fields: fields.iter().map(lower_struct_literal_field).collect(),
            span: span.clone(),
        },
        syntax::SyntaxExpression::FieldAccess {
            target,
            field,
            field_span,
            span,
        } => semantic::SemanticExpression::FieldAccess {
            target: Box::new(lower_expression(target)),
            field: field.clone(),
            field_span: field_span.clone(),
            span: span.clone(),
        },
        syntax::SyntaxExpression::Call {
            callee,
            type_arguments,
            arguments,
            span,
        } => semantic::SemanticExpression::Call {
            callee: Box::new(lower_expression(callee)),
            type_arguments: type_arguments.iter().map(lower_type_name).collect(),
            arguments: arguments.iter().map(lower_expression).collect(),
            span: span.clone(),
        },
        syntax::SyntaxExpression::Unary {
            operator,
            expression,
            span,
        } => semantic::SemanticExpression::Unary {
            operator: lower_unary_operator(*operator),
            expression: Box::new(lower_expression(expression)),
            span: span.clone(),
        },
        syntax::SyntaxExpression::Binary {
            operator,
            left,
            right,
            span,
        } => semantic::SemanticExpression::Binary {
            operator: lower_binary_operator(*operator),
            left: Box::new(lower_expression(left)),
            right: Box::new(lower_expression(right)),
            span: span.clone(),
        },
        syntax::SyntaxExpression::Match { target, arms, span } => {
            semantic::SemanticExpression::Match {
                target: Box::new(lower_expression(target)),
                arms: arms.iter().map(lower_match_arm).collect(),
                span: span.clone(),
            }
        }
        syntax::SyntaxExpression::Matches {
            value,
            type_name,
            span,
        } => semantic::SemanticExpression::Matches {
            value: Box::new(lower_expression(value)),
            type_name: lower_type_name(type_name),
            span: span.clone(),
        },
    }
}

fn lower_binary_operator(
    operator: syntax::SyntaxBinaryOperator,
) -> semantic::SemanticBinaryOperator {
    match operator {
        syntax::SyntaxBinaryOperator::Add => semantic::SemanticBinaryOperator::Add,
        syntax::SyntaxBinaryOperator::Subtract => semantic::SemanticBinaryOperator::Subtract,
        syntax::SyntaxBinaryOperator::Multiply => semantic::SemanticBinaryOperator::Multiply,
        syntax::SyntaxBinaryOperator::Divide => semantic::SemanticBinaryOperator::Divide,
        syntax::SyntaxBinaryOperator::EqualEqual => semantic::SemanticBinaryOperator::EqualEqual,
        syntax::SyntaxBinaryOperator::NotEqual => semantic::SemanticBinaryOperator::NotEqual,
        syntax::SyntaxBinaryOperator::LessThan => semantic::SemanticBinaryOperator::LessThan,
        syntax::SyntaxBinaryOperator::LessThanOrEqual => {
            semantic::SemanticBinaryOperator::LessThanOrEqual
        }
        syntax::SyntaxBinaryOperator::GreaterThan => semantic::SemanticBinaryOperator::GreaterThan,
        syntax::SyntaxBinaryOperator::GreaterThanOrEqual => {
            semantic::SemanticBinaryOperator::GreaterThanOrEqual
        }
        syntax::SyntaxBinaryOperator::And => semantic::SemanticBinaryOperator::And,
        syntax::SyntaxBinaryOperator::Or => semantic::SemanticBinaryOperator::Or,
    }
}

fn lower_unary_operator(operator: syntax::SyntaxUnaryOperator) -> semantic::SemanticUnaryOperator {
    match operator {
        syntax::SyntaxUnaryOperator::Not => semantic::SemanticUnaryOperator::Not,
        syntax::SyntaxUnaryOperator::Negate => semantic::SemanticUnaryOperator::Negate,
    }
}

fn lower_struct_literal_field(
    field: &syntax::SyntaxStructLiteralField,
) -> semantic::SemanticStructLiteralField {
    semantic::SemanticStructLiteralField {
        name: field.name.clone(),
        name_span: field.name_span.clone(),
        value: lower_expression(&field.value),
        span: field.span.clone(),
    }
}

fn lower_match_arm(arm: &syntax::SyntaxMatchArm) -> semantic::SemanticMatchArm {
    semantic::SemanticMatchArm {
        pattern: lower_match_pattern(&arm.pattern),
        value: lower_expression(&arm.value),
        span: arm.span.clone(),
    }
}

fn lower_match_pattern(pattern: &syntax::SyntaxMatchPattern) -> semantic::SemanticMatchPattern {
    match pattern {
        syntax::SyntaxMatchPattern::Type { type_name, span } => {
            semantic::SemanticMatchPattern::Type {
                type_name: lower_type_name(type_name),
                span: span.clone(),
            }
        }
        syntax::SyntaxMatchPattern::Binding {
            name,
            name_span,
            type_name,
            span,
        } => semantic::SemanticMatchPattern::Binding {
            name: name.clone(),
            name_span: name_span.clone(),
            type_name: lower_type_name(type_name),
            span: span.clone(),
        },
    }
}

fn lower_type_name(type_name: &syntax::SyntaxTypeName) -> semantic::SemanticTypeName {
    semantic::SemanticTypeName {
        names: type_name
            .names
            .iter()
            .map(|segment| semantic::SemanticTypeNameSegment {
                name: segment.name.clone(),
                type_arguments: segment.type_arguments.iter().map(lower_type_name).collect(),
                span: segment.span.clone(),
            })
            .collect(),
        span: type_name.span.clone(),
    }
}

fn lower_type_parameter(
    type_parameter: &syntax::SyntaxTypeParameter,
) -> semantic::SemanticTypeParameter {
    semantic::SemanticTypeParameter {
        name: type_parameter.name.clone(),
        constraint: type_parameter.constraint.as_ref().map(lower_type_name),
        span: type_parameter.span.clone(),
    }
}
