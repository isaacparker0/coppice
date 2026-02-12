use std::collections::{HashMap, HashSet};

use compiler__frontend::{
    BinaryOperator, Block, ConstantDeclaration, Diagnostic, Expression, File, FunctionDeclaration,
    MatchArm, MatchPattern, Span, Statement, StructLiteralField, TypeDeclaration,
    TypeDeclarationKind, TypeName, UnaryOperator,
};

use crate::types::{Type, type_from_name};

#[must_use]
pub fn check_file(file: &File) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut checker = Checker::new(&mut diagnostics);
    checker.collect_type_declarations(&file.types);
    checker.collect_function_signatures(&file.functions);
    checker.check_constant_declarations(&file.constants);
    for function in &file.functions {
        checker.check_function(function);
    }
    diagnostics
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

struct TypeInfo {
    kind: TypeKind,
}

enum TypeKind {
    Struct { fields: Vec<(String, Type)> },
    Union { variants: Vec<Type> },
}

struct FunctionInfo {
    parameter_types: Vec<Type>,
    return_type: Type,
}

struct Checker<'a> {
    constants: HashMap<String, ConstantInfo>,
    types: HashMap<String, TypeInfo>,
    functions: HashMap<String, FunctionInfo>,
    scopes: Vec<HashMap<String, VariableInfo>>,
    diagnostics: &'a mut Vec<Diagnostic>,
    current_return_type: Type,
    loop_depth: usize,
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

impl<'a> Checker<'a> {
    fn new(diagnostics: &'a mut Vec<Diagnostic>) -> Self {
        Self {
            constants: HashMap::new(),
            types: HashMap::new(),
            functions: HashMap::new(),
            scopes: Vec::new(),
            diagnostics,
            current_return_type: Type::Unknown,
            loop_depth: 0,
        }
    }

    fn collect_type_declarations(&mut self, types: &[TypeDeclaration]) {
        for type_declaration in types {
            self.check_type_name(&type_declaration.name, &type_declaration.span);
            if self.types.contains_key(&type_declaration.name) {
                self.error(
                    format!("duplicate type '{}'", type_declaration.name),
                    type_declaration.span.clone(),
                );
                continue;
            }
            let kind = match &type_declaration.kind {
                TypeDeclarationKind::Struct { .. } => TypeKind::Struct { fields: Vec::new() },
                TypeDeclarationKind::Union { .. } => TypeKind::Union {
                    variants: Vec::new(),
                },
            };
            self.types
                .insert(type_declaration.name.clone(), TypeInfo { kind });
        }

        for type_declaration in types {
            match &type_declaration.kind {
                TypeDeclarationKind::Struct { fields, .. } => {
                    let mut resolved_fields = Vec::new();
                    let mut seen = HashSet::new();
                    for field in fields {
                        if !seen.insert(field.name.clone()) {
                            self.error(
                                format!(
                                    "duplicate field '{}' in '{}'",
                                    field.name, type_declaration.name
                                ),
                                field.span.clone(),
                            );
                            continue;
                        }
                        let field_type = self.resolve_type_name(&field.type_name);
                        resolved_fields.push((field.name.clone(), field_type));
                    }
                    if let Some(info) = self.types.get_mut(&type_declaration.name) {
                        info.kind = TypeKind::Struct {
                            fields: resolved_fields,
                        };
                    }
                }
                TypeDeclarationKind::Union { variants } => {
                    let mut resolved_variants = Vec::new();
                    let mut seen = HashSet::new();
                    for variant in variants {
                        if variant.names.len() != 1 {
                            self.error("union variants must be single types", variant.span.clone());
                            continue;
                        }
                        let variant_type = self.resolve_type_name(variant);
                        let key = variant_type.display();
                        if !seen.insert(key.clone()) {
                            self.error(
                                format!("duplicate union variant '{key}'"),
                                variant.span.clone(),
                            );
                            continue;
                        }
                        resolved_variants.push(variant_type);
                    }
                    if let Some(info) = self.types.get_mut(&type_declaration.name) {
                        info.kind = TypeKind::Union {
                            variants: resolved_variants,
                        };
                    }
                }
            }
        }
    }

    fn collect_function_signatures(&mut self, functions: &[FunctionDeclaration]) {
        for function in functions {
            self.check_function_name(&function.name, &function.name_span);
            if self.functions.contains_key(&function.name) {
                self.error(
                    format!("duplicate function '{}'", function.name),
                    function.name_span.clone(),
                );
                continue;
            }

            let return_type = self.resolve_type_name(&function.return_type);

            let mut parameter_types = Vec::new();
            for parameter in &function.parameters {
                let value_type = self.resolve_type_name(&parameter.type_name);
                parameter_types.push(value_type);
            }

            self.functions.insert(
                function.name.clone(),
                FunctionInfo {
                    parameter_types,
                    return_type,
                },
            );
        }
    }

