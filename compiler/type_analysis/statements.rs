use std::collections::HashMap;

use compiler__semantic_program::{
    SemanticAssignTarget, SemanticBlock, SemanticExpression, SemanticFunctionDeclaration,
    SemanticMethodDeclaration, SemanticStatement, SemanticTypeDeclaration,
    SemanticTypeDeclarationKind,
};
use compiler__semantic_types::{NominalTypeId, NominalTypeRef, Type};

use super::{ExpressionSpan, FallthroughNarrowing, StatementOutcome, StatementSpan, TypeChecker};

impl TypeChecker<'_> {
    pub(super) fn check_function(&mut self, function: &SemanticFunctionDeclaration) {
        let names_and_spans = function
            .type_parameters
            .iter()
            .map(|parameter| (parameter.name.clone(), parameter.span.clone()))
            .collect::<Vec<_>>();
        self.push_type_parameters(&names_and_spans);
        self.scopes.push(HashMap::new());

        let (parameter_types, return_type) = if let Some(info) = self.functions.get(&function.name)
        {
            (info.parameter_types.clone(), info.return_type.clone())
        } else {
            (Vec::new(), self.resolve_type_name(&function.return_type))
        };
        self.current_return_type = return_type;

        for (index, parameter) in function.parameters.iter().enumerate() {
            self.check_parameter_name(&parameter.name, &parameter.name_span);
            let value_type = parameter_types.get(index).cloned().unwrap_or(Type::Unknown);
            self.define_variable(
                parameter.name.clone(),
                value_type,
                parameter.mutable,
                &parameter.span,
                parameter.name_span.clone(),
            );
        }

        let body_returns = self.check_block(&function.body);

        self.check_unused_in_current_scope();
        self.scopes.pop();
        self.pop_type_parameters();

        if !body_returns {
            self.error(
                "missing return in function body",
                function.body.span.clone(),
            );
        }
    }

    pub(super) fn check_methods(&mut self, types: &[SemanticTypeDeclaration]) {
        for type_declaration in types {
            let SemanticTypeDeclarationKind::Struct { methods, .. } = &type_declaration.kind else {
                continue;
            };
            for method in methods {
                self.check_method(type_declaration, method);
            }
        }
    }

    pub(super) fn check_method(
        &mut self,
        type_declaration: &SemanticTypeDeclaration,
        method: &SemanticMethodDeclaration,
    ) {
        let names_and_spans = type_declaration
            .type_parameters
            .iter()
            .map(|parameter| (parameter.name.clone(), parameter.span.clone()))
            .collect::<Vec<_>>();
        self.push_type_parameters(&names_and_spans);
        self.scopes.push(HashMap::new());

        let method_key = super::MethodKey {
            receiver_type_id: NominalTypeId {
                package_id: self.package_id,
                symbol_name: type_declaration.name.clone(),
            },
            method_name: method.name.clone(),
        };
        let (parameter_types, return_type) = if let Some(info) = self.methods.get(&method_key) {
            (info.parameter_types.clone(), info.return_type.clone())
        } else {
            (Vec::new(), self.resolve_type_name(&method.return_type))
        };
        self.current_return_type = return_type;

        let self_type = if type_declaration.type_parameters.is_empty() {
            Type::Named(NominalTypeRef {
                id: NominalTypeId {
                    package_id: self.package_id,
                    symbol_name: type_declaration.name.clone(),
                },
                display_name: type_declaration.name.clone(),
            })
        } else {
            Type::Applied {
                base: NominalTypeRef {
                    id: NominalTypeId {
                        package_id: self.package_id,
                        symbol_name: type_declaration.name.clone(),
                    },
                    display_name: type_declaration.name.clone(),
                },
                arguments: type_declaration
                    .type_parameters
                    .iter()
                    .map(|parameter| Type::TypeParameter(parameter.name.clone()))
                    .collect(),
            }
        };
        self.define_variable(
            "self".to_string(),
            self_type,
            method.self_mutable,
            &method.self_span,
            method.self_span.clone(),
        );
        if let Some(scope) = self.scopes.last_mut()
            && let Some(self_variable) = scope.get_mut("self")
        {
            self_variable.used = true;
        }

        for (index, parameter) in method.parameters.iter().enumerate() {
            self.check_parameter_name(&parameter.name, &parameter.name_span);
            let value_type = parameter_types.get(index).cloned().unwrap_or(Type::Unknown);
            self.define_variable(
                parameter.name.clone(),
                value_type,
                parameter.mutable,
                &parameter.span,
                parameter.name_span.clone(),
            );
        }

        let body_returns = self.check_block(&method.body);

        self.check_unused_in_current_scope();
        self.scopes.pop();
        self.pop_type_parameters();

        if !body_returns {
            self.error("missing return in function body", method.body.span.clone());
        }
    }

    pub(super) fn check_block(&mut self, block: &SemanticBlock) -> bool {
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

    pub(super) fn check_statement(&mut self, statement: &SemanticStatement) -> StatementOutcome {
        match statement {
            SemanticStatement::Binding {
                name,
                name_span,
                mutable,
                type_name,
                initializer,
                span,
                ..
            } => {
                self.check_variable_name(name, name_span);
                let value_type = self.check_expression(initializer);
                let mut binding_type = value_type.clone();
                let mut annotation_mismatch = false;
                if let Some(type_name) = type_name {
                    let annotated_type = self.resolve_type_name(type_name);
                    if annotated_type != Type::Unknown
                        && value_type != Type::Unknown
                        && !self.is_assignable(&value_type, &annotated_type)
                    {
                        self.error(
                            format!(
                                "type mismatch: expected {}, got {}",
                                annotated_type.display(),
                                value_type.display()
                            ),
                            initializer.span(),
                        );
                        annotation_mismatch = true;
                    }
                    if annotated_type != Type::Unknown && !annotation_mismatch {
                        binding_type = annotated_type;
                    }
                }
                if binding_type == Type::Never {
                    return StatementOutcome {
                        terminates: true,
                        fallthrough_narrowing: None,
                    };
                }
                self.define_variable(
                    name.clone(),
                    binding_type,
                    *mutable,
                    span,
                    name_span.clone(),
                );
                StatementOutcome {
                    terminates: false,
                    fallthrough_narrowing: None,
                }
            }
            SemanticStatement::Assign { target, value, .. } => {
                let value_type = self.check_expression(value);
                match target {
                    SemanticAssignTarget::Name {
                        name, name_span, ..
                    } => {
                        if let Some((is_mutable, variable_type)) =
                            self.lookup_variable_for_assignment(name)
                        {
                            if !is_mutable {
                                self.error(
                                    format!("cannot assign to immutable binding '{name}'"),
                                    name_span.clone(),
                                );
                            } else if variable_type != Type::Unknown
                                && value_type != Type::Unknown
                                && !self.is_assignable(&value_type, &variable_type)
                            {
                                self.error(
                                    format!(
                                        "assignment type mismatch: expected {}, got {}",
                                        variable_type.display(),
                                        value_type.display()
                                    ),
                                    value.span(),
                                );
                            }
                        } else if self.constants.contains_key(name) {
                            self.error(
                                format!("cannot assign to constant '{name}'"),
                                name_span.clone(),
                            );
                        } else if self.imported_constant_type(name).is_some() {
                            self.mark_import_used(name);
                            self.error(
                                format!("cannot assign to constant '{name}'"),
                                name_span.clone(),
                            );
                        } else {
                            self.error(format!("unknown name '{name}'"), name_span.clone());
                        }
                    }
                    SemanticAssignTarget::Index {
                        target,
                        index,
                        span: _,
                    } => {
                        if let Some(binding_name) = Self::assignment_root_binding_name(target) {
                            let receiver_is_mutable = self
                                .lookup_variable_for_assignment(binding_name)
                                .is_some_and(|(is_mutable, _)| is_mutable);
                            if !receiver_is_mutable
                                && (self.constants.contains_key(binding_name)
                                    || self.lookup_variable_type(binding_name).is_some())
                            {
                                self.error(
                                    format!(
                                        "cannot index-assign through immutable binding '{binding_name}'"
                                    ),
                                    target.span(),
                                );
                            }
                        } else {
                            self.error(
                                "cannot index-assign through non-binding receiver",
                                target.span(),
                            );
                        }
                        let target_type = self.check_expression(target);
                        let index_type = self.check_expression(index);
                        if index_type != Type::Integer64 && index_type != Type::Unknown {
                            self.error("list index must be int64", index.span());
                        }
                        if let Type::List(element_type) = target_type {
                            if value_type != Type::Unknown
                                && !self.is_assignable(&value_type, &element_type)
                            {
                                self.error(
                                    format!(
                                        "indexed assignment type mismatch: expected {}, got {}",
                                        element_type.display(),
                                        value_type.display()
                                    ),
                                    value.span(),
                                );
                            }
                        } else if target_type != Type::Unknown {
                            self.error(
                                format!(
                                    "cannot index-assign non-list type {}",
                                    target_type.display()
                                ),
                                target.span(),
                            );
                        }
                    }
                }
                StatementOutcome {
                    terminates: false,
                    fallthrough_narrowing: None,
                }
            }
            SemanticStatement::Return { value, span: _ } => {
                let value_type = self.check_expression(value);
                if self.current_return_type != Type::Unknown
                    && value_type != Type::Unknown
                    && !self.is_assignable(&value_type, &self.current_return_type)
                {
                    self.error(
                        format!(
                            "return type mismatch: expected {}, got {}",
                            self.current_return_type.display(),
                            value_type.display()
                        ),
                        value.span(),
                    );
                }
                StatementOutcome {
                    terminates: true,
                    fallthrough_narrowing: None,
                }
            }
            SemanticStatement::Break { span } => {
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
            SemanticStatement::Continue { span } => {
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
            SemanticStatement::If {
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
            SemanticStatement::For {
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
            SemanticStatement::Expression { value, .. } => {
                let value_type = self.check_expression(value);
                if !matches!(value, SemanticExpression::Call { .. }) && value_type != Type::Unknown
                {
                    self.error("expression statements must be calls", value.span());
                }
                StatementOutcome {
                    terminates: value_type == Type::Never,
                    fallthrough_narrowing: None,
                }
            }
        }
    }

    fn assignment_root_binding_name(target: &SemanticExpression) -> Option<&str> {
        match target {
            SemanticExpression::NameReference { name, .. } => Some(name),
            SemanticExpression::FieldAccess { target, .. }
            | SemanticExpression::IndexAccess { target, .. } => {
                Self::assignment_root_binding_name(target)
            }
            _ => None,
        }
    }
}
