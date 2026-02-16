use compiler__semantic_program as semantic;
use compiler__syntax as syntax;

#[must_use]
pub fn lower_parsed_file(parsed_file: &syntax::ParsedFile) -> semantic::PackageUnit {
    let mut type_declarations = Vec::new();
    let mut constant_declarations = Vec::new();
    let mut function_declarations = Vec::new();

    for declaration in &parsed_file.declarations {
        match declaration {
            syntax::Declaration::Type(type_declaration) => {
                type_declarations.push(lower_type_declaration(type_declaration));
            }
            syntax::Declaration::Constant(constant_declaration) => {
                constant_declarations.push(lower_constant_declaration(constant_declaration));
            }
            syntax::Declaration::Function(function_declaration) => {
                function_declarations.push(lower_function_declaration(function_declaration));
            }
            syntax::Declaration::Import(_) | syntax::Declaration::Exports(_) => {}
        }
    }

    semantic::PackageUnit {
        role: parsed_file.role,
        type_declarations,
        constant_declarations,
        function_declarations,
    }
}

fn lower_doc_comment(doc: Option<&syntax::DocComment>) -> Option<semantic::DocComment> {
    doc.map(|doc| semantic::DocComment {
        lines: doc.lines.clone(),
        span: doc.span.clone(),
        end_line: doc.end_line,
    })
}

fn lower_visibility(visibility: syntax::Visibility) -> semantic::Visibility {
    match visibility {
        syntax::Visibility::Private => semantic::Visibility::Private,
        syntax::Visibility::Public => semantic::Visibility::Public,
    }
}

fn lower_type_declaration(type_declaration: &syntax::TypeDeclaration) -> semantic::TypeDeclaration {
    semantic::TypeDeclaration {
        name: type_declaration.name.clone(),
        kind: lower_type_declaration_kind(&type_declaration.kind),
        doc: lower_doc_comment(type_declaration.doc.as_ref()),
        visibility: lower_visibility(type_declaration.visibility),
        span: type_declaration.span.clone(),
    }
}

fn lower_type_declaration_kind(
    kind: &syntax::TypeDeclarationKind,
) -> semantic::TypeDeclarationKind {
    match kind {
        syntax::TypeDeclarationKind::Struct { fields, methods } => {
            semantic::TypeDeclarationKind::Struct {
                fields: fields.iter().map(lower_field_declaration).collect(),
                methods: methods.iter().map(lower_method_declaration).collect(),
            }
        }
        syntax::TypeDeclarationKind::Enum { variants } => semantic::TypeDeclarationKind::Enum {
            variants: variants.iter().map(lower_enum_variant).collect(),
        },
        syntax::TypeDeclarationKind::Union { variants } => semantic::TypeDeclarationKind::Union {
            variants: variants.iter().map(lower_type_name).collect(),
        },
    }
}

fn lower_enum_variant(variant: &syntax::EnumVariant) -> semantic::EnumVariant {
    semantic::EnumVariant {
        name: variant.name.clone(),
        span: variant.span.clone(),
    }
}

fn lower_field_declaration(field: &syntax::FieldDeclaration) -> semantic::FieldDeclaration {
    semantic::FieldDeclaration {
        name: field.name.clone(),
        type_name: lower_type_name(&field.type_name),
        doc: lower_doc_comment(field.doc.as_ref()),
        visibility: lower_visibility(field.visibility),
        span: field.span.clone(),
    }
}

fn lower_method_declaration(method: &syntax::MethodDeclaration) -> semantic::MethodDeclaration {
    semantic::MethodDeclaration {
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
        doc: lower_doc_comment(method.doc.as_ref()),
        visibility: lower_visibility(method.visibility),
        span: method.span.clone(),
    }
}

fn lower_constant_declaration(
    constant: &syntax::ConstantDeclaration,
) -> semantic::ConstantDeclaration {
    semantic::ConstantDeclaration {
        name: constant.name.clone(),
        type_name: lower_type_name(&constant.type_name),
        expression: lower_expression(&constant.expression),
        doc: lower_doc_comment(constant.doc.as_ref()),
        visibility: lower_visibility(constant.visibility),
        span: constant.span.clone(),
    }
}

