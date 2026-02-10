use crate::diagnostics::Span;

#[derive(Clone, Debug)]
pub struct File {
    pub type_declarations: Vec<TypeDeclaration>,
    pub constant_declarations: Vec<ConstantDeclaration>,
    pub function_declarations: Vec<FunctionDeclaration>,
}

#[derive(Clone, Debug)]
pub struct TypeDeclaration {
    pub name: String,
    pub fields: Vec<StructField>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct StructField {
    pub name: String,
    pub type_name: TypeName,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ConstantDeclaration {
    pub name: String,
    pub expression: Expression,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct FunctionDeclaration {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub return_type: TypeName,
    pub body: Block,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Parameter {
    pub name: String,
    pub type_name: TypeName,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub statements: Vec<Statement>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Statement {
    Let {
        name: String,
        mutable: bool,
        type_name: Option<TypeName>,
        expression: Expression,
        span: Span,
    },
    Assign {
        name: String,
        name_span: Span,
        expression: Expression,
        span: Span,
    },
    Return {
        expression: Expression,
        span: Span,
    },
    If {
        condition: Expression,
        then_block: Block,
        else_block: Option<Block>,
        span: Span,
    },
}

#[derive(Clone, Debug)]
pub enum Expression {
    IntegerLiteral {
        value: i64,
        span: Span,
    },
    BooleanLiteral {
        value: bool,
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
    Call {
        callee: Box<Expression>,
        arguments: Vec<Expression>,
        span: Span,
    },
    Binary {
        operator: BinaryOperator,
        left: Box<Expression>,
        right: Box<Expression>,
        span: Span,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    EqualEqual,
}

#[derive(Clone, Debug)]
pub struct TypeName {
    pub name: String,
    pub span: Span,
}
