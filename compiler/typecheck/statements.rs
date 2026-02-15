use std::collections::HashMap;

use crate::types::Type;
use compiler__syntax::{
    Block, Expression, FunctionDeclaration, MethodDeclaration, Statement, TypeDeclaration,
    TypeDeclarationKind,
};

use super::{ExpressionSpan, FallthroughNarrowing, StatementOutcome, StatementSpan, TypeChecker};

impl TypeChecker<'_> {
    pub(super) fn check_function(&mut self, function: &FunctionDeclaration) {
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

    pub(super) fn check_methods(&mut self, types: &[TypeDeclaration]) {
        for type_declaration in types {
            let TypeDeclarationKind::Struct { methods, .. } = &type_declaration.kind else {
                continue;
            };
            for method in methods {
                self.check_method(type_declaration, method);
            }
        }
    }

    pub(super) fn check_method(
        &mut self,
        type_declaration: &TypeDeclaration,
        method: &MethodDeclaration,
    ) {
        self.scopes.push(HashMap::new());

        let method_key = super::MethodKey {
            receiver_type_name: type_declaration.name.clone(),
            method_name: method.name.clone(),
        };
        let (parameter_types, return_type) = if let Some(info) = self.methods.get(&method_key) {
            (info.parameter_types.clone(), info.return_type.clone())
        } else {
            (Vec::new(), self.resolve_type_name(&method.return_type))
        };
        self.current_return_type = return_type;

        self.define_variable(
            "self".to_string(),
            Type::Named(type_declaration.name.clone()),
            method.self_mutable,
            method.self_span.clone(),
        );
        if let Some(scope) = self.scopes.last_mut()
            && let Some(self_variable) = scope.get_mut("self")
        {
            self_variable.used = true;
        }

        for (index, parameter) in method.parameters.iter().enumerate() {
            self.check_parameter_name(&parameter.name, &parameter.span);
            let value_type = parameter_types.get(index).cloned().unwrap_or(Type::Unknown);
            self.define_variable(
                parameter.name.clone(),
                value_type,
                false,
                parameter.span.clone(),
            );
        }

        let body_returns = self.check_block(&method.body);

        self.check_unused_in_current_scope();
        self.scopes.pop();

        if !body_returns {
            self.error("missing return in function body", method.body.span.clone());
        }
    }

    pub(super) fn check_block(&mut self, block: &Block) -> bool {
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

    pub(super) fn check_statement(&mut self, statement: &Statement) -> StatementOutcome {
        match statement {
            Statement::Let {
                name,
                mutable,
                type_name,
                initializer,
                span,
                ..
            } => {
                self.check_variable_name(name, span);
                let value_type = self.check_expression(initializer);
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
                            initializer.span(),
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
                value,
                ..
            } => {
                let value_type = self.check_expression(value);
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
                StatementOutcome {
                    terminates: false,
                    fallthrough_narrowing: None,
                }
            }
            Statement::Return { value, span: _ } => {
                let value_type = self.check_expression(value);
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
                        value.span(),
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
            Statement::Expression { value, .. } => {
                let _ = self.check_expression(value);
                if !matches!(value, Expression::Call { .. }) {
                    self.error("expression statements must be calls", value.span());
                }
                StatementOutcome {
                    terminates: false,
                    fallthrough_narrowing: None,
                }
            }
        }
    }
}