fn lower_function_declaration(
    function: &syntax::FunctionDeclaration,
) -> semantic::FunctionDeclaration {
    semantic::FunctionDeclaration {
        name: function.name.clone(),
        name_span: function.name_span.clone(),
        parameters: function
            .parameters
            .iter()
            .map(lower_parameter_declaration)
            .collect(),
        return_type: lower_type_name(&function.return_type),
        body: lower_block(&function.body),
        doc: lower_doc_comment(function.doc.as_ref()),
        visibility: lower_visibility(function.visibility),
        span: function.span.clone(),
    }
}

fn lower_parameter_declaration(
    parameter: &syntax::ParameterDeclaration,
) -> semantic::ParameterDeclaration {
    semantic::ParameterDeclaration {
        name: parameter.name.clone(),
        type_name: lower_type_name(&parameter.type_name),
        span: parameter.span.clone(),
    }
}

fn lower_block(block: &syntax::Block) -> semantic::Block {
    semantic::Block {
        statements: block.statements.iter().map(lower_statement).collect(),
        span: block.span.clone(),
    }
}

fn lower_statement(statement: &syntax::Statement) -> semantic::Statement {
    match statement {
        syntax::Statement::Let {
            name,
            mutable,
            type_name,
            initializer,
            span,
        } => semantic::Statement::Let {
            name: name.clone(),
            mutable: *mutable,
            type_name: type_name.as_ref().map(lower_type_name),
            initializer: lower_expression(initializer),
            span: span.clone(),
        },
        syntax::Statement::Assign {
            name,
            name_span,
            value,
            span,
        } => semantic::Statement::Assign {
            name: name.clone(),
            name_span: name_span.clone(),
            value: lower_expression(value),
            span: span.clone(),
        },
        syntax::Statement::Return { value, span } => semantic::Statement::Return {
            value: lower_expression(value),
            span: span.clone(),
        },
        syntax::Statement::Abort { message, span } => semantic::Statement::Abort {
            message: lower_expression(message),
            span: span.clone(),
        },
        syntax::Statement::Break { span } => semantic::Statement::Break { span: span.clone() },
        syntax::Statement::Continue { span } => {
            semantic::Statement::Continue { span: span.clone() }
        }
        syntax::Statement::If {
            condition,
            then_block,
            else_block,
            span,
        } => semantic::Statement::If {
            condition: lower_expression(condition),
            then_block: lower_block(then_block),
            else_block: else_block.as_ref().map(lower_block),
            span: span.clone(),
        },
        syntax::Statement::For {
            condition,
            body,
            span,
        } => semantic::Statement::For {
            condition: condition.as_ref().map(lower_expression),
            body: lower_block(body),
            span: span.clone(),
        },
        syntax::Statement::Expression { value, span } => semantic::Statement::Expression {
            value: lower_expression(value),
            span: span.clone(),
        },
    }
}

fn lower_expression(expression: &syntax::Expression) -> semantic::Expression {
    match expression {
        syntax::Expression::IntegerLiteral { value, span } => {
            semantic::Expression::IntegerLiteral {
                value: *value,
                span: span.clone(),
            }
        }
        syntax::Expression::NilLiteral { span } => {
            semantic::Expression::NilLiteral { span: span.clone() }
        }
        syntax::Expression::BooleanLiteral { value, span } => {
            semantic::Expression::BooleanLiteral {
                value: *value,
                span: span.clone(),
            }
        }
        syntax::Expression::StringLiteral { value, span } => semantic::Expression::StringLiteral {
            value: value.clone(),
            span: span.clone(),
        },
        syntax::Expression::Identifier { name, span } => semantic::Expression::Identifier {
            name: name.clone(),
            span: span.clone(),
        },
        syntax::Expression::StructLiteral {
            type_name,
            fields,
            span,
        } => semantic::Expression::StructLiteral {
            type_name: lower_type_name(type_name),
            fields: fields.iter().map(lower_struct_literal_field).collect(),
            span: span.clone(),
        },
        syntax::Expression::FieldAccess {
            target,
            field,
            field_span,
            span,
        } => semantic::Expression::FieldAccess {
            target: Box::new(lower_expression(target)),
            field: field.clone(),
            field_span: field_span.clone(),
            span: span.clone(),
        },
        syntax::Expression::Call {
            callee,
            arguments,
            span,
        } => semantic::Expression::Call {
            callee: Box::new(lower_expression(callee)),
            arguments: arguments.iter().map(lower_expression).collect(),
            span: span.clone(),
        },
        syntax::Expression::Unary {
            operator,
            expression,
            span,
        } => semantic::Expression::Unary {
            operator: lower_unary_operator(*operator),
            expression: Box::new(lower_expression(expression)),
            span: span.clone(),
        },
        syntax::Expression::Binary {
            operator,
            left,
            right,
            span,
        } => semantic::Expression::Binary {
            operator: lower_binary_operator(*operator),
            left: Box::new(lower_expression(left)),
            right: Box::new(lower_expression(right)),
            span: span.clone(),
        },
        syntax::Expression::Match { target, arms, span } => semantic::Expression::Match {
            target: Box::new(lower_expression(target)),
            arms: arms.iter().map(lower_match_arm).collect(),
            span: span.clone(),
        },
        syntax::Expression::Matches {
            value,
            type_name,
            span,
        } => semantic::Expression::Matches {
            value: Box::new(lower_expression(value)),
            type_name: lower_type_name(type_name),
            span: span.clone(),
        },
    }
}

