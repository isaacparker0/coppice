use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutableProgram {
    pub statements: Vec<ExecutableStatement>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutableStatement {
    Expression { expression: ExecutableExpression },
    Return { value: ExecutableExpression },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutableExpression {
    NilLiteral,
    StringLiteral {
        value: String,
    },
    Identifier {
        name: String,
    },
    Call {
        callee: Box<ExecutableExpression>,
        arguments: Vec<ExecutableExpression>,
    },
}
