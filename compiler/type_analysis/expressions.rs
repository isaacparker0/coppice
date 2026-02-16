use std::collections::HashMap;

use compiler__semantic_program::{
    BinaryOperator, Expression, MatchArm, MatchPattern, StructLiteralField, TypeName, UnaryOperator,
};
use compiler__source::Span;

use compiler__semantic_types::Type;

use super::{ExpressionSpan, MethodKey, TypeChecker, TypeKind};

struct InstantiatedFunctionSignature {
    parameter_types: Vec<Type>,
    return_type: Type,
}

struct ResolvedCallTarget {
    display_name: String,
    parameter_types: Vec<Type>,
    return_type: Type,
}

struct ResolvedStructFields {
    struct_display_name: String,
    fields: Vec<(String, Type)>,
}

impl TypeChecker<'_> {
    pub(super) fn check_expression(&mut self, expression: &Expression) -> Type {
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
                if let Expression::Identifier { name, .. } = target.as_ref() {
                    let is_enum_like_union = self.types.get(name).is_some_and(|info| {
                        if let TypeKind::Union { variants } = &info.kind {
                            let enum_like_prefix = format!("{name}.");
                            !variants.is_empty()
                                && variants.iter().all(|variant| {
                                    matches!(
                                        variant,
                                        Type::Named(named)
                                            if named.display_name.starts_with(&enum_like_prefix)
                                    )
                                })
                        } else {
                            false
                        }
                    });
                    if is_enum_like_union {
                        if let Some(variant_type) = self.resolve_enum_variant_type(name, field) {
                            return variant_type;
                        }
                        self.error(
                            format!("unknown enum variant '{name}.{field}'"),
                            field_span.clone(),
                        );
                        return Type::Unknown;
                    }
                }
                let target_type = self.check_expression(target);
                self.resolve_field_access_type(&target_type, field, field_span)
            }
            Expression::Call {
                callee,
                type_arguments,
                arguments,
                span,
            } => {
                let resolved_target = if let Expression::Identifier { name, span } = callee.as_ref()
                {
                    if let Some(info) = self.functions.get(name).cloned() {
                        let instantiated = self.instantiate_function_call_signature(
                            name,
                            &info.type_parameters,
                            &info.parameter_types,
                            &info.return_type,
                            type_arguments,
                            span,
                        );
                        ResolvedCallTarget {
                            display_name: name.clone(),
                            parameter_types: instantiated.parameter_types,
                            return_type: instantiated.return_type,
                        }
                    } else if let Some(info) = self.imported_functions.get(name).cloned() {
                        self.mark_import_used(name);
                        let instantiated = self.instantiate_function_call_signature(
                            name,
                            &info.type_parameters,
                            &info.parameter_types,
                            &info.return_type,
                            type_arguments,
                            span,
                        );
                        ResolvedCallTarget {
                            display_name: name.clone(),
                            parameter_types: instantiated.parameter_types,
                            return_type: instantiated.return_type,
                        }
                    } else {
                        if self.imported_bindings.contains_key(name) {
                            self.mark_import_used(name);
                        }
                        self.error(format!("unknown function '{name}'"), span.clone());
                        for argument in arguments {
                            self.check_expression(argument);
                        }
                        return Type::Unknown;
                    }
                } else if let Expression::FieldAccess {
                    target,
                    field,
                    field_span,
                    ..
                } = callee.as_ref()
                {
                    if !type_arguments.is_empty() {
                        self.error("methods do not take type arguments", span.clone());
                    }
                    let receiver_type = self.check_expression(target);
                    let receiver_type = if let Type::Named(named) = &receiver_type {
                        named.clone()
                    } else {
                        if receiver_type != Type::Unknown {
                            self.error(
                                format!(
                                    "cannot call method '{}' on non-struct type {}",
                                    field,
                                    receiver_type.display()
                                ),
                                field_span.clone(),
                            );
                        }
                        for argument in arguments {
                            self.check_expression(argument);
                        }
                        return Type::Unknown;
                    };
                    let receiver_type_name = receiver_type.display_name.clone();

                    let method_key = MethodKey {
                        receiver_type_id: receiver_type.id.clone(),
                        method_name: field.clone(),
                    };
                    if let Some(info) = self.methods.get(&method_key) {
                        let method_self_mutable = info.self_mutable;
                        let method_parameter_types = info.parameter_types.clone();
                        let method_return_type = info.return_type.clone();
                        if method_self_mutable {
                            if let Expression::Identifier { name, .. } = target.as_ref() {
                                let receiver_is_mutable = self
                                    .lookup_variable_for_assignment(name)
                                    .is_some_and(|(is_mutable, _)| is_mutable);
                                if !receiver_is_mutable {
                                    if self.constants.contains_key(name)
                                        || self.lookup_variable_type(name).is_some()
                                    {
                                        self.error(
                                            format!(
                                                "cannot call mutating method '{receiver_type_name}.{field}' on immutable binding '{name}'"
                                            ),
                                            field_span.clone(),
                                        );
                                    }
                                    for argument in arguments {
                                        self.check_expression(argument);
                                    }
                                    return Type::Unknown;
                                }
                            } else {
                                self.error(
                                    format!(
                                        "cannot call mutating method '{receiver_type_name}.{field}' on non-binding receiver"
                                    ),
                                    field_span.clone(),
                                );
                                for argument in arguments {
                                    self.check_expression(argument);
                                }
                                return Type::Unknown;
                            }
                        }
                        ResolvedCallTarget {
                            display_name: field.clone(),
                            parameter_types: method_parameter_types,
                            return_type: method_return_type,
                        }
                    } else {
                        self.error(
                            format!("unknown method '{receiver_type_name}.{field}'"),
                            field_span.clone(),
                        );
                        for argument in arguments {
                            self.check_expression(argument);
                        }
                        return Type::Unknown;
                    }
                } else {
                    self.error("invalid call target", callee.span());
                    for argument in arguments {
                        self.check_expression(argument);
                    }
                    return Type::Unknown;
                };

                if arguments.len() != resolved_target.parameter_types.len() {
                    self.error(
                        format!(
                            "expected {} arguments, got {}",
                            resolved_target.parameter_types.len(),
                            arguments.len()
                        ),
                        span.clone(),
                    );
                }

                let callee_name = resolved_target.display_name.clone();
                for (index, argument) in arguments.iter().enumerate() {
                    let argument_type = self.check_expression(argument);
                    if let Some(expected_type) = resolved_target.parameter_types.get(index)
                        && *expected_type != Type::Unknown
                        && argument_type != Type::Unknown
                        && !Self::is_assignable(&argument_type, expected_type)
                    {
                        self.error(
                            format!(
                                "argument {} to '{}' must be {}, got {}",
                                index + 1,
                                callee_name,
                                expected_type.display(),
                                argument_type.display()
                            ),
                            argument.span(),
                        );
                    }
                }

                resolved_target.return_type
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

    pub(super) fn check_matches_expression(
        &mut self,
        value: &Expression,
        type_name: &TypeName,
    ) -> Type {
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

    pub(super) fn check_match_expression(
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

        let mut seen_patterns = std::collections::HashSet::new();
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

    pub(super) fn resolve_match_pattern_type(&mut self, pattern: &MatchPattern) -> Type {
        match pattern {
            MatchPattern::Type { type_name, span } => {
                self.resolve_match_pattern_type_name(type_name, span)
            }
            MatchPattern::Binding {
                type_name, span, ..
            } => self.resolve_match_pattern_type_name(type_name, span),
        }
    }

    pub(super) fn resolve_match_pattern_type_name(
        &mut self,
        type_name: &TypeName,
        span: &Span,
    ) -> Type {
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

    pub(super) fn check_struct_literal(
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
        let Some(resolved_struct_fields) = self.resolve_struct_fields(&struct_type) else {
            if struct_type != Type::Unknown {
                self.error(
                    format!(
                        "struct literal requires struct type, found '{}'",
                        struct_type.display()
                    ),
                    type_name.span.clone(),
                );
            }
            for field in fields {
                self.check_expression(&field.value);
            }
            return struct_type;
        };

        let mut seen = std::collections::HashSet::new();
        for field in fields {
            if !seen.insert(field.name.as_str()) {
                self.error(
                    format!(
                        "duplicate field '{}' in {} literal",
                        field.name, resolved_struct_fields.struct_display_name
                    ),
                    field.name_span.clone(),
                );
                self.check_expression(&field.value);
                continue;
            }

            let Some((_, field_type)) = resolved_struct_fields
                .fields
                .iter()
                .find(|(name, _)| name == &field.name)
            else {
                self.error(
                    format!(
                        "unknown field '{}' on {}",
                        field.name, resolved_struct_fields.struct_display_name
                    ),
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

        for (field_name, _) in &resolved_struct_fields.fields {
            if !seen.contains(field_name.as_str()) {
                self.error(
                    format!(
                        "missing field '{field_name}' in {} literal",
                        resolved_struct_fields.struct_display_name
                    ),
                    type_name.span.clone(),
                );
            }
        }

        struct_type
    }

    pub(super) fn resolve_field_access_type(
        &mut self,
        target_type: &Type,
        field: &str,
        span: &Span,
    ) -> Type {
        let Some(resolved_struct_fields) = self.resolve_struct_fields(target_type) else {
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

        if let Some((_, field_type)) = resolved_struct_fields
            .fields
            .iter()
            .find(|(name, _)| name == field)
        {
            return field_type.clone();
        }
        self.error(
            format!(
                "unknown field '{field}' on {}",
                resolved_struct_fields.struct_display_name
            ),
            span.clone(),
        );
        Type::Unknown
    }

    fn instantiate_function_call_signature(
        &mut self,
        function_name: &str,
        type_parameters: &[String],
        parameter_types: &[Type],
        return_type: &Type,
        type_arguments: &[TypeName],
        span: &Span,
    ) -> InstantiatedFunctionSignature {
        if type_parameters.is_empty() {
            if !type_arguments.is_empty() {
                self.error(
                    format!("function '{function_name}' does not take type arguments"),
                    span.clone(),
                );
            }
            return InstantiatedFunctionSignature {
                parameter_types: parameter_types.to_vec(),
                return_type: return_type.clone(),
            };
        }
        if type_arguments.is_empty() {
            self.error(
                format!(
                    "generic function '{function_name}' requires {} explicit type arguments",
                    type_parameters.len()
                ),
                span.clone(),
            );
            return InstantiatedFunctionSignature {
                parameter_types: parameter_types.to_vec(),
                return_type: return_type.clone(),
            };
        }
        if type_arguments.len() != type_parameters.len() {
            self.error(
                format!(
                    "function '{function_name}' expects {} type arguments, got {}",
                    type_parameters.len(),
                    type_arguments.len()
                ),
                span.clone(),
            );
            return InstantiatedFunctionSignature {
                parameter_types: parameter_types.to_vec(),
                return_type: return_type.clone(),
            };
        }

        let substitutions: HashMap<String, Type> = type_parameters
            .iter()
            .zip(type_arguments.iter())
            .map(|(name, argument)| (name.clone(), self.resolve_type_name(argument)))
            .collect();
        let instantiated_parameters = parameter_types
            .iter()
            .map(|parameter_type| Self::instantiate_type(parameter_type, &substitutions))
            .collect();
        let instantiated_return = Self::instantiate_type(return_type, &substitutions);
        InstantiatedFunctionSignature {
            parameter_types: instantiated_parameters,
            return_type: instantiated_return,
        }
    }

    fn resolve_struct_fields(&mut self, struct_type: &Type) -> Option<ResolvedStructFields> {
        match struct_type {
            Type::Named(type_name) => {
                let info = self
                    .types
                    .values()
                    .find(|info| info.nominal_type_id == type_name.id)?;
                let TypeKind::Struct { fields } = &info.kind else {
                    return None;
                };
                Some(ResolvedStructFields {
                    struct_display_name: type_name.display_name.clone(),
                    fields: fields.clone(),
                })
            }
            Type::Applied { base, arguments } => {
                let info = self
                    .types
                    .values()
                    .find(|info| info.nominal_type_id == base.id)?;
                let TypeKind::Struct { fields } = &info.kind else {
                    return None;
                };
                let substitutions: HashMap<String, Type> = info
                    .type_parameters
                    .iter()
                    .cloned()
                    .zip(arguments.iter().cloned())
                    .collect();
                let instantiated_fields = fields
                    .iter()
                    .map(|(name, field_type)| {
                        (
                            name.clone(),
                            Self::instantiate_type(field_type, &substitutions),
                        )
                    })
                    .collect();
                Some(ResolvedStructFields {
                    struct_display_name: struct_type.display(),
                    fields: instantiated_fields,
                })
            }
            _ => None,
        }
    }
}
