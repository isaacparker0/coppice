use compiler__source::{FileRole, Span};

#[derive(Clone)]
pub struct SemanticFile {
    pub role: FileRole,
    pub declarations: Vec<SemanticDeclaration>,
}

#[derive(Clone, Debug)]
pub enum SemanticDeclaration {
    Type(SemanticTypeDeclaration),
    Constant(SemanticConstantDeclaration),
    Function(SemanticFunctionDeclaration),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticTopLevelVisibility {
    Private,
    Visible,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticMemberVisibility {
    Private,
    Public,
}

#[derive(Clone, Debug)]
pub struct SemanticDocComment {
    pub lines: Vec<String>,
    pub span: Span,
    pub end_line: usize,
}

#[derive(Clone, Debug)]
pub struct SemanticTypeDeclaration {
    pub name: String,
    pub type_parameters: Vec<SemanticTypeParameter>,
    pub implemented_interfaces: Vec<SemanticTypeName>,
    pub kind: SemanticTypeDeclarationKind,
    pub doc: Option<SemanticDocComment>,
    pub visibility: SemanticTopLevelVisibility,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum SemanticTypeDeclarationKind {
    Struct {
        fields: Vec<SemanticFieldDeclaration>,
        methods: Vec<SemanticMethodDeclaration>,
    },
    Enum {
        variants: Vec<SemanticEnumVariant>,
    },
    Interface {
        methods: Vec<SemanticInterfaceMethodDeclaration>,
    },
    Union {
        variants: Vec<SemanticTypeName>,
    },
}

#[derive(Clone, Debug)]
pub struct SemanticEnumVariant {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SemanticFieldDeclaration {
    pub name: String,
    pub type_name: SemanticTypeName,
    pub doc: Option<SemanticDocComment>,
    pub visibility: SemanticMemberVisibility,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SemanticMethodDeclaration {
    pub name: String,
    pub name_span: Span,
    pub self_span: Span,
    pub self_mutable: bool,
    pub parameters: Vec<SemanticParameterDeclaration>,
    pub return_type: SemanticTypeName,
    pub body: SemanticBlock,
    pub doc: Option<SemanticDocComment>,
    pub visibility: SemanticMemberVisibility,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SemanticInterfaceMethodDeclaration {
    pub name: String,
    pub name_span: Span,
    pub self_span: Span,
    pub self_mutable: bool,
    pub parameters: Vec<SemanticParameterDeclaration>,
    pub return_type: SemanticTypeName,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SemanticConstantDeclaration {
    pub name: String,
    pub type_name: SemanticTypeName,
    pub expression: SemanticExpression,
    pub doc: Option<SemanticDocComment>,
    pub visibility: SemanticTopLevelVisibility,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SemanticFunctionDeclaration {
    pub name: String,
    pub name_span: Span,
    pub type_parameters: Vec<SemanticTypeParameter>,
    pub parameters: Vec<SemanticParameterDeclaration>,
    pub return_type: SemanticTypeName,
    pub body: SemanticBlock,
    pub doc: Option<SemanticDocComment>,
    pub visibility: SemanticTopLevelVisibility,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SemanticParameterDeclaration {
    pub name: String,
    pub mutable: bool,
    pub type_name: SemanticTypeName,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SemanticBlock {
    pub statements: Vec<SemanticStatement>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum SemanticStatement {
    Binding {
        name: String,
        mutable: bool,
        type_name: Option<SemanticTypeName>,
        initializer: SemanticExpression,
        span: Span,
    },
    Assign {
        target: SemanticAssignTarget,
        value: SemanticExpression,
        span: Span,
    },
    Return {
        value: SemanticExpression,
        span: Span,
    },
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },
    If {
        condition: SemanticExpression,
        then_block: SemanticBlock,
        else_block: Option<SemanticBlock>,
        span: Span,
    },
    For {
        condition: Option<SemanticExpression>,
        body: SemanticBlock,
        span: Span,
    },
    Expression {
        value: SemanticExpression,
        span: Span,
    },
}

#[derive(Clone, Debug)]
pub enum SemanticAssignTarget {
    Name {
        name: String,
        name_span: Span,
        span: Span,
    },
    Index {
        target: Box<SemanticExpression>,
        index: Box<SemanticExpression>,
        span: Span,
    },
}

#[derive(Clone, Debug)]
pub enum SemanticExpression {
    IntegerLiteral {
        id: SemanticExpressionId,
        value: i64,
        span: Span,
    },
    NilLiteral {
        id: SemanticExpressionId,
        span: Span,
    },
    BooleanLiteral {
        id: SemanticExpressionId,
        value: bool,
        span: Span,
    },
    StringLiteral {
        id: SemanticExpressionId,
        value: String,
        span: Span,
    },
    ListLiteral {
        id: SemanticExpressionId,
        elements: Vec<SemanticExpression>,
        span: Span,
    },
    NameReference {
        id: SemanticExpressionId,
        name: String,
        kind: SemanticNameReferenceKind,
        span: Span,
    },
    StructLiteral {
        id: SemanticExpressionId,
        type_name: SemanticTypeName,
        fields: Vec<SemanticStructLiteralField>,
        span: Span,
    },
    FieldAccess {
        id: SemanticExpressionId,
        target: Box<SemanticExpression>,
        field: String,
        field_span: Span,
        span: Span,
    },
    IndexAccess {
        id: SemanticExpressionId,
        target: Box<SemanticExpression>,
        index: Box<SemanticExpression>,
        span: Span,
    },
    Call {
        id: SemanticExpressionId,
        callee: Box<SemanticExpression>,
        type_arguments: Vec<SemanticTypeName>,
        arguments: Vec<SemanticExpression>,
        span: Span,
    },
    Unary {
        id: SemanticExpressionId,
        operator: SemanticUnaryOperator,
        expression: Box<SemanticExpression>,
        span: Span,
    },
    Binary {
        id: SemanticExpressionId,
        operator: SemanticBinaryOperator,
        left: Box<SemanticExpression>,
        right: Box<SemanticExpression>,
        span: Span,
    },
    Match {
        id: SemanticExpressionId,
        target: Box<SemanticExpression>,
        arms: Vec<SemanticMatchArm>,
        span: Span,
    },
    Matches {
        id: SemanticExpressionId,
        value: Box<SemanticExpression>,
        type_name: SemanticTypeName,
        span: Span,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SemanticExpressionId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticNameReferenceKind {
    UserDefined,
    Builtin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticBinaryOperator {
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
pub enum SemanticUnaryOperator {
    Not,
    Negate,
}

#[derive(Clone, Debug)]
pub struct SemanticTypeName {
    pub names: Vec<SemanticTypeNameSegment>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SemanticTypeNameSegment {
    pub name: String,
    pub type_arguments: Vec<SemanticTypeName>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SemanticTypeParameter {
    pub name: String,
    pub constraint: Option<SemanticTypeName>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SemanticStructLiteralField {
    pub name: String,
    pub name_span: Span,
    pub value: SemanticExpression,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SemanticMatchArm {
    pub pattern: SemanticMatchPattern,
    pub value: SemanticExpression,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum SemanticMatchPattern {
    Type {
        type_name: SemanticTypeName,
        span: Span,
    },
    Binding {
        name: String,
        name_span: Span,
        type_name: SemanticTypeName,
        span: Span,
    },
}

impl SemanticMatchPattern {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            SemanticMatchPattern::Type { span, .. }
            | SemanticMatchPattern::Binding { span, .. } => span.clone(),
        }
    }
}
