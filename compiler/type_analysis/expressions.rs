use std::collections::HashMap;

use compiler__semantic_program::{
    SemanticBinaryOperator, SemanticExpression, SemanticMatchArm, SemanticMatchPattern,
    SemanticStructLiteralField, SemanticTypeName, SemanticUnaryOperator,
};
use compiler__source::Span;

use compiler__semantic_types::{GenericTypeParameter, NominalTypeId, Type};

use super::{
    ExpressionSpan, MethodKey, TypeAnnotatedCallTarget, TypeAnnotatedEnumVariantReference,
    TypeAnnotatedStructReference, TypeChecker, TypeKind,
};

struct InstantiatedFunctionSignature {
    parameter_types: Vec<Type>,
    return_type: Type,
    resolved_type_arguments: Vec<Type>,
}

struct ResolvedCallTarget {
    display_name: String,
    parameter_types: Vec<Type>,
    return_type: Type,
    resolved_type_arguments: Vec<Type>,
    call_target: Option<TypeAnnotatedCallTarget>,
}

struct ResolvedStructFields {
    struct_display_name: String,
    struct_reference: TypeAnnotatedStructReference,
    fields: Vec<(String, Type)>,
}

impl TypeChecker<'_> {
    pub(super) fn check_expression(&mut self, expression: &SemanticExpression) -> Type {
        match expression {
            SemanticExpression::IntegerLiteral { .. } => Type::Integer64,
            SemanticExpression::NilLiteral { .. } => Type::Nil,
            SemanticExpression::BooleanLiteral { .. } => Type::Boolean,
            SemanticExpression::StringLiteral { .. } => Type::String,
            SemanticExpression::NameReference {
                id,
                name,
                kind,
                span,
                ..
            } => self.check_name_reference_expression(*id, name, *kind, span),
            SemanticExpression::StructLiteral {
                type_name,
                fields,
                span: _,
                ..
            } => self.check_struct_literal(expression, type_name, fields),
            SemanticExpression::FieldAccess {
                id,
                target,
                field,
                field_span,
                ..
            } => {
                if let SemanticExpression::NameReference { name, .. } = target.as_ref() {
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
                            self.enum_variant_reference_by_expression_id.insert(
                                *id,
                                TypeAnnotatedEnumVariantReference {
                                    enum_name: name.clone(),
                                    variant_name: field.clone(),
                                },
                            );
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
            SemanticExpression::Call {
                callee,
                type_arguments,
                arguments,
                span,
                ..
            } => {
                let argument_types = arguments
                    .iter()
                    .map(|argument| self.check_expression(argument))
                    .collect::<Vec<_>>();
                let resolved_target = if let SemanticExpression::NameReference {
                    name, span, ..
                } = callee.as_ref()
                {
                    if self.name_reference_resolves_to_value_binding(name) {
                        None
                    } else if let Some(info) = self.functions.get(name).cloned() {
                        let instantiated = self.instantiate_function_call_signature(
                            name,
                            &info.type_parameters,
                            &info.parameter_types,
                            &info.return_type,
                            type_arguments,
                            &argument_types,
                            span,
                        );
                        Some(ResolvedCallTarget {
                            display_name: name.clone(),
                            parameter_types: instantiated.parameter_types,
                            return_type: instantiated.return_type,
                            resolved_type_arguments: instantiated.resolved_type_arguments,
                            call_target: Some(info.call_target.clone()),
                        })
                    } else if let Some(info) = self.imported_functions.get(name).cloned() {
                        self.mark_import_used(name);
                        let instantiated = self.instantiate_function_call_signature(
                            name,
                            &info.type_parameters,
                            &info.parameter_types,
                            &info.return_type,
                            type_arguments,
                            &argument_types,
                            span,
                        );
                        Some(ResolvedCallTarget {
                            display_name: name.clone(),
                            parameter_types: instantiated.parameter_types,
                            return_type: instantiated.return_type,
                            resolved_type_arguments: instantiated.resolved_type_arguments,
                            call_target: Some(info.call_target.clone()),
                        })
                    } else {
                        if self.imported_bindings.contains_key(name) {
                            self.mark_import_used(name);
                        }
                        self.error(format!("unknown function '{name}'"), span.clone());
                        return Type::Unknown;
                    }
                } else if let SemanticExpression::FieldAccess {
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
                    let (receiver_type_id, receiver_type_name, receiver_type_arguments) =
                        match &receiver_type {
                            Type::Named(named) => {
                                (named.id.clone(), named.display_name.clone(), Vec::new())
                            }
                            Type::Applied { base, arguments } => {
                                (base.id.clone(), receiver_type.display(), arguments.clone())
                            }
                            _ => {
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
                                return Type::Unknown;
                            }
                        };

                    let method_key = MethodKey {
                        receiver_type_id: receiver_type_id.clone(),
                        method_name: field.clone(),
                    };
                    if let Some((method_self_mutable, method_parameter_types, method_return_type)) =
                        self.methods.get(&method_key).map(|info| {
                            (
                                info.self_mutable,
                                info.parameter_types.clone(),
                                info.return_type.clone(),
                            )
                        })
                    {
                        let instantiated_signature = self.instantiate_method_call_signature(
                            &receiver_type_id,
                            &receiver_type_arguments,
                            &method_parameter_types,
                            &method_return_type,
                            field_span,
                        );
                        let method_parameter_types = instantiated_signature.parameter_types;
                        let method_return_type = instantiated_signature.return_type;
                        if method_self_mutable {
                            if let SemanticExpression::NameReference { name, .. } = target.as_ref()
                            {
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
                                    return Type::Unknown;
                                }
                            } else {
                                self.error(
                                    format!(
                                        "cannot call mutating method '{receiver_type_name}.{field}' on non-binding receiver"
                                    ),
                                    field_span.clone(),
                                );
                                return Type::Unknown;
                            }
                        }
                        Some(ResolvedCallTarget {
                            display_name: field.clone(),
                            parameter_types: method_parameter_types,
                            return_type: method_return_type,
                            resolved_type_arguments: Vec::new(),
                            call_target: None,
                        })
                    } else {
                        self.error(
                            format!("unknown method '{receiver_type_name}.{field}'"),
                            field_span.clone(),
                        );
                        return Type::Unknown;
                    }
                } else {
                    None
                };

                let resolved_target = if let Some(resolved_target) = resolved_target {
                    resolved_target
                } else {
                    if !type_arguments.is_empty() {
                        self.error(
                            "type arguments are only allowed on direct function calls",
                            span.clone(),
                        );
                    }
                    let callee_type = self.check_expression(callee);
                    let Type::Function {
                        parameter_types,
                        return_type,
                    } = callee_type
                    else {
                        if callee_type != Type::Unknown {
                            self.error(
                                format!("cannot call value of type {}", callee_type.display()),
                                callee.span(),
                            );
                        }
                        return Type::Unknown;
                    };
                    ResolvedCallTarget {
                        display_name: "function value".to_string(),
                        parameter_types,
                        return_type: *return_type,
                        resolved_type_arguments: Vec::new(),
                        call_target: None,
                    }
                };
                if let Some(call_target) = &resolved_target.call_target {
                    self.call_target_by_expression_id.insert(
                        super::semantic_expression_id(expression),
                        call_target.clone(),
                    );
                }
                if !resolved_target.resolved_type_arguments.is_empty() {
                    let resolved_type_arguments = resolved_target
                        .resolved_type_arguments
                        .iter()
                        .map(super::type_annotated_resolved_type_argument_from_type)
                        .collect::<Option<Vec<_>>>();
                    if let Some(resolved_type_arguments) = resolved_type_arguments {
                        self.resolved_type_argument_types_by_expression_id.insert(
                            super::semantic_expression_id(expression),
                            resolved_type_arguments,
                        );
                    }
                }

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
                    let argument_type = argument_types.get(index).cloned().unwrap_or(Type::Unknown);
                    if let Some(expected_type) = resolved_target.parameter_types.get(index)
                        && *expected_type != Type::Unknown
                        && argument_type != Type::Unknown
                        && !self.is_assignable(&argument_type, expected_type)
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
            SemanticExpression::Binary {
                operator,
                left,
                right,
                span: _,
                ..
            } => {
                let left_type = self.check_expression(left);
                let right_type = self.check_expression(right);
                match operator {
                    SemanticBinaryOperator::Add
                    | SemanticBinaryOperator::Subtract
                    | SemanticBinaryOperator::Multiply
                    | SemanticBinaryOperator::Divide
                    | SemanticBinaryOperator::Modulo => {
                        if left_type != Type::Integer64 || right_type != Type::Integer64 {
                            self.error("arithmetic operators require int64 operands", left.span());
                            return Type::Unknown;
                        }
                        Type::Integer64
                    }
                    SemanticBinaryOperator::EqualEqual | SemanticBinaryOperator::NotEqual => {
                        if !self.are_comparable_for_equality(&left_type, &right_type)
                            && left_type != Type::Unknown
                            && right_type != Type::Unknown
                        {
                            self.error("equality operators require same type", left.span());
                            return Type::Unknown;
                        }
                        Type::Boolean
                    }
                    SemanticBinaryOperator::LessThan
                    | SemanticBinaryOperator::LessThanOrEqual
                    | SemanticBinaryOperator::GreaterThan
                    | SemanticBinaryOperator::GreaterThanOrEqual => {
                        if left_type != Type::Integer64 || right_type != Type::Integer64 {
                            self.error("comparison operators require int64 operands", left.span());
                            return Type::Unknown;
                        }
                        Type::Boolean
                    }
                    SemanticBinaryOperator::And | SemanticBinaryOperator::Or => {
                        if left_type != Type::Boolean || right_type != Type::Boolean {
                            self.error("boolean operators require boolean operands", left.span());
                            return Type::Unknown;
                        }
                        Type::Boolean
                    }
                }
            }
            SemanticExpression::Unary {
                operator,
                expression,
                ..
            } => {
                let value_type = self.check_expression(expression);
                match operator {
                    SemanticUnaryOperator::Not => {
                        if value_type != Type::Boolean && value_type != Type::Unknown {
                            self.error("not operator requires boolean operand", expression.span());
                            return Type::Unknown;
                        }
                        Type::Boolean
                    }
                    SemanticUnaryOperator::Negate => {
                        if value_type != Type::Integer64 && value_type != Type::Unknown {
                            self.error("unary minus requires int64 operand", expression.span());
                            return Type::Unknown;
                        }
                        Type::Integer64
                    }
                }
            }
            SemanticExpression::Match {
                target, arms, span, ..
            } => self.check_match_expression(target, arms, span),
            SemanticExpression::Matches {
                value,
                type_name,
                span: _,
                ..
            } => self.check_matches_expression(value, type_name),
        }
    }

    pub(super) fn check_matches_expression(
        &mut self,
        value: &SemanticExpression,
        type_name: &SemanticTypeName,
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
        target: &SemanticExpression,
        arms: &[SemanticMatchArm],
        span: &Span,
    ) -> Type {
        let target_type = self.check_expression(target);
        if arms.is_empty() {
            self.error("match must have at least one arm", span.clone());
            return Type::Unknown;
        }
        let target_variants = match &target_type {
            Type::Union(variants) => Some(variants.clone()),
            _ => None,
        };
        if arms.len() == 1 {
            let should_report_single_arm = match &target_variants {
                Some(variants) => variants.len() <= 1,
                None => target_type != Type::Unknown,
            };
            if should_report_single_arm {
                self.error("match must have at least two arms", span.clone());
                return Type::Unknown;
            }
        }
        if Self::is_boolean_membership_match(arms) {
            self.error(
                "use 'matches' for single-pattern boolean checks",
                span.clone(),
            );
        }

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
            if let SemanticMatchPattern::Binding {
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
                    && !self.is_assignable(&arm_type, expected_type)
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

    pub(super) fn resolve_match_pattern_type(&mut self, pattern: &SemanticMatchPattern) -> Type {
        match pattern {
            SemanticMatchPattern::Type { type_name, span } => {
                self.resolve_match_pattern_type_name(type_name, span)
            }
            SemanticMatchPattern::Binding {
                type_name, span, ..
            } => self.resolve_match_pattern_type_name(type_name, span),
        }
    }

    pub(super) fn resolve_match_pattern_type_name(
        &mut self,
        type_name: &SemanticTypeName,
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
        if matches!(resolved, Type::TypeParameter(_)) {
            self.error("match patterns must not use type parameters", span.clone());
            return Type::Unknown;
        }
        if matches!(resolved, Type::Applied { .. }) {
            self.error(
                "match patterns must not use instantiated generic types",
                span.clone(),
            );
            return Type::Unknown;
        }
        resolved
    }

    pub(super) fn check_struct_literal(
        &mut self,
        expression: &SemanticExpression,
        type_name: &SemanticTypeName,
        fields: &[SemanticStructLiteralField],
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
        self.struct_reference_by_expression_id.insert(
            super::semantic_expression_id(expression),
            resolved_struct_fields.struct_reference.clone(),
        );

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
        type_parameters: &[GenericTypeParameter],
        parameter_types: &[Type],
        return_type: &Type,
        type_arguments: &[SemanticTypeName],
        argument_types: &[Type],
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
                resolved_type_arguments: Vec::new(),
            };
        }
        if type_arguments.is_empty() {
            if let Some(inferred_type_arguments) = self.infer_function_type_arguments_from_call(
                function_name,
                type_parameters,
                parameter_types,
                argument_types,
                span,
            ) {
                self.check_type_argument_constraints(
                    function_name,
                    type_parameters,
                    &inferred_type_arguments,
                    span,
                );
                let substitutions: HashMap<String, Type> = type_parameters
                    .iter()
                    .map(|parameter| parameter.name.clone())
                    .zip(inferred_type_arguments.iter().cloned())
                    .collect();
                let instantiated_parameters = parameter_types
                    .iter()
                    .map(|parameter_type| Self::instantiate_type(parameter_type, &substitutions))
                    .collect();
                let instantiated_return = Self::instantiate_type(return_type, &substitutions);
                return InstantiatedFunctionSignature {
                    parameter_types: instantiated_parameters,
                    return_type: instantiated_return,
                    resolved_type_arguments: inferred_type_arguments,
                };
            }
            return InstantiatedFunctionSignature {
                parameter_types: parameter_types.to_vec(),
                return_type: return_type.clone(),
                resolved_type_arguments: Vec::new(),
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
                resolved_type_arguments: Vec::new(),
            };
        }

        let resolved_type_arguments = type_arguments
            .iter()
            .map(|argument| self.resolve_type_name(argument))
            .collect::<Vec<_>>();
        self.check_type_argument_constraints(
            function_name,
            type_parameters,
            &resolved_type_arguments,
            span,
        );
        let substitutions: HashMap<String, Type> = type_parameters
            .iter()
            .map(|parameter| parameter.name.clone())
            .zip(resolved_type_arguments.iter().cloned())
            .collect();
        let instantiated_parameters = parameter_types
            .iter()
            .map(|parameter_type| Self::instantiate_type(parameter_type, &substitutions))
            .collect();
        let instantiated_return = Self::instantiate_type(return_type, &substitutions);
        InstantiatedFunctionSignature {
            parameter_types: instantiated_parameters,
            return_type: instantiated_return,
            resolved_type_arguments,
        }
    }

    fn instantiate_method_call_signature(
        &mut self,
        receiver_type_id: &NominalTypeId,
        receiver_type_arguments: &[Type],
        parameter_types: &[Type],
        return_type: &Type,
        span: &Span,
    ) -> InstantiatedFunctionSignature {
        let Some(receiver_info) = self
            .types
            .values()
            .find(|info| info.nominal_type_id == *receiver_type_id)
        else {
            return InstantiatedFunctionSignature {
                parameter_types: parameter_types.to_vec(),
                return_type: return_type.clone(),
                resolved_type_arguments: Vec::new(),
            };
        };

        if receiver_info.type_parameters.is_empty() {
            return InstantiatedFunctionSignature {
                parameter_types: parameter_types.to_vec(),
                return_type: return_type.clone(),
                resolved_type_arguments: Vec::new(),
            };
        }

        if receiver_type_arguments.len() != receiver_info.type_parameters.len() {
            self.error(
                format!(
                    "receiver type expects {} type arguments, got {}",
                    receiver_info.type_parameters.len(),
                    receiver_type_arguments.len()
                ),
                span.clone(),
            );
            return InstantiatedFunctionSignature {
                parameter_types: parameter_types.to_vec(),
                return_type: return_type.clone(),
                resolved_type_arguments: Vec::new(),
            };
        }

        let substitutions: HashMap<String, Type> = receiver_info
            .type_parameters
            .iter()
            .map(|parameter| parameter.name.clone())
            .zip(receiver_type_arguments.iter().cloned())
            .collect();
        let instantiated_parameters = parameter_types
            .iter()
            .map(|parameter_type| Self::instantiate_type(parameter_type, &substitutions))
            .collect();
        let instantiated_return = Self::instantiate_type(return_type, &substitutions);
        InstantiatedFunctionSignature {
            parameter_types: instantiated_parameters,
            return_type: instantiated_return,
            resolved_type_arguments: receiver_type_arguments.to_vec(),
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
                    struct_reference: TypeAnnotatedStructReference {
                        package_path: info.package_path.clone(),
                        symbol_name: info.nominal_type_id.symbol_name.clone(),
                    },
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
                    .map(|parameter| parameter.name.clone())
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
                    struct_reference: TypeAnnotatedStructReference {
                        package_path: info.package_path.clone(),
                        symbol_name: info.nominal_type_id.symbol_name.clone(),
                    },
                    fields: instantiated_fields,
                })
            }
            _ => None,
        }
    }

    fn infer_function_type_arguments_from_call(
        &mut self,
        function_name: &str,
        type_parameters: &[GenericTypeParameter],
        parameter_types: &[Type],
        argument_types: &[Type],
        span: &Span,
    ) -> Option<Vec<Type>> {
        if parameter_types.len() != argument_types.len() {
            return None;
        }

        let mut inferred_by_type_parameter_name: HashMap<String, Type> = HashMap::new();
        let mut inconsistent_type_parameter_names = std::collections::BTreeSet::new();
        for (parameter_type, argument_type) in parameter_types.iter().zip(argument_types) {
            self.collect_type_parameter_inference_from_argument(
                parameter_type,
                argument_type,
                &mut inferred_by_type_parameter_name,
                &mut inconsistent_type_parameter_names,
            );
        }

        if !inconsistent_type_parameter_names.is_empty() {
            let inconsistent_names = inconsistent_type_parameter_names
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            self.error(
                format!(
                    "cannot infer consistent type arguments for generic function '{function_name}' (conflicting inferences for: {inconsistent_names})"
                ),
                span.clone(),
            );
            return Some(vec![Type::Unknown; type_parameters.len()]);
        }

        let missing_type_parameter_names = type_parameters
            .iter()
            .filter_map(|parameter| {
                if inferred_by_type_parameter_name.contains_key(&parameter.name) {
                    None
                } else {
                    Some(parameter.name.clone())
                }
            })
            .collect::<Vec<_>>();
        if !missing_type_parameter_names.is_empty() {
            self.error(
                format!(
                    "generic function '{function_name}' requires {} explicit type arguments",
                    type_parameters.len()
                ),
                span.clone(),
            );
            return None;
        }

        Some(
            type_parameters
                .iter()
                .map(|parameter| {
                    inferred_by_type_parameter_name
                        .get(&parameter.name)
                        .cloned()
                        .unwrap_or(Type::Unknown)
                })
                .collect(),
        )
    }

    fn collect_type_parameter_inference_from_argument(
        &self,
        parameter_type: &Type,
        argument_type: &Type,
        inferred_by_type_parameter_name: &mut HashMap<String, Type>,
        inconsistent_type_parameter_names: &mut std::collections::BTreeSet<String>,
    ) {
        match parameter_type {
            Type::TypeParameter(type_parameter_name) => {
                let Some(previous_inference) = inferred_by_type_parameter_name
                    .get(type_parameter_name)
                    .cloned()
                else {
                    inferred_by_type_parameter_name
                        .insert(type_parameter_name.clone(), argument_type.clone());
                    return;
                };
                if previous_inference == *argument_type {
                    return;
                }
                if self.is_assignable(&previous_inference, argument_type)
                    && self.is_assignable(argument_type, &previous_inference)
                {
                    return;
                }
                inconsistent_type_parameter_names.insert(type_parameter_name.clone());
            }
            Type::Applied {
                base: parameter_base,
                arguments: parameter_type_arguments,
            } => {
                let Type::Applied {
                    base: argument_base,
                    arguments: argument_type_arguments,
                } = argument_type
                else {
                    return;
                };
                if parameter_base.id != argument_base.id
                    || parameter_type_arguments.len() != argument_type_arguments.len()
                {
                    return;
                }
                for (nested_parameter_type, nested_argument_type) in
                    parameter_type_arguments.iter().zip(argument_type_arguments)
                {
                    self.collect_type_parameter_inference_from_argument(
                        nested_parameter_type,
                        nested_argument_type,
                        inferred_by_type_parameter_name,
                        inconsistent_type_parameter_names,
                    );
                }
            }
            Type::Function {
                parameter_types: parameter_parameter_types,
                return_type: parameter_return_type,
            } => {
                let Type::Function {
                    parameter_types: argument_parameter_types,
                    return_type: argument_return_type,
                } = argument_type
                else {
                    return;
                };
                if parameter_parameter_types.len() != argument_parameter_types.len() {
                    return;
                }
                for (nested_parameter_type, nested_argument_type) in parameter_parameter_types
                    .iter()
                    .zip(argument_parameter_types)
                {
                    self.collect_type_parameter_inference_from_argument(
                        nested_parameter_type,
                        nested_argument_type,
                        inferred_by_type_parameter_name,
                        inconsistent_type_parameter_names,
                    );
                }
                self.collect_type_parameter_inference_from_argument(
                    parameter_return_type,
                    argument_return_type,
                    inferred_by_type_parameter_name,
                    inconsistent_type_parameter_names,
                );
            }
            Type::Union(parameter_union_members) => {
                let Type::Union(argument_union_members) = argument_type else {
                    return;
                };
                if parameter_union_members.len() != argument_union_members.len() {
                    return;
                }
                for (nested_parameter_type, nested_argument_type) in
                    parameter_union_members.iter().zip(argument_union_members)
                {
                    self.collect_type_parameter_inference_from_argument(
                        nested_parameter_type,
                        nested_argument_type,
                        inferred_by_type_parameter_name,
                        inconsistent_type_parameter_names,
                    );
                }
            }
            Type::Integer64
            | Type::Boolean
            | Type::String
            | Type::Nil
            | Type::Never
            | Type::Named(_)
            | Type::Unknown => {}
        }
    }
}
