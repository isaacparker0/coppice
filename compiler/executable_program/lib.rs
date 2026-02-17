use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutableProgram {
    pub statements: Vec<ExecutableStatement>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
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
    Expression {
        expression: ExecutableExpression,
    },
    Return {
        value: ExecutableExpression,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutableExpression {
    IntegerLiteral {
        value: i64,
    },
    NilLiteral,
    StringLiteral {
        value: String,
    },
    Identifier {
        name: String,
    },
    Add {
        left: Box<ExecutableExpression>,
        right: Box<ExecutableExpression>,
    },
    Call {
        callee: Box<ExecutableExpression>,
        arguments: Vec<ExecutableExpression>,
    },
}
