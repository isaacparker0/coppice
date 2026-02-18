use std::collections::HashMap;

use compiler__source::Span;

#[derive(Clone, Default)]
pub struct TypeAnnotatedFile {
    pub function_signature_by_name: HashMap<String, TypeAnnotatedFunctionSignature>,
    pub struct_declarations: Vec<TypeAnnotatedStructDeclaration>,
    pub function_declarations: Vec<TypeAnnotatedFunctionDeclaration>,
}

#[derive(Clone)]
pub struct TypeAnnotatedFunctionSignature {
    pub type_parameter_count: usize,
    pub parameter_count: usize,
    pub returns_nil: bool,
}

#[derive(Clone)]
pub struct TypeAnnotatedFunctionDeclaration {
    pub name: String,
    pub parameters: Vec<TypeAnnotatedParameterDeclaration>,
    pub return_type: TypeAnnotatedTypeName,
    pub span: Span,
    pub statements: Vec<TypeAnnotatedStatement>,
}

#[derive(Clone)]
pub struct TypeAnnotatedParameterDeclaration {
    pub name: String,
    pub type_name: TypeAnnotatedTypeName,
    pub span: Span,
}

#[derive(Clone)]
pub struct TypeAnnotatedStructDeclaration {
    pub name: String,
    pub fields: Vec<TypeAnnotatedStructFieldDeclaration>,
    pub span: Span,
}

#[derive(Clone)]
pub struct TypeAnnotatedStructFieldDeclaration {
    pub name: String,
    pub type_name: TypeAnnotatedTypeName,
    pub span: Span,
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
    If {
        condition: TypeAnnotatedExpression,
        then_statements: Vec<TypeAnnotatedStatement>,
        else_statements: Option<Vec<TypeAnnotatedStatement>>,
        span: Span,
    },
    For {
        condition: Option<TypeAnnotatedExpression>,
        body_statements: Vec<TypeAnnotatedStatement>,
        span: Span,
    },
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },
    Abort {
        message: TypeAnnotatedExpression,
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
    BooleanLiteral {
        value: bool,
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
    StructLiteral {
        type_name: TypeAnnotatedTypeName,
        fields: Vec<TypeAnnotatedStructLiteralField>,
        span: Span,
    },
    FieldAccess {
        target: Box<TypeAnnotatedExpression>,
        field: String,
        span: Span,
    },
    Unary {
        operator: TypeAnnotatedUnaryOperator,
        expression: Box<TypeAnnotatedExpression>,
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
    Subtract,
    Multiply,
    Divide,
    EqualEqual,
    NotEqual,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeAnnotatedUnaryOperator {
    Not,
    Negate,
}

#[derive(Clone)]
pub struct TypeAnnotatedTypeName {
    pub names: Vec<TypeAnnotatedTypeNameSegment>,
    pub span: Span,
}

#[derive(Clone)]
pub struct TypeAnnotatedTypeNameSegment {
    pub name: String,
    pub has_type_arguments: bool,
    pub span: Span,
}

#[derive(Clone)]
pub struct TypeAnnotatedStructLiteralField {
    pub name: String,
    pub value: TypeAnnotatedExpression,
    pub span: Span,
}