fn lower_binary_operator(operator: syntax::BinaryOperator) -> semantic::BinaryOperator {
    match operator {
        syntax::BinaryOperator::Add => semantic::BinaryOperator::Add,
        syntax::BinaryOperator::Subtract => semantic::BinaryOperator::Subtract,
        syntax::BinaryOperator::Multiply => semantic::BinaryOperator::Multiply,
        syntax::BinaryOperator::Divide => semantic::BinaryOperator::Divide,
        syntax::BinaryOperator::EqualEqual => semantic::BinaryOperator::EqualEqual,
        syntax::BinaryOperator::NotEqual => semantic::BinaryOperator::NotEqual,
        syntax::BinaryOperator::LessThan => semantic::BinaryOperator::LessThan,
        syntax::BinaryOperator::LessThanOrEqual => semantic::BinaryOperator::LessThanOrEqual,
        syntax::BinaryOperator::GreaterThan => semantic::BinaryOperator::GreaterThan,
        syntax::BinaryOperator::GreaterThanOrEqual => semantic::BinaryOperator::GreaterThanOrEqual,
        syntax::BinaryOperator::And => semantic::BinaryOperator::And,
        syntax::BinaryOperator::Or => semantic::BinaryOperator::Or,
    }
}

fn lower_unary_operator(operator: syntax::UnaryOperator) -> semantic::UnaryOperator {
    match operator {
        syntax::UnaryOperator::Not => semantic::UnaryOperator::Not,
        syntax::UnaryOperator::Negate => semantic::UnaryOperator::Negate,
    }
}

fn lower_struct_literal_field(field: &syntax::StructLiteralField) -> semantic::StructLiteralField {
    semantic::StructLiteralField {
        name: field.name.clone(),
        name_span: field.name_span.clone(),
        value: lower_expression(&field.value),
        span: field.span.clone(),
    }
}

fn lower_match_arm(arm: &syntax::MatchArm) -> semantic::MatchArm {
    semantic::MatchArm {
        pattern: lower_match_pattern(&arm.pattern),
        value: lower_expression(&arm.value),
        span: arm.span.clone(),
    }
}

fn lower_match_pattern(pattern: &syntax::MatchPattern) -> semantic::MatchPattern {
    match pattern {
        syntax::MatchPattern::Type { type_name, span } => semantic::MatchPattern::Type {
            type_name: lower_type_name(type_name),
            span: span.clone(),
        },
        syntax::MatchPattern::Binding {
            name,
            name_span,
            type_name,
            span,
        } => semantic::MatchPattern::Binding {
            name: name.clone(),
            name_span: name_span.clone(),
            type_name: lower_type_name(type_name),
            span: span.clone(),
        },
    }
}

fn lower_type_name(type_name: &syntax::TypeName) -> semantic::TypeName {
    semantic::TypeName {
        names: type_name
            .names
            .iter()
            .map(|atom| semantic::TypeNameAtom {
                name: atom.name.clone(),
                span: atom.span.clone(),
            })
            .collect(),
        span: type_name.span.clone(),
    }
}
