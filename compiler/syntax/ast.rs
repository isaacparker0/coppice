use compiler__source::{FileRole, Span};

#[derive(Clone, Debug)]
pub struct SyntaxImportDeclaration {
    pub package_path: String,
    pub members: Vec<SyntaxImportMember>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SyntaxImportMember {
    pub name: String,
    pub alias: Option<String>,
    pub alias_span: Option<Span>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SyntaxExportsDeclaration {
    pub members: Vec<SyntaxExportsMember>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SyntaxExportsMember {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SyntaxParsedFile {
    pub role: FileRole,
    pub items: Vec<SyntaxFileItem>,
}

#[derive(Clone, Debug)]
pub enum SyntaxFileItem {
    DocComment(SyntaxDocComment),
    Declaration(Box<SyntaxDeclaration>),
}

impl SyntaxParsedFile {
    pub fn top_level_declarations(&self) -> impl Iterator<Item = &SyntaxDeclaration> {
        self.items.iter().filter_map(|item| match item {
            SyntaxFileItem::DocComment(_) => None,
            SyntaxFileItem::Declaration(declaration) => Some(declaration.as_ref()),
        })
    }
}

#[derive(Clone, Debug)]
pub enum SyntaxDeclaration {
    Import(SyntaxImportDeclaration),
    Exports(SyntaxExportsDeclaration),
    Type(SyntaxTypeDeclaration),
    Constant(SyntaxConstantDeclaration),
    Function(SyntaxFunctionDeclaration),
    Group(SyntaxTestGroupDeclaration),
    Test(SyntaxTestDeclaration),
}

#[derive(Clone, Debug)]
pub struct SyntaxTestGroupDeclaration {
    pub name: String,
    pub name_span: Span,
    pub tests: Vec<SyntaxTestDeclaration>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SyntaxTestDeclaration {
    pub name: String,
    pub name_span: Span,
    pub body: SyntaxBlock,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SyntaxDocComment {
    pub lines: Vec<String>,
    pub span: Span,
    pub end_line: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxTopLevelVisibility {
    Private,
    Visible,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxMemberVisibility {
    Private,
    Public,
}

#[derive(Clone, Debug)]
pub struct SyntaxTypeDeclaration {
    pub name: String,
    pub name_span: Span,
    pub type_parameters: Vec<SyntaxTypeParameter>,
    pub implemented_interfaces: Vec<SyntaxTypeName>,
    pub kind: SyntaxTypeDeclarationKind,
    pub visibility: SyntaxTopLevelVisibility,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum SyntaxTypeDeclarationKind {
    Struct {
        items: Vec<SyntaxStructMemberItem>,
    },
    Enum {
        variants: Vec<SyntaxEnumVariant>,
    },
    Interface {
        methods: Vec<SyntaxInterfaceMethodDeclaration>,
    },
    Union {
        variants: Vec<SyntaxTypeName>,
    },
}

#[derive(Clone, Debug)]
pub enum SyntaxStructMemberItem {
    DocComment(SyntaxDocComment),
    Field(Box<SyntaxFieldDeclaration>),
    Method(Box<SyntaxMethodDeclaration>),
}

#[derive(Clone, Debug)]
pub struct SyntaxEnumVariant {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SyntaxFieldDeclaration {
    pub name: String,
    pub type_name: SyntaxTypeName,
    pub visibility: SyntaxMemberVisibility,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SyntaxMethodDeclaration {
    pub name: String,
    pub name_span: Span,
    pub self_span: Span,
    pub self_mutable: bool,
    pub parameters: Vec<SyntaxParameterDeclaration>,
    pub return_type: SyntaxTypeName,
    pub body: SyntaxBlock,
    pub visibility: SyntaxMemberVisibility,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SyntaxInterfaceMethodDeclaration {
    pub name: String,
    pub name_span: Span,
    pub self_span: Span,
    pub self_mutable: bool,
    pub parameters: Vec<SyntaxParameterDeclaration>,
    pub return_type: SyntaxTypeName,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SyntaxConstantDeclaration {
    pub name: String,
    pub name_span: Span,
    pub type_name: SyntaxTypeName,
    pub expression: SyntaxExpression,
    pub visibility: SyntaxTopLevelVisibility,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SyntaxFunctionDeclaration {
    pub name: String,
    pub name_span: Span,
    pub type_parameters: Vec<SyntaxTypeParameter>,
    pub parameters: Vec<SyntaxParameterDeclaration>,
    pub return_type: SyntaxTypeName,
    pub body: SyntaxBlock,
    pub visibility: SyntaxTopLevelVisibility,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SyntaxParameterDeclaration {
    pub name: String,
    pub name_span: Span,
    pub mutable: bool,
    pub type_name: SyntaxTypeName,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SyntaxBlock {
    pub items: Vec<SyntaxBlockItem>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum SyntaxBlockItem {
    DocComment(SyntaxDocComment),
    Statement(SyntaxStatement),
}

#[derive(Clone, Debug)]
pub enum SyntaxStatement {
    Binding {
        name: String,
        name_span: Span,
        mutable: bool,
        type_name: Option<SyntaxTypeName>,
        initializer: SyntaxExpression,
        span: Span,
    },
    Assign {
        target: SyntaxAssignTarget,
        value: SyntaxExpression,
        span: Span,
    },
    Return {
        value: Option<SyntaxExpression>,
        span: Span,
    },
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },
    If {
        condition: SyntaxExpression,
        then_block: SyntaxBlock,
        else_block: Option<SyntaxBlock>,
        span: Span,
    },
    For {
        condition: Option<SyntaxExpression>,
        body: SyntaxBlock,
        span: Span,
    },
    Expression {
        value: SyntaxExpression,
        span: Span,
    },
}

#[derive(Clone, Debug)]
pub enum SyntaxAssignTarget {
    Name {
        name: String,
        name_span: Span,
        span: Span,
    },
    Index {
        target: Box<SyntaxExpression>,
        index: Box<SyntaxExpression>,
        span: Span,
    },
}

#[derive(Clone, Debug)]
pub enum SyntaxStringInterpolationPart {
    Literal(String),
    Expression(SyntaxExpression),
}

#[derive(Clone, Debug)]
pub enum SyntaxExpression {
    IntegerLiteral {
        value: i64,
        span: Span,
    },
    NilLiteral {
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
    ListLiteral {
        elements: Vec<SyntaxExpression>,
        span: Span,
    },
    NameReference {
        name: String,
        kind: SyntaxNameReferenceKind,
        span: Span,
    },
    StructLiteral {
        type_name: SyntaxTypeName,
        fields: Vec<SyntaxStructLiteralField>,
        span: Span,
    },
    FieldAccess {
        target: Box<SyntaxExpression>,
        field: String,
        field_span: Span,
        span: Span,
    },
    IndexAccess {
        target: Box<SyntaxExpression>,
        index: Box<SyntaxExpression>,
        span: Span,
    },
    Call {
        callee: Box<SyntaxExpression>,
        type_arguments: Vec<SyntaxTypeName>,
        arguments: Vec<SyntaxExpression>,
        span: Span,
    },
    Unary {
        operator: SyntaxUnaryOperator,
        expression: Box<SyntaxExpression>,
        span: Span,
    },
    Binary {
        operator: SyntaxBinaryOperator,
        left: Box<SyntaxExpression>,
        right: Box<SyntaxExpression>,
        span: Span,
    },
    Match {
        target: Box<SyntaxExpression>,
        arms: Vec<SyntaxMatchArm>,
        span: Span,
    },
    Matches {
        value: Box<SyntaxExpression>,
        type_name: SyntaxTypeName,
        span: Span,
    },
    StringInterpolation {
        parts: Vec<SyntaxStringInterpolationPart>,
        span: Span,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxNameReferenceKind {
    UserDefined,
    Builtin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxBinaryOperator {
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
pub enum SyntaxUnaryOperator {
    Not,
    Negate,
}

#[derive(Clone, Debug)]
pub struct SyntaxTypeName {
    pub names: Vec<SyntaxTypeNameSegment>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SyntaxTypeNameSegment {
    pub name: String,
    pub type_arguments: Vec<SyntaxTypeName>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SyntaxTypeParameter {
    pub name: String,
    pub constraint: Option<SyntaxTypeName>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SyntaxStructLiteralField {
    pub name: String,
    pub name_span: Span,
    pub value: SyntaxExpression,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SyntaxMatchArm {
    pub pattern: SyntaxMatchPattern,
    pub value: SyntaxExpression,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum SyntaxMatchPattern {
    Type {
        type_name: SyntaxTypeName,
        span: Span,
    },
    Binding {
        name: String,
        name_span: Span,
        type_name: SyntaxTypeName,
        span: Span,
    },
}

impl SyntaxMatchPattern {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            SyntaxMatchPattern::Type { span, .. } | SyntaxMatchPattern::Binding { span, .. } => {
                span.clone()
            }
        }
    }
}