    fn check_constant_declarations(&mut self, constants: &[ConstantDeclaration]) {
        for constant in constants {
            self.check_constant_name(&constant.name, &constant.span);
            let value_type = self.check_expression(&constant.expression);
            if self.constants.contains_key(&constant.name) {
                self.error(
                    format!("duplicate constant '{name}'", name = constant.name),
                    constant.span.clone(),
                );
                continue;
            }
            self.constants
                .insert(constant.name.clone(), ConstantInfo { value_type });
        }
    }

    fn check_function(&mut self, function: &FunctionDeclaration) {
        self.scopes.push(HashMap::new());

        let (parameter_types, return_type) = if let Some(info) = self.functions.get(&function.name)
        {
            (info.parameter_types.clone(), info.return_type.clone())
        } else {
            (Vec::new(), self.resolve_type_name(&function.return_type))
        };
        self.current_return_type = return_type;

        for (index, parameter) in function.parameters.iter().enumerate() {
            self.check_parameter_name(&parameter.name, &parameter.span);
            let value_type = parameter_types.get(index).cloned().unwrap_or(Type::Unknown);
            self.define_variable(
                parameter.name.clone(),
                value_type,
                false,
                parameter.span.clone(),
            );
        }

        let body_returns = self.check_block(&function.body);

        self.check_unused_in_current_scope();
        self.scopes.pop();

        if !body_returns {
            self.error(
                "missing return in function body",
                function.body.span.clone(),
            );
        }
    }

    fn check_block(&mut self, block: &Block) -> bool {
        self.scopes.push(HashMap::new());
        let mut falls_through = true;
        let mut has_reported_unreachable = false;
        for statement in &block.statements {
            if !falls_through {
                if !has_reported_unreachable {
                    self.error("unreachable code", statement.span());
                    has_reported_unreachable = true;
                }
                continue;
            }

            let outcome = self.check_statement(statement);
            if let Some(fallthrough_narrowing) = outcome.fallthrough_narrowing {
                self.apply_variable_narrowing(
                    &fallthrough_narrowing.variable_name,
                    fallthrough_narrowing.narrowed_type,
                );
            }
            if falls_through && outcome.terminates {
                falls_through = false;
            }
        }
        self.check_unused_in_current_scope();
        self.scopes.pop();
        !falls_through
    }

