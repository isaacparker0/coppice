#[derive(Clone, Debug)]
pub struct ExecutableProgram {
    pub struct_declarations: Vec<ExecutableStructDeclaration>,
    pub function_declarations: Vec<ExecutableFunctionDeclaration>,
}

#[derive(Clone, Debug)]
pub struct ExecutableFunctionDeclaration {
    pub name: String,
    pub parameters: Vec<ExecutableParameterDeclaration>,
    pub return_type: ExecutableTypeReference,
    pub statements: Vec<ExecutableStatement>,
}

#[derive(Clone, Debug)]
pub struct ExecutableParameterDeclaration {
    pub name: String,
    pub type_reference: ExecutableTypeReference,
}

#[derive(Clone, Debug)]
pub struct ExecutableStructDeclaration {
    pub name: String,
    pub fields: Vec<ExecutableStructFieldDeclaration>,
    pub methods: Vec<ExecutableMethodDeclaration>,
}

#[derive(Clone, Debug)]
pub struct ExecutableStructFieldDeclaration {
    pub name: String,
    pub type_reference: ExecutableTypeReference,
}

#[derive(Clone, Debug)]
pub struct ExecutableMethodDeclaration {
    pub name: String,
    pub self_mutable: bool,
    pub parameters: Vec<ExecutableParameterDeclaration>,
    pub return_type: ExecutableTypeReference,
    pub statements: Vec<ExecutableStatement>,
}

#[derive(Clone, Debug)]
pub enum ExecutableTypeReference {
    Int64,
    Boolean,
    String,
    Nil,
    Never,
    Named { name: String },
}

#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
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
        type_name: String,
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
        arguments: Vec<ExecutableExpression>,
    },
}

#[derive(Clone, Copy, Debug)]
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

#[derive(Clone, Copy, Debug)]
pub enum ExecutableUnaryOperator {
    Not,
    Negate,
}

#[derive(Clone, Debug)]
pub struct ExecutableStructLiteralField {
    pub name: String,
    pub value: ExecutableExpression,
}
