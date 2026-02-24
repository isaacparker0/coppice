use std::collections::HashMap;

use compiler__source::Span;

#[derive(Clone, Default)]
pub struct TypeAnnotatedFile {
    pub function_signature_by_name: HashMap<String, TypeAnnotatedFunctionSignature>,
    pub constant_declarations: Vec<TypeAnnotatedConstantDeclaration>,
    pub interface_declarations: Vec<TypeAnnotatedInterfaceDeclaration>,
    pub struct_declarations: Vec<TypeAnnotatedStructDeclaration>,
    pub function_declarations: Vec<TypeAnnotatedFunctionDeclaration>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TypeAnnotatedCallableReference {
    pub package_path: String,
    pub symbol_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TypeAnnotatedStructReference {
    pub package_path: String,
    pub symbol_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TypeAnnotatedInterfaceReference {
    pub package_path: String,
    pub symbol_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TypeAnnotatedNominalTypeReference {
    pub package_path: String,
    pub symbol_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TypeAnnotatedConstantReference {
    pub package_path: String,
    pub symbol_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TypeAnnotatedEnumVariantReference {
    pub enum_name: String,
    pub variant_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeAnnotatedCallTarget {
    BuiltinFunction {
        function_name: String,
    },
    UserDefinedFunction {
        callable_reference: TypeAnnotatedCallableReference,
    },
}

#[derive(Clone)]
pub struct TypeAnnotatedFunctionSignature {
    pub type_parameter_count: usize,
    pub parameter_count: usize,
    pub returns_nil: bool,
}

#[derive(Clone)]
pub struct TypeAnnotatedConstantDeclaration {
    pub name: String,
    pub constant_reference: TypeAnnotatedConstantReference,
    pub type_name: TypeAnnotatedTypeName,
    pub initializer: TypeAnnotatedExpression,
    pub span: Span,
}

#[derive(Clone)]
pub struct TypeAnnotatedFunctionDeclaration {
    pub name: String,
    pub callable_reference: TypeAnnotatedCallableReference,
    pub type_parameters: Vec<TypeAnnotatedTypeParameter>,
    pub parameters: Vec<TypeAnnotatedParameterDeclaration>,
    pub return_type: TypeAnnotatedTypeName,
    pub span: Span,
    pub statements: Vec<TypeAnnotatedStatement>,
}

#[derive(Clone)]
pub struct TypeAnnotatedTypeParameter {
    pub name: String,
    pub constraint: Option<TypeAnnotatedTypeName>,
    pub span: Span,
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
    pub struct_reference: TypeAnnotatedStructReference,
    pub type_parameters: Vec<TypeAnnotatedTypeParameter>,
    pub implemented_interfaces: Vec<TypeAnnotatedInterfaceReference>,
    pub fields: Vec<TypeAnnotatedStructFieldDeclaration>,
    pub methods: Vec<TypeAnnotatedMethodDeclaration>,
    pub span: Span,
}

#[derive(Clone)]
pub struct TypeAnnotatedInterfaceDeclaration {
    pub name: String,
    pub interface_reference: TypeAnnotatedInterfaceReference,
    pub methods: Vec<TypeAnnotatedInterfaceMethodDeclaration>,
    pub span: Span,
}

#[derive(Clone)]
pub struct TypeAnnotatedInterfaceMethodDeclaration {
    pub name: String,
    pub self_mutable: bool,
    pub parameters: Vec<TypeAnnotatedParameterDeclaration>,
    pub return_type: TypeAnnotatedTypeName,
    pub span: Span,
}

#[derive(Clone)]
pub struct TypeAnnotatedStructFieldDeclaration {
    pub name: String,
    pub type_name: TypeAnnotatedTypeName,
    pub span: Span,
}

#[derive(Clone)]
pub struct TypeAnnotatedMethodDeclaration {
    pub name: String,
    pub self_mutable: bool,
    pub parameters: Vec<TypeAnnotatedParameterDeclaration>,
    pub return_type: TypeAnnotatedTypeName,
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
    Expression {
        value: TypeAnnotatedExpression,
        span: Span,
    },
    Return {
        value: TypeAnnotatedExpression,
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
    NameReference {
        name: String,
        kind: TypeAnnotatedNameReferenceKind,
        constant_reference: Option<TypeAnnotatedConstantReference>,
        callable_reference: Option<TypeAnnotatedCallableReference>,
        span: Span,
    },
    EnumVariantLiteral {
        enum_variant_reference: TypeAnnotatedEnumVariantReference,
        span: Span,
    },
    StructLiteral {
        type_name: TypeAnnotatedTypeName,
        struct_reference: Option<TypeAnnotatedStructReference>,
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
        call_target: Option<TypeAnnotatedCallTarget>,
        arguments: Vec<TypeAnnotatedExpression>,
        type_arguments: Vec<TypeAnnotatedTypeName>,
        resolved_type_arguments: Vec<TypeAnnotatedResolvedTypeArgument>,
        span: Span,
    },
    Match {
        target: Box<TypeAnnotatedExpression>,
        arms: Vec<TypeAnnotatedMatchArm>,
        span: Span,
    },
    Matches {
        value: Box<TypeAnnotatedExpression>,
        type_name: TypeAnnotatedTypeName,
        span: Span,
    },
}

#[derive(Clone)]
pub struct TypeAnnotatedMatchArm {
    pub pattern: TypeAnnotatedMatchPattern,
    pub value: TypeAnnotatedExpression,
    pub span: Span,
}

#[derive(Clone)]
pub enum TypeAnnotatedMatchPattern {
    Type {
        type_name: TypeAnnotatedTypeName,
        span: Span,
    },
    Binding {
        name: String,
        type_name: TypeAnnotatedTypeName,
        span: Span,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeAnnotatedBinaryOperator {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeAnnotatedUnaryOperator {
    Not,
    Negate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeAnnotatedNameReferenceKind {
    UserDefined,
    Builtin,
}

#[derive(Clone)]
pub struct TypeAnnotatedTypeName {
    pub names: Vec<TypeAnnotatedTypeNameSegment>,
    pub span: Span,
}

#[derive(Clone)]
pub struct TypeAnnotatedTypeNameSegment {
    pub name: String,
    pub nominal_type_reference: Option<TypeAnnotatedNominalTypeReference>,
    pub type_arguments: Vec<TypeAnnotatedTypeName>,
    pub span: Span,
}

#[derive(Clone)]
pub enum TypeAnnotatedResolvedTypeArgument {
    Int64,
    Boolean,
    String,
    Nil,
    Never,
    Function {
        parameter_types: Vec<TypeAnnotatedResolvedTypeArgument>,
        return_type: Box<TypeAnnotatedResolvedTypeArgument>,
    },
    Union {
        members: Vec<TypeAnnotatedResolvedTypeArgument>,
    },
    TypeParameter {
        name: String,
    },
    NominalTypeApplication {
        base_nominal_type_reference: Option<TypeAnnotatedNominalTypeReference>,
        base_name: String,
        arguments: Vec<TypeAnnotatedResolvedTypeArgument>,
    },
    NominalType {
        nominal_type_reference: Option<TypeAnnotatedNominalTypeReference>,
        name: String,
    },
}

#[derive(Clone)]
pub struct TypeAnnotatedStructLiteralField {
    pub name: String,
    pub value: TypeAnnotatedExpression,
    pub span: Span,
}