    fn check_statement(&mut self, statement: &Statement) -> StatementOutcome {
        match statement {
            Statement::Let {
                name,
                mutable,
                type_name,
                expression,
                span,
                ..
            } => {
                self.check_variable_name(name, span);
                let value_type = self.check_expression(expression);
                let mut binding_type = value_type.clone();
                let mut annotation_mismatch = false;
                if let Some(type_name) = type_name {
                    let annotated_type = self.resolve_type_name(type_name);
                    if annotated_type != Type::Unknown
                        && value_type != Type::Unknown
                        && !Self::is_assignable(&value_type, &annotated_type)
                    {
                        self.error(
                            format!(
                                "type mismatch: expected {}, got {}",
                                annotated_type.display(),
                                value_type.display()
                            ),
                            expression.span(),
                        );
                        annotation_mismatch = true;
                    }
                    if annotated_type != Type::Unknown && !annotation_mismatch {
                        binding_type = annotated_type;
                    }
                }
                self.define_variable(name.clone(), binding_type, *mutable, span.clone());
                StatementOutcome {
                    terminates: false,
                    fallthrough_narrowing: None,
                }
            }
            Statement::Assign {
                name,
                name_span,
                expression,
                ..
            } => {
                let value_type = self.check_expression(expression);
                if let Some((is_mutable, variable_type)) = self.lookup_variable_for_assignment(name)
                {
                    if !is_mutable {
                        self.error(
                            format!("cannot assign to immutable binding '{name}'"),
                            name_span.clone(),
                        );
                    } else if variable_type != Type::Unknown
                        && value_type != Type::Unknown
                        && !Self::is_assignable(&value_type, &variable_type)
                    {
                        self.error(
                            format!(
                                "assignment type mismatch: expected {}, got {}",
                                variable_type.display(),
                                value_type.display()
                            ),
                            expression.span(),
                        );
                    }
                } else if self.constants.contains_key(name) {
                    self.error(
                        format!("cannot assign to constant '{name}'"),
                        name_span.clone(),
                    );
                } else {
                    self.error(format!("unknown name '{name}'"), name_span.clone());
                }
                StatementOutcome {
                    terminates: false,
                    fallthrough_narrowing: None,
                }
            }
            Statement::Return {
                expression,
                span: _,
            } => {
                let value_type = self.check_expression(expression);
                if self.current_return_type != Type::Unknown
                    && value_type != Type::Unknown
                    && !Self::is_assignable(&value_type, &self.current_return_type)
                {
                    self.error(
                        format!(
                            "return type mismatch: expected {}, got {}",
                            self.current_return_type.display(),
                            value_type.display()
                        ),
                        expression.span(),
                    );
                }
                StatementOutcome {
                    terminates: true,
                    fallthrough_narrowing: None,
                }
            }
            Statement::Abort { message, .. } => {
                let message_type = self.check_expression(message);
                if message_type != Type::String && message_type != Type::Unknown {
                    self.error("abort message must be string", message.span());
                }
                StatementOutcome {
                    terminates: true,
                    fallthrough_narrowing: None,
                }
            }
            Statement::Break { span } => {
                if self.loop_depth == 0 {
                    self.error("break can only be used inside a loop", span.clone());
                    StatementOutcome {
                        terminates: false,
                        fallthrough_narrowing: None,
                    }
                } else {
                    StatementOutcome {
                        terminates: true,
                        fallthrough_narrowing: None,
                    }
                }
            }
            Statement::Continue { span } => {
                if self.loop_depth == 0 {
                    self.error("continue can only be used inside a loop", span.clone());
                    StatementOutcome {
                        terminates: false,
                        fallthrough_narrowing: None,
                    }
                } else {
                    StatementOutcome {
                        terminates: true,
                        fallthrough_narrowing: None,
                    }
                }
            }
            Statement::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                let condition_type = self.check_expression(condition);
                if condition_type != Type::Boolean && condition_type != Type::Unknown {
                    self.error("if condition must be boolean", condition.span());
                }
                let condition_type_narrowing = self.derive_condition_type_narrowing(condition);
                let then_branch_terminates = self.check_block_with_type_narrowing(
                    then_block,
                    condition_type_narrowing.as_ref(),
                    true,
                );
                let else_branch_terminates = else_block.as_ref().is_some_and(|block| {
                    self.check_block_with_type_narrowing(
                        block,
                        condition_type_narrowing.as_ref(),
                        false,
                    )
                });
                let fallthrough_narrowing = if then_branch_terminates && !else_branch_terminates {
                    condition_type_narrowing
                        .as_ref()
                        .map(|type_narrowing| FallthroughNarrowing {
                            variable_name: type_narrowing.name.clone(),
                            narrowed_type: type_narrowing.when_false.clone(),
                        })
                } else if !then_branch_terminates && else_branch_terminates {
                    condition_type_narrowing
                        .as_ref()
                        .map(|type_narrowing| FallthroughNarrowing {
                            variable_name: type_narrowing.name.clone(),
                            narrowed_type: type_narrowing.when_true.clone(),
                        })
                } else {
                    None
                };
                StatementOutcome {
                    terminates: then_branch_terminates && else_branch_terminates,
                    fallthrough_narrowing,
                }
            }
            Statement::For {
                condition, body, ..
            } => {
                if let Some(condition) = condition {
                    let condition_type = self.check_expression(condition);
                    if condition_type != Type::Boolean && condition_type != Type::Unknown {
                        self.error("for condition must be boolean", condition.span());
                    }
                }
                self.loop_depth += 1;
                let _ = self.check_block(body);
                self.loop_depth = self.loop_depth.saturating_sub(1);
                StatementOutcome {
                    terminates: false,
                    fallthrough_narrowing: None,
                }
            }
        }
    }

    fn check_expression(&mut self, expression: &Expression) -> Type {
        match expression {
            Expression::IntegerLiteral { .. } => Type::Integer64,
            Expression::NilLiteral { .. } => Type::Nil,
            Expression::BooleanLiteral { .. } => Type::Boolean,
            Expression::StringLiteral { .. } => Type::String,
            Expression::Identifier { name, span } => self.resolve_variable(name, span),
            Expression::StructLiteral {
                type_name,
                fields,
                span: _,
            } => self.check_struct_literal(type_name, fields),
            Expression::FieldAccess {
                target,
                field,
                field_span,
                ..
            } => {
                let target_type = self.check_expression(target);
                self.resolve_field_access_type(&target_type, field, field_span)
            }
            Expression::Call {
                callee,
                arguments,
                span,
            } => {
                let (function_name, name_span) =
                    if let Expression::Identifier { name, span } = callee.as_ref() {
                        (name.as_str(), span.clone())
                    } else {
                        self.error("invalid call target", callee.span());
                        for argument in arguments {
                            self.check_expression(argument);
                        }
                        return Type::Unknown;
                    };

                let (parameter_types, return_type) =
                    if let Some(info) = self.functions.get(function_name) {
                        (info.parameter_types.clone(), info.return_type.clone())
                    } else {
                        self.error(
                            format!("unknown function '{function_name}'"),
                            name_span.clone(),
                        );
                        for argument in arguments {
                            self.check_expression(argument);
                        }
                        return Type::Unknown;
                    };

                if arguments.len() != parameter_types.len() {
                    self.error(
                        format!(
                            "expected {} arguments, got {}",
                            parameter_types.len(),
                            arguments.len()
                        ),
                        span.clone(),
                    );
                }

                for (index, argument) in arguments.iter().enumerate() {
                    let argument_type = self.check_expression(argument);
                    if let Some(expected_type) = parameter_types.get(index)
                        && *expected_type != Type::Unknown
                        && argument_type != Type::Unknown
                        && !Self::is_assignable(&argument_type, expected_type)
                    {
                        self.error(
                            format!(
                                "argument {} to '{}' must be {}, got {}",
                                index + 1,
                                function_name,
                                expected_type.display(),
                                argument_type.display()
                            ),
                            argument.span(),
                        );
                    }
                }

                return_type
            }
            Expression::Binary {
                operator,
                left,
                right,
                span: _,
            } => {
                let left_type = self.check_expression(left);
                let right_type = self.check_expression(right);
                match operator {
                    BinaryOperator::Add
                    | BinaryOperator::Subtract
                    | BinaryOperator::Multiply
                    | BinaryOperator::Divide => {
                        if left_type != Type::Integer64 || right_type != Type::Integer64 {
                            self.error("arithmetic operators require int64 operands", left.span());
                            return Type::Unknown;
                        }
                        Type::Integer64
                    }
                    BinaryOperator::EqualEqual | BinaryOperator::NotEqual => {
                        if !Self::are_comparable_for_equality(&left_type, &right_type)
                            && left_type != Type::Unknown
                            && right_type != Type::Unknown
                        {
                            self.error("equality operators require same type", left.span());
                            return Type::Unknown;
                        }
                        Type::Boolean
                    }
                    BinaryOperator::LessThan
                    | BinaryOperator::LessThanOrEqual
                    | BinaryOperator::GreaterThan
                    | BinaryOperator::GreaterThanOrEqual => {
                        if left_type != Type::Integer64 || right_type != Type::Integer64 {
                            self.error("comparison operators require int64 operands", left.span());
                            return Type::Unknown;
                        }
                        Type::Boolean
                    }
                    BinaryOperator::And | BinaryOperator::Or => {
                        if left_type != Type::Boolean || right_type != Type::Boolean {
                            self.error("boolean operators require boolean operands", left.span());
                            return Type::Unknown;
                        }
                        Type::Boolean
                    }
                }
            }
            Expression::Unary {
                operator,
                expression,
                ..
            } => {
                let value_type = self.check_expression(expression);
                match operator {
                    UnaryOperator::Not => {
                        if value_type != Type::Boolean && value_type != Type::Unknown {
                            self.error("not operator requires boolean operand", expression.span());
                            return Type::Unknown;
                        }
                        Type::Boolean
                    }
                    UnaryOperator::Negate => {
                        if value_type != Type::Integer64 && value_type != Type::Unknown {
                            self.error("unary minus requires int64 operand", expression.span());
                            return Type::Unknown;
                        }
                        Type::Integer64
                    }
                }
            }
            Expression::Match { target, arms, span } => {
                self.check_match_expression(target, arms, span)
            }
            Expression::Matches {
                value,
                type_name,
                span: _,
            } => self.check_matches_expression(value, type_name),
        }
    }

    fn check_matches_expression(&mut self, value: &Expression, type_name: &TypeName) -> Type {
        let value_type = self.check_expression(value);
        let pattern_type = self.resolve_match_pattern_type_name(type_name, &type_name.span);
        if value_type == Type::Unknown || pattern_type == Type::Unknown {
            return Type::Boolean;
        }

        match &value_type {
            Type::Union(variants) => {
                if !variants.contains(&pattern_type) {
                    self.error(
                        format!(
                            "matches pattern type '{}' is not in target type",
                            pattern_type.display()
                        ),
                        type_name.span.clone(),
                    );
                }
            }
            _ => {
                if value_type != pattern_type {
                    self.error(
                        format!(
                            "matches pattern type '{}' does not match target type {}",
                            pattern_type.display(),
                            value_type.display()
                        ),
                        type_name.span.clone(),
                    );
                }
            }
        }

        Type::Boolean
    }

    fn check_match_expression(
        &mut self,
        target: &Expression,
        arms: &[MatchArm],
        span: &Span,
    ) -> Type {
        let target_type = self.check_expression(target);
        if arms.is_empty() {
            self.error("match must have at least one arm", span.clone());
            return Type::Unknown;
        }
        if Self::is_boolean_membership_match(arms) {
            self.error(
                "use 'matches' for single-pattern boolean checks",
                span.clone(),
            );
        }

        let target_variants = match &target_type {
            Type::Union(variants) => Some(variants.clone()),
            _ => None,
        };

        let mut seen_patterns = HashSet::new();
        let mut result_type: Option<Type> = None;

        for arm in arms {
            let pattern_type = self.resolve_match_pattern_type(&arm.pattern);
            if pattern_type != Type::Unknown && target_type != Type::Unknown {
                if let Some(variants) = &target_variants {
                    if !variants.contains(&pattern_type) {
                        self.error(
                            format!(
                                "match pattern type '{}' is not in target type",
                                pattern_type.display()
                            ),
                            arm.pattern.span(),
                        );
                    }
                } else if pattern_type != target_type {
                    self.error(
                        format!(
                            "match pattern type '{}' does not match target type {}",
                            pattern_type.display(),
                            target_type.display()
                        ),
                        arm.pattern.span(),
                    );
                }
            }

            if pattern_type != Type::Unknown {
                let pattern_key = pattern_type.display();
                if !seen_patterns.insert(pattern_key.clone()) {
                    self.error(
                        format!("duplicate match arm for type '{pattern_key}'"),
                        arm.pattern.span(),
                    );
                }
            }

            self.scopes.push(HashMap::new());
            if let MatchPattern::Binding {
                name, name_span, ..
            } = &arm.pattern
            {
                self.define_variable(name.clone(), pattern_type.clone(), false, name_span.clone());
            }

            let arm_type = self.check_expression(&arm.value);
            self.check_unused_in_current_scope();
            self.scopes.pop();

            if let Some(expected_type) = &result_type {
                if *expected_type != Type::Unknown
                    && arm_type != Type::Unknown
                    && !Self::is_assignable(&arm_type, expected_type)
                {
                    self.error(
                        format!(
                            "match arm type mismatch: expected {}, got {}",
                            expected_type.display(),
                            arm_type.display()
                        ),
                        arm.value.span(),
                    );
                }
            } else {
                result_type = Some(arm_type);
            }
        }

        if let Some(variants) = target_variants {
            let missing = variants
                .iter()
                .filter(|variant| !seen_patterns.contains(&variant.display()))
                .map(Type::display)
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                self.error(
                    format!("non-exhaustive match, missing: {}", missing.join(", ")),
                    span.clone(),
                );
            }
        }

        result_type.unwrap_or(Type::Unknown)
    }

    fn resolve_match_pattern_type(&mut self, pattern: &MatchPattern) -> Type {
        match pattern {
            MatchPattern::Type { type_name, span } => {
                self.resolve_match_pattern_type_name(type_name, span)
            }
            MatchPattern::Binding {
                type_name, span, ..
            } => self.resolve_match_pattern_type_name(type_name, span),
        }
    }

    fn resolve_match_pattern_type_name(&mut self, type_name: &TypeName, span: &Span) -> Type {
        if type_name.names.len() != 1 {
            self.error("match patterns must be single types", span.clone());
            return Type::Unknown;
        }
        let resolved = self.resolve_type_name(type_name);
        if matches!(resolved, Type::Union(_)) {
            self.error("match patterns must be concrete types", span.clone());
            return Type::Unknown;
        }
        resolved
    }

    fn check_struct_literal(
        &mut self,
        type_name: &TypeName,
        fields: &[StructLiteralField],
    ) -> Type {
        if type_name.names.len() != 1 {
            self.error(
                "struct literal requires a named struct type",
                type_name.span.clone(),
            );
            for field in fields {
                self.check_expression(&field.value);
            }
            return Type::Unknown;
        }

        let struct_type = self.resolve_type_name(type_name);
        let type_name_str = &type_name.names[0].name;
        let Some(info) = self.types.get(type_name_str) else {
            for field in fields {
                self.check_expression(&field.value);
            }
            return struct_type;
        };
        let field_defs = match &info.kind {
            TypeKind::Struct { fields } => fields.clone(),
            TypeKind::Union { .. } => {
                self.error(
                    format!("struct literal requires struct type, found '{type_name_str}'"),
                    type_name.span.clone(),
                );
                for field in fields {
                    self.check_expression(&field.value);
                }
                return struct_type;
            }
        };

        let mut seen = HashSet::new();
        for field in fields {
            if !seen.insert(field.name.as_str()) {
                self.error(
                    format!(
                        "duplicate field '{}' in {} literal",
                        field.name, type_name_str
                    ),
                    field.name_span.clone(),
                );
                self.check_expression(&field.value);
                continue;
            }

            let Some((_, field_type)) = field_defs.iter().find(|(name, _)| name == &field.name)
            else {
                self.error(
                    format!("unknown field '{}' on {}", field.name, type_name_str),
                    field.name_span.clone(),
                );
                self.check_expression(&field.value);
                continue;
            };

            let value_type = self.check_expression(&field.value);
            if *field_type != Type::Unknown
                && value_type != Type::Unknown
                && value_type != *field_type
            {
                self.error(
                    format!(
                        "field '{}' must be {}, got {}",
                        field.name,
                        field_type.display(),
                        value_type.display()
                    ),
                    field.value.span(),
                );
            }
        }

        for (field_name, _) in &field_defs {
            if !seen.contains(field_name.as_str()) {
                self.error(
                    format!("missing field '{field_name}' in {type_name_str} literal"),
                    type_name.span.clone(),
                );
            }
        }

        struct_type
    }

    fn resolve_field_access_type(&mut self, target_type: &Type, field: &str, span: &Span) -> Type {
        let Type::Named(type_name) = target_type else {
            if *target_type != Type::Unknown {
                self.error(
                    format!(
                        "cannot access field '{}' on non-struct type {}",
                        field,
                        target_type.display()
                    ),
                    span.clone(),
                );
            }
            return Type::Unknown;
        };

        let Some(info) = self.types.get(type_name) else {
            return Type::Unknown;
        };
        if let TypeKind::Struct { fields } = &info.kind {
            if let Some((_, field_type)) = fields.iter().find(|(name, _)| name == field) {
                return field_type.clone();
            }
        } else {
            self.error(
                format!("cannot access field '{field}' on non-struct type {type_name}"),
                span.clone(),
            );
            return Type::Unknown;
        }
        self.error(
            format!("unknown field '{field}' on {type_name}"),
            span.clone(),
        );
        Type::Unknown
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

    fn check_block_with_type_narrowing(
        &mut self,
        block: &Block,
        type_narrowing: Option<&BranchNarrowing>,
        use_true_branch: bool,
    ) -> bool {
        let restore = type_narrowing.and_then(|type_narrowing| {
            let narrowed_type = if use_true_branch {
                type_narrowing.when_true.clone()
            } else {
                type_narrowing.when_false.clone()
            };
            self.apply_variable_narrowing(&type_narrowing.name, narrowed_type)
        });

        let result = self.check_block(block);

        if let Some((scope_index, name, original_type)) = restore {
            self.restore_variable_type(scope_index, &name, original_type);
        }

        result
    }

    fn apply_variable_narrowing(
        &mut self,
        name: &str,
        narrowed_type: Type,
    ) -> Option<(usize, String, Type)> {
        if narrowed_type == Type::Unknown {
            return None;
        }
        for (scope_index, scope) in self.scopes.iter_mut().enumerate().rev() {
            if let Some(info) = scope.get_mut(name) {
                let original_type = info.value_type.clone();
                info.value_type = narrowed_type;
                return Some((scope_index, name.to_string(), original_type));
            }
        }
        None
    }

    fn restore_variable_type(&mut self, scope_index: usize, name: &str, original_type: Type) {
        if let Some(scope) = self.scopes.get_mut(scope_index)
            && let Some(info) = scope.get_mut(name)
        {
            info.value_type = original_type;
        }
    }

    fn derive_condition_type_narrowing(
        &mut self,
        condition: &Expression,
    ) -> Option<BranchNarrowing> {
        if let Expression::Binary {
            operator,
            left,
            right,
            ..
        } = condition
        {
            if *operator != BinaryOperator::EqualEqual && *operator != BinaryOperator::NotEqual {
                return None;
            }

            let (name, is_nil_test) = if let Expression::Identifier { name, .. } = left.as_ref() {
                (
                    name,
                    matches!(right.as_ref(), Expression::NilLiteral { .. }),
                )
            } else if let Expression::Identifier { name, .. } = right.as_ref() {
                (name, matches!(left.as_ref(), Expression::NilLiteral { .. }))
            } else {
                return None;
            };

            if !is_nil_test {
                return None;
            }

            let variable_type = self.lookup_variable_type(name)?;
            let non_nil_type = Self::without_type_member(&variable_type, &Type::Nil);

            let (when_true, when_false) = match *operator {
                BinaryOperator::EqualEqual => (Type::Nil, non_nil_type),
                BinaryOperator::NotEqual => (non_nil_type, Type::Nil),
                _ => return None,
            };
            return Some(BranchNarrowing {
                name: name.clone(),
                when_true,
                when_false,
            });
        }

        if let Expression::Matches {
            value,
            type_name,
            span: _,
        } = condition
        {
            let Expression::Identifier { name, .. } = value.as_ref() else {
                return None;
            };
            let pattern_type = self.resolve_match_pattern_type_name(type_name, &type_name.span);
            if pattern_type == Type::Unknown {
                return None;
            }
            let variable_type = self.lookup_variable_type(name)?;
            if let Type::Union(variants) = &variable_type
                && variants.contains(&pattern_type)
            {
                return Some(BranchNarrowing {
                    name: name.clone(),
                    when_true: pattern_type.clone(),
                    when_false: Self::without_type_member(&variable_type, &pattern_type),
                });
            }
            if variable_type == pattern_type {
                return Some(BranchNarrowing {
                    name: name.clone(),
                    when_true: pattern_type,
                    when_false: Type::Unknown,
                });
            }
        }

        None
    }

    fn is_boolean_membership_match(arms: &[MatchArm]) -> bool {
        let mut true_count = 0usize;
        let mut false_count = 0usize;
        for arm in arms {
            match &arm.value {
                Expression::BooleanLiteral { value: true, .. } => true_count += 1,
                Expression::BooleanLiteral { value: false, .. } => false_count += 1,
                _ => return false,
            }
        }
        true_count == 1 && false_count >= 1
    }

    fn lookup_variable_type(&self, name: &str) -> Option<Type> {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info.value_type.clone());
            }
        }
        None
    }

    fn without_type_member(value_type: &Type, removed_member: &Type) -> Type {
        match value_type {
            Type::Union(members) => {
                let filtered = members
                    .iter()
                    .filter(|member| *member != removed_member)
                    .cloned()
                    .collect::<Vec<_>>();
                if filtered.is_empty() {
                    Type::Unknown
                } else {
                    Self::normalize_union(filtered)
                }
            }
            _ => {
                if value_type == removed_member {
                    Type::Unknown
                } else {
                    value_type.clone()
                }
            }
        }
    }

    fn check_unused_in_current_scope(&mut self) {
        if let Some(scope) = self.scopes.last() {
            let mut unused = Vec::new();
            let mut used_with_ignored_prefix = Vec::new();
            for (name, info) in scope {
                if info.used && name.starts_with('_') {
                    used_with_ignored_prefix.push((name.clone(), info.span.clone()));
                    continue;
                }
                if info.used || name.starts_with('_') {
                    continue;
                }
                unused.push((name.clone(), info.span.clone()));
            }
            for (name, span) in used_with_ignored_prefix {
                self.error(
                    format!("bindings prefixed with '_' must be unused: '{name}' is used"),
                    span,
                );
            }
            for (name, span) in unused {
                self.error(format!("unused variable '{name}'"), span);
            }
        }
    }

    fn is_assignable(value_type: &Type, expected_type: &Type) -> bool {
        match expected_type {
            Type::Unknown => true,
            Type::Union(members) => match value_type {
                Type::Unknown => true,
                Type::Union(value_members) => value_members
                    .iter()
                    .all(|value_member| members.contains(value_member)),
                _ => members.contains(value_type),
            },
            _ => match value_type {
                Type::Unknown => true,
                Type::Union(_) => false,
                _ => value_type == expected_type,
            },
        }
    }

    fn are_comparable_for_equality(left_type: &Type, right_type: &Type) -> bool {
        Self::is_assignable(left_type, right_type) || Self::is_assignable(right_type, left_type)
    }

    fn error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics.push(Diagnostic::new(message, span));
    }

    fn check_type_name(&mut self, name: &str, span: &Span) {
        if !is_pascal_case(name) {
            self.error("type name must be PascalCase", span.clone());
        }
    }

    fn check_function_name(&mut self, name: &str, span: &Span) {
        if !is_camel_case(name) {
            self.error("function name must be camelCase", span.clone());
        }
    }

    fn check_constant_name(&mut self, name: &str, span: &Span) {
        if !is_upper_snake_case(name) {
            self.error("constant name must be UPPER_SNAKE_CASE", span.clone());
        }
    }

    fn check_variable_name(&mut self, name: &str, span: &Span) {
        if !is_camel_case_with_optional_leading_underscore(name) {
            self.error("variable name must be camelCase", span.clone());
        }
    }

    fn check_parameter_name(&mut self, name: &str, span: &Span) {
        if !is_camel_case_with_optional_leading_underscore(name) {
            self.error("parameter name must be camelCase", span.clone());
        }
    }

    fn resolve_type_name(&mut self, type_name: &TypeName) -> Type {
        let mut resolved = Vec::new();
        let mut has_unknown = false;
        for atom in &type_name.names {
            let name = atom.name.as_str();
            if let Some(builtin) = type_from_name(name) {
                resolved.push(builtin);
                continue;
            }
            if let Some(info) = self.types.get(name) {
                match &info.kind {
                    TypeKind::Struct { .. } => resolved.push(Type::Named(name.to_string())),
                    TypeKind::Union { variants } => {
                        resolved.push(Self::normalize_union(variants.clone()));
                    }
                }
                continue;
            }
            self.error(format!("unknown type '{name}'"), atom.span.clone());
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

    fn normalize_union(types: Vec<Type>) -> Type {
        let mut flat = Vec::new();
        let mut seen = HashSet::new();
        for value_type in types {
            if let Type::Union(inner) = value_type {
                for inner_type in inner {
                    let key = inner_type.display();
                    if seen.insert(key) {
                        flat.push(inner_type);
                    }
                }
            } else {
                let key = value_type.display();
                if seen.insert(key) {
                    flat.push(value_type);
                }
            }
        }
        if flat.len() == 1 {
            flat.remove(0)
        } else {
            Type::Union(flat)
        }
    }
}

fn is_pascal_case(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_uppercase() {
        return false;
    }
    if !is_alnum_no_underscore(first) {
        return false;
    }
    let mut previous_upper = true;
    for ch in chars {
        if !is_alnum_no_underscore(ch) {
            return false;
        }
        let is_upper = ch.is_ascii_uppercase();
        if previous_upper && is_upper {
            return false;
        }
        previous_upper = is_upper;
    }
    true
}

fn is_camel_case(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    if !is_alnum_no_underscore(first) {
        return false;
    }
    let mut previous_upper = false;
    for ch in chars {
        if !is_alnum_no_underscore(ch) {
            return false;
        }
        let is_upper = ch.is_ascii_uppercase();
        if previous_upper && is_upper {
            return false;
        }
        previous_upper = is_upper;
    }
    true
}

fn is_upper_snake_case(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_uppercase() {
        return false;
    }
    for ch in chars {
        if !(ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_') {
            return false;
        }
    }
    true
}

fn is_camel_case_with_optional_leading_underscore(name: &str) -> bool {
    if name.starts_with("__") {
        return false;
    }
    if let Some(rest) = name.strip_prefix('_') {
        if rest.is_empty() {
            return true;
        }
        return is_camel_case(rest);
    }
    is_camel_case(name)
}

fn is_alnum_no_underscore(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
}

trait ExpressionSpan {
    fn span(&self) -> Span;
}

trait StatementSpan {
    fn span(&self) -> Span;
}

impl ExpressionSpan for Expression {
    fn span(&self) -> Span {
        match self {
            Expression::IntegerLiteral { span, .. }
            | Expression::NilLiteral { span, .. }
            | Expression::BooleanLiteral { span, .. }
            | Expression::StringLiteral { span, .. }
            | Expression::Identifier { span, .. }
            | Expression::StructLiteral { span, .. }
            | Expression::FieldAccess { span, .. }
            | Expression::Call { span, .. }
            | Expression::Unary { span, .. }
            | Expression::Binary { span, .. }
            | Expression::Match { span, .. }
            | Expression::Matches { span, .. } => span.clone(),
        }
    }
}

impl StatementSpan for Statement {
    fn span(&self) -> Span {
        match self {
            Statement::Let { span, .. }
            | Statement::Assign { span, .. }
            | Statement::Return { span, .. }
            | Statement::Abort { span, .. }
            | Statement::If { span, .. }
            | Statement::For { span, .. }
            | Statement::Break { span, .. }
            | Statement::Continue { span, .. } => span.clone(),
        }
    }
}
