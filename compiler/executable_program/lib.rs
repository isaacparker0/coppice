use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutableProgram {
    pub entrypoint_callable_reference: ExecutableCallableReference,
    pub constant_declarations: Vec<ExecutableConstantDeclaration>,
    pub interface_declarations: Vec<ExecutableInterfaceDeclaration>,
    pub struct_declarations: Vec<ExecutableStructDeclaration>,
    pub function_declarations: Vec<ExecutableFunctionDeclaration>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutableFunctionDeclaration {
    pub name: String,
    pub callable_reference: ExecutableCallableReference,
    pub type_parameter_names: Vec<String>,
    pub type_parameter_constraint_interface_reference_by_name:
        BTreeMap<String, ExecutableInterfaceReference>,
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
    BuiltinListGet,
    BuiltinListSet,
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
    pub type_parameter_names: Vec<String>,
    pub implemented_interfaces: Vec<ExecutableInterfaceReference>,
    pub fields: Vec<ExecutableStructFieldDeclaration>,
    pub methods: Vec<ExecutableMethodDeclaration>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ExecutableStructReference {
    pub package_path: String,
    pub symbol_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ExecutableInterfaceReference {
    pub package_path: String,
    pub symbol_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutableNominalTypeReference {
    pub package_path: String,
    pub symbol_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ExecutableConstantReference {
    pub package_path: String,
    pub symbol_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ExecutableEnumVariantReference {
    pub enum_name: String,
    pub variant_name: String,
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
pub struct ExecutableInterfaceDeclaration {
    pub name: String,
    pub interface_reference: ExecutableInterfaceReference,
    pub methods: Vec<ExecutableInterfaceMethodDeclaration>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutableInterfaceMethodDeclaration {
    pub name: String,
    pub self_mutable: bool,
    pub parameters: Vec<ExecutableParameterDeclaration>,
    pub return_type: ExecutableTypeReference,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutableConstantDeclaration {
    pub name: String,
    pub constant_reference: ExecutableConstantReference,
    pub type_reference: ExecutableTypeReference,
    pub initializer: ExecutableExpression,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutableTypeReference {
    Int64,
    Boolean,
    String,
    Nil,
    Never,
    List {
        element_type: Box<ExecutableTypeReference>,
    },
    Function {
        parameter_types: Vec<ExecutableTypeReference>,
        return_type: Box<ExecutableTypeReference>,
    },
    Union {
        members: Vec<ExecutableTypeReference>,
    },
    TypeParameter {
        name: String,
    },
    NominalTypeApplication {
        base_nominal_type_reference: Option<ExecutableNominalTypeReference>,
        base_name: String,
        arguments: Vec<ExecutableTypeReference>,
    },
    NominalType {
        nominal_type_reference: Option<ExecutableNominalTypeReference>,
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
    ListLiteral {
        elements: Vec<ExecutableExpression>,
        element_type: ExecutableTypeReference,
    },
    Identifier {
        name: String,
        constant_reference: Option<ExecutableConstantReference>,
        callable_reference: Option<ExecutableCallableReference>,
    },
    EnumVariantLiteral {
        enum_variant_reference: ExecutableEnumVariantReference,
        type_reference: ExecutableTypeReference,
    },
    StructLiteral {
        struct_reference: ExecutableStructReference,
        type_reference: ExecutableTypeReference,
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
        type_arguments: Vec<ExecutableTypeReference>,
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
    Modulo,
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
