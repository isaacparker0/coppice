use std::collections::HashMap;

use compiler__frontend::{
    BinaryOperator, ConstantDeclaration, Diagnostic, Expression, File, Span, Statement,
};

use crate::types::{Type, type_from_name};

#[must_use]
pub fn check_file(file: &File) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut checker = Checker::new(&mut diagnostics);
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

struct Checker<'a> {
    constants: HashMap<String, ConstantInfo>,
    scopes: Vec<HashMap<String, VariableInfo>>,
    diagnostics: &'a mut Vec<Diagnostic>,
    current_return_type: Type,
    saw_return: bool,
}

impl<'a> Checker<'a> {
    fn new(diagnostics: &'a mut Vec<Diagnostic>) -> Self {
        Self {
            constants: HashMap::new(),
            scopes: Vec::new(),
            diagnostics,
            current_return_type: Type::Unknown,
            saw_return: false,
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

        let return_type = type_from_name(&function.return_type.name).unwrap_or(Type::Unknown);
        if return_type == Type::Unknown {
            self.error(
                format!("unknown return type '{}'", function.return_type.name),
                function.return_type.span.clone(),
            );
        }
        self.current_return_type = return_type;

        for parameter in &function.parameters {
            let value_type = type_from_name(&parameter.type_name.name).unwrap_or(Type::Unknown);
            if value_type == Type::Unknown {
                self.error(
                    format!("unknown type '{}'", parameter.type_name.name),
                    parameter.type_name.span.clone(),
                );
            }
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
                ..
            } => {
                let condition_type = self.check_expression(condition);
                if condition_type != Type::Boolean && condition_type != Type::Unknown {
                    self.error("if condition must be boolean", condition.span());
                }
                self.check_block(then_block);
            }
        }
    }

    fn check_expression(&mut self, expression: &Expression) -> Type {
        match expression {
            Expression::IntegerLiteral { .. } => Type::Integer64,
            Expression::BooleanLiteral { .. } => Type::Boolean,
            Expression::StringLiteral { .. } => Type::String,
            Expression::Identifier { name, span } => self.resolve_variable(name, span),
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
            | Expression::Binary { span, .. } => span.clone(),
        }
    }
}
