use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutableProgram {
    pub entrypoint_callable_reference: ExecutableCallableReference,
    pub struct_declarations: Vec<ExecutableStructDeclaration>,
    pub function_declarations: Vec<ExecutableFunctionDeclaration>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutableFunctionDeclaration {
    pub name: String,
    pub callable_reference: ExecutableCallableReference,
    pub parameters: Vec<ExecutableParameterDeclaration>,
    pub return_type: ExecutableTypeReference,
    pub statements: Vec<ExecutableStatement>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ExecutableCallableReference {
    pub package_path: String,
    pub symbol_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ExecutableCallTarget {
    BuiltinFunction {
        function_name: String,
    },
    UserDefinedFunction {
        callable_reference: ExecutableCallableReference,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutableParameterDeclaration {
    pub name: String,
    pub type_reference: ExecutableTypeReference,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutableStructDeclaration {
    pub name: String,
    pub struct_reference: ExecutableStructReference,
    pub fields: Vec<ExecutableStructFieldDeclaration>,
    pub methods: Vec<ExecutableMethodDeclaration>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ExecutableStructReference {
    pub package_path: String,
    pub symbol_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutableStructFieldDeclaration {
    pub name: String,
    pub type_reference: ExecutableTypeReference,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutableMethodDeclaration {
    pub name: String,
    pub self_mutable: bool,
    pub parameters: Vec<ExecutableParameterDeclaration>,
    pub return_type: ExecutableTypeReference,
    pub statements: Vec<ExecutableStatement>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ExecutableTypeReference {
    Int64,
    Boolean,
    String,
    Nil,
    Never,
    Union {
        members: Vec<ExecutableTypeReference>,
    },
    Named {
        name: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ExecutableStatement {
    Binding {
        name: String,
        mutable: bool,
        initializer: ExecutableExpression,
    },
    Assign {
        name: String,
        value: ExecutableExpression,
    },
    If {
        condition: ExecutableExpression,
        then_statements: Vec<ExecutableStatement>,
        else_statements: Option<Vec<ExecutableStatement>>,
    },
    For {
        condition: Option<ExecutableExpression>,
        body_statements: Vec<ExecutableStatement>,
    },
    Break,
    Continue,
    Expression {
        expression: ExecutableExpression,
    },
    Return {
        value: ExecutableExpression,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ExecutableExpression {
    IntegerLiteral {
        value: i64,
    },
    BooleanLiteral {
        value: bool,
    },
    NilLiteral,
    StringLiteral {
        value: String,
    },
    Identifier {
        name: String,
    },
    StructLiteral {
        struct_reference: ExecutableStructReference,
        fields: Vec<ExecutableStructLiteralField>,
    },
    FieldAccess {
        target: Box<ExecutableExpression>,
        field: String,
    },
    Unary {
        operator: ExecutableUnaryOperator,
        expression: Box<ExecutableExpression>,
    },
    Binary {
        operator: ExecutableBinaryOperator,
        left: Box<ExecutableExpression>,
        right: Box<ExecutableExpression>,
    },
    Call {
        callee: Box<ExecutableExpression>,
        call_target: Option<ExecutableCallTarget>,
        arguments: Vec<ExecutableExpression>,
    },
    Match {
        target: Box<ExecutableExpression>,
        arms: Vec<ExecutableMatchArm>,
    },
    Matches {
        value: Box<ExecutableExpression>,
        type_reference: ExecutableTypeReference,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutableMatchArm {
    pub pattern: ExecutableMatchPattern,
    pub value: ExecutableExpression,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ExecutableMatchPattern {
    Type {
        type_reference: ExecutableTypeReference,
    },
    Binding {
        binding_name: String,
        type_reference: ExecutableTypeReference,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ExecutableBinaryOperator {
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ExecutableUnaryOperator {
    Not,
    Negate,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutableStructLiteralField {
    pub name: String,
    pub value: ExecutableExpression,
}
