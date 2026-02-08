use std::collections::HashMap;

use frontend::{BinOp, Diagnostic, Expr, File, Span, Stmt};

use crate::types::{type_from_name, Type};

pub fn check_file(file: &File) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for func in &file.functions {
        let mut checker = Checker::new(&mut diags);
        checker.check_function(func);
    }
    diags
}

struct VarInfo {
    ty: Type,
    used: bool,
    span: Span,
}

struct Checker<'a> {
    scopes: Vec<HashMap<String, VarInfo>>,
    diags: &'a mut Vec<Diagnostic>,
    current_return: Type,
    saw_return: bool,
}

impl<'a> Checker<'a> {
    fn new(diags: &'a mut Vec<Diagnostic>) -> Self {
        Self {
            scopes: Vec::new(),
            diags,
            current_return: Type::Unknown,
            saw_return: false,
        }
    }

    fn check_function(&mut self, func: &frontend::Function) {
        self.scopes.push(HashMap::new());
        self.saw_return = false;

        let return_ty = type_from_name(&func.return_type.name).unwrap_or(Type::Unknown);
        if return_ty == Type::Unknown {
            self.error(
                format!("unknown return type '{}'", func.return_type.name),
                func.return_type.span.clone(),
            );
        }
        self.current_return = return_ty;

        for param in &func.params {
            let ty = type_from_name(&param.ty.name).unwrap_or(Type::Unknown);
            if ty == Type::Unknown {
                self.error(
                    format!("unknown type '{}'", param.ty.name),
                    param.ty.span.clone(),
                );
            }
            self.define_var(param.name.clone(), ty, param.span.clone());
        }

        self.check_block(&func.body);

        self.check_unused_in_current_scope();
        self.scopes.pop();

        if !self.saw_return {
            self.error(
                "missing return in function body",
                func.body.span.clone(),
            );
        }
    }

    fn check_block(&mut self, block: &frontend::Block) {
        self.scopes.push(HashMap::new());
        for stmt in &block.stmts {
            self.check_stmt(stmt);
        }
        self.check_unused_in_current_scope();
        self.scopes.pop();
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                name,
                expr,
                span,
                ..
            } => {
                let ty = self.check_expr(expr);
                self.define_var(name.clone(), ty, span.clone());
            }
            Stmt::Return { expr, span: _ } => {
                let ty = self.check_expr(expr);
                if self.current_return != Type::Unknown && ty != Type::Unknown && ty != self.current_return {
                    self.error(
                        format!(
                            "return type mismatch: expected {}, got {}",
                            self.current_return.name(),
                            ty.name()
                        ),
                        expr.span(),
                    );
                }
                self.saw_return = true;
            }
            Stmt::If {
                condition,
                then_block,
                ..
            } => {
                let cond_ty = self.check_expr(condition);
                if cond_ty != Type::Boolean && cond_ty != Type::Unknown {
                    self.error("if condition must be boolean", condition.span());
                }
                self.check_block(then_block);
            }
        }
    }

    fn check_expr(&mut self, expr: &Expr) -> Type {
        match expr {
            Expr::IntLiteral { .. } => Type::Int64,
            Expr::BoolLiteral { .. } => Type::Boolean,
            Expr::StringLiteral { .. } => Type::String,
            Expr::Ident { name, span } => self.resolve_var(name, span),
            Expr::Binary { op, left, right, span: _ } => {
                let l = self.check_expr(left);
                let r = self.check_expr(right);
                match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                        if l != Type::Int64 || r != Type::Int64 {
                            self.error(
                                "arithmetic operators require int64 operands",
                                left.span(),
                            );
                            return Type::Unknown;
                        }
                        Type::Int64
                    }
                    BinOp::EqEq => {
                        if l != r && l != Type::Unknown && r != Type::Unknown {
                            self.error("== operands must have same type", left.span());
                            return Type::Unknown;
                        }
                        Type::Boolean
                    }
                }
            }
        }
    }

    fn define_var(&mut self, name: String, ty: Type, span: Span) {
        let duplicate = self
            .scopes
            .last()
            .map(|scope| scope.contains_key(&name))
            .unwrap_or(false);
        if duplicate {
            self.error(format!("duplicate binding '{}'", name), span.clone());
        }
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(
                name,
                VarInfo {
                    ty,
                    used: false,
                    span,
                },
            );
        }
    }

    fn resolve_var(&mut self, name: &str, span: &Span) -> Type {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(info) = scope.get_mut(name) {
                info.used = true;
                return info.ty.clone();
            }
        }
        self.error(format!("unknown name '{}'", name), span.clone());
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
                self.error(format!("unused variable '{}'", name), span);
            }
        }
    }

    fn error(&mut self, message: impl Into<String>, span: Span) {
        self.diags.push(Diagnostic::new(message, span));
    }
}

trait ExprSpan {
    fn span(&self) -> Span;
}

impl ExprSpan for Expr {
    fn span(&self) -> Span {
        match self {
            Expr::IntLiteral { span, .. } => span.clone(),
            Expr::BoolLiteral { span, .. } => span.clone(),
            Expr::StringLiteral { span, .. } => span.clone(),
            Expr::Ident { span, .. } => span.clone(),
            Expr::Binary { span, .. } => span.clone(),
        }
    }
}
