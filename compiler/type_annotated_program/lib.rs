use std::collections::HashMap;

use compiler__source::Span;

#[derive(Clone, Default)]
pub struct TypeAnnotatedFile {
    pub function_signature_by_name: HashMap<String, TypeAnnotatedFunctionSignature>,
    pub main_function: Option<TypeAnnotatedFunction>,
}

#[derive(Clone)]
pub struct TypeAnnotatedFunctionSignature {
    pub type_parameter_count: usize,
    pub parameter_count: usize,
    pub returns_nil: bool,
}

#[derive(Clone)]
pub struct TypeAnnotatedFunction {
    pub name: String,
    pub span: Span,
    pub statements: Vec<TypeAnnotatedStatement>,
}

#[derive(Clone)]
pub enum TypeAnnotatedStatement {
    Binding {
        name: String,
        mutable: bool,
        initializer: TypeAnnotatedExpression,
        span: Span,
    },
    Assign {
        name: String,
        value: TypeAnnotatedExpression,
        span: Span,
    },
    Expression {
        value: TypeAnnotatedExpression,
        span: Span,
    },
    Return {
        value: TypeAnnotatedExpression,
        span: Span,
    },
    Unsupported {
        span: Span,
    },
}

#[derive(Clone)]
pub enum TypeAnnotatedExpression {
    IntegerLiteral {
        value: i64,
        span: Span,
    },
    NilLiteral {
        span: Span,
    },
    StringLiteral {
        value: String,
        span: Span,
    },
    Identifier {
        name: String,
        span: Span,
    },
    Binary {
        operator: TypeAnnotatedBinaryOperator,
        left: Box<TypeAnnotatedExpression>,
        right: Box<TypeAnnotatedExpression>,
        span: Span,
    },
    Call {
        callee: Box<TypeAnnotatedExpression>,
        arguments: Vec<TypeAnnotatedExpression>,
        has_type_arguments: bool,
        span: Span,
    },
    Unsupported {
        span: Span,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeAnnotatedBinaryOperator {
    Add,
}
