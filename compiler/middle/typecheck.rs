use std::collections::HashMap;

use compiler__frontend::{
    BinaryOperator, ConstantDeclaration, Diagnostic, Expression, File, Span, Statement,
};

use crate::types::{Type, type_from_name};

#[must_use]
pub fn check_file(file: &File) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut checker = Checker::new(&mut diagnostics);
    checker.collect_function_signatures(&file.function_declarations);
    checker.check_constant_declarations(&file.constant_declarations);
    for function in &file.function_declarations {
        checker.check_function(function);
    }
    diagnostics
}

struct VariableInfo {
    value_type: Type,
    used: bool,
    span: Span,
}

struct ConstantInfo {
    value_type: Type,
}

struct FunctionInfo {
    parameter_types: Vec<Type>,
    return_type: Type,
}

struct Checker<'a> {
    constants: HashMap<String, ConstantInfo>,
    functions: HashMap<String, FunctionInfo>,
    scopes: Vec<HashMap<String, VariableInfo>>,
    diagnostics: &'a mut Vec<Diagnostic>,
    current_return_type: Type,
    saw_return: bool,
}

impl<'a> Checker<'a> {
    fn new(diagnostics: &'a mut Vec<Diagnostic>) -> Self {
        Self {
            constants: HashMap::new(),
            functions: HashMap::new(),
            scopes: Vec::new(),
            diagnostics,
            current_return_type: Type::Unknown,
            saw_return: false,
        }
    }

    fn collect_function_signatures(
        &mut self,
        functions: &[compiler__frontend::FunctionDeclaration],
    ) {
        for function in functions {
            if self.functions.contains_key(&function.name) {
                self.error(
                    format!("duplicate function '{}'", function.name),
                    function.span.clone(),
                );
                continue;
            }

            let return_type = type_from_name(&function.return_type.name).unwrap_or(Type::Unknown);
            if return_type == Type::Unknown {
                self.error(
                    format!("unknown return type '{}'", function.return_type.name),
                    function.return_type.span.clone(),
                );
            }

            let mut parameter_types = Vec::new();
            for parameter in &function.parameters {
                let value_type = type_from_name(&parameter.type_name.name).unwrap_or(Type::Unknown);
                if value_type == Type::Unknown {
                    self.error(
                        format!("unknown type '{}'", parameter.type_name.name),
                        parameter.type_name.span.clone(),
                    );
                }
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

    fn check_function(&mut self, function: &compiler__frontend::FunctionDeclaration) {
        self.scopes.push(HashMap::new());
        self.saw_return = false;

        let (parameter_types, return_type) = if let Some(info) = self.functions.get(&function.name)
        {
            (info.parameter_types.clone(), info.return_type.clone())
        } else {
            (
                Vec::new(),
                type_from_name(&function.return_type.name).unwrap_or(Type::Unknown),
            )
        };
        self.current_return_type = return_type;

        for (index, parameter) in function.parameters.iter().enumerate() {
            let value_type = parameter_types.get(index).cloned().unwrap_or(Type::Unknown);
            self.define_variable(parameter.name.clone(), value_type, parameter.span.clone());
        }

        self.check_block(&function.body);

        self.check_unused_in_current_scope();
        self.scopes.pop();

        if !self.saw_return {
            self.error(
                "missing return in function body",
                function.body.span.clone(),
            );
        }
    }

    fn check_block(&mut self, block: &compiler__frontend::Block) {
        self.scopes.push(HashMap::new());
        for statement in &block.statements {
            self.check_statement(statement);
        }
        self.check_unused_in_current_scope();
        self.scopes.pop();
    }

    fn check_statement(&mut self, statement: &Statement) {
        match statement {
            Statement::Let {
                name,
                expression,
                span,
                ..
            } => {
                let value_type = self.check_expression(expression);
                self.define_variable(name.clone(), value_type, span.clone());
            }
            Statement::Return {
                expression,
                span: _,
            } => {
                let value_type = self.check_expression(expression);
                if self.current_return_type != Type::Unknown
                    && value_type != Type::Unknown
                    && value_type != self.current_return_type
                {
                    self.error(
                        format!(
                            "return type mismatch: expected {}, got {}",
                            self.current_return_type.name(),
                            value_type.name()
                        ),
                        expression.span(),
                    );
                }
                self.saw_return = true;
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
                self.check_block(then_block);
                if let Some(else_block) = else_block {
                    self.check_block(else_block);
                }
            }
        }
    }

    fn check_expression(&mut self, expression: &Expression) -> Type {
        match expression {
            Expression::IntegerLiteral { .. } => Type::Integer64,
            Expression::BooleanLiteral { .. } => Type::Boolean,
            Expression::StringLiteral { .. } => Type::String,
            Expression::Identifier { name, span } => self.resolve_variable(name, span),
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
                        && argument_type != *expected_type
                    {
                        self.error(
                            format!(
                                "argument {} to '{}' must be {}, got {}",
                                index + 1,
                                function_name,
                                expected_type.name(),
                                argument_type.name()
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
                    BinaryOperator::EqualEqual => {
                        if left_type != right_type
                            && left_type != Type::Unknown
                            && right_type != Type::Unknown
                        {
                            self.error("== operands must have same type", left.span());
                            return Type::Unknown;
                        }
                        Type::Boolean
                    }
                }
            }
        }
    }

    fn define_variable(&mut self, name: String, value_type: Type, span: Span) {
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

    fn check_unused_in_current_scope(&mut self) {
        if let Some(scope) = self.scopes.last() {
            let mut unused = Vec::new();
            for (name, info) in scope {
                if info.used || name.starts_with('_') {
                    continue;
                }
                unused.push((name.clone(), info.span.clone()));
            }
            for (name, span) in unused {
                self.error(format!("unused variable '{name}'"), span);
            }
        }
    }

    fn error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics.push(Diagnostic::new(message, span));
    }
}

trait ExpressionSpan {
    fn span(&self) -> Span;
}

impl ExpressionSpan for Expression {
    fn span(&self) -> Span {
        match self {
            Expression::IntegerLiteral { span, .. }
            | Expression::BooleanLiteral { span, .. }
            | Expression::StringLiteral { span, .. }
            | Expression::Identifier { span, .. }
            | Expression::Call { span, .. }
            | Expression::Binary { span, .. } => span.clone(),
        }
    }
}
