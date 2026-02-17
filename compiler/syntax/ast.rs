use compiler__source::{FileRole, Span};

#[derive(Clone, Debug)]
pub struct ImportDeclaration {
    pub package_path: String,
    pub members: Vec<ImportMember>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ImportMember {
    pub name: String,
    pub alias: Option<String>,
    pub alias_span: Option<Span>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ExportsDeclaration {
    pub members: Vec<ExportsMember>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ExportsMember {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ParsedFile {
    pub role: FileRole,
    pub items: Vec<FileItem>,
}

#[derive(Clone, Debug)]
pub enum FileItem {
    DocComment(DocComment),
    Declaration(Box<Declaration>),
}

impl ParsedFile {
    pub fn top_level_declarations(&self) -> impl Iterator<Item = &Declaration> {
        self.items.iter().filter_map(|item| match item {
            FileItem::DocComment(_) => None,
            FileItem::Declaration(declaration) => Some(declaration.as_ref()),
        })
    }
}

#[derive(Clone, Debug)]
pub enum Declaration {
    Import(ImportDeclaration),
    Exports(ExportsDeclaration),
    Type(TypeDeclaration),
    Constant(ConstantDeclaration),
    Function(FunctionDeclaration),
}

#[derive(Clone, Debug)]
pub struct DocComment {
    pub lines: Vec<String>,
    pub span: Span,
    pub end_line: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    Private,
    Public,
}

#[derive(Clone, Debug)]
pub struct TypeDeclaration {
    pub name: String,
    pub type_parameters: Vec<TypeParameter>,
    pub kind: TypeDeclarationKind,
    pub visibility: Visibility,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum TypeDeclarationKind {
    Struct { items: Vec<StructMemberItem> },
    Enum { variants: Vec<EnumVariant> },
    Union { variants: Vec<TypeName> },
}

#[derive(Clone, Debug)]
pub enum StructMemberItem {
    DocComment(DocComment),
    Field(Box<FieldDeclaration>),
    Method(Box<MethodDeclaration>),
}

#[derive(Clone, Debug)]
pub struct EnumVariant {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct FieldDeclaration {
    pub name: String,
    pub type_name: TypeName,
    pub visibility: Visibility,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct MethodDeclaration {
    pub name: String,
    pub name_span: Span,
    pub self_span: Span,
    pub self_mutable: bool,
    pub parameters: Vec<ParameterDeclaration>,
    pub return_type: TypeName,
    pub body: Block,
    pub visibility: Visibility,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ConstantDeclaration {
    pub name: String,
    pub type_name: TypeName,
    pub expression: Expression,
    pub visibility: Visibility,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct FunctionDeclaration {
    pub name: String,
    pub name_span: Span,
    pub type_parameters: Vec<TypeParameter>,
    pub parameters: Vec<ParameterDeclaration>,
    pub return_type: TypeName,
    pub body: Block,
    pub visibility: Visibility,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ParameterDeclaration {
    pub name: String,
    pub type_name: TypeName,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub items: Vec<BlockItem>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum BlockItem {
    DocComment(DocComment),
    Statement(Statement),
}

#[derive(Clone, Debug)]
pub enum Statement {
    Binding {
        name: String,
        mutable: bool,
        type_name: Option<TypeName>,
        initializer: Expression,
        span: Span,
    },
    Assign {
        name: String,
        name_span: Span,
        value: Expression,
        span: Span,
    },
    Return {
        value: Expression,
        span: Span,
    },
    Abort {
        message: Expression,
        span: Span,
    },
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },
    If {
        condition: Expression,
        then_block: Block,
        else_block: Option<Block>,
        span: Span,
    },
    For {
        condition: Option<Expression>,
        body: Block,
        span: Span,
    },
    Expression {
        value: Expression,
        span: Span,
    },
}

#[derive(Clone, Debug)]
pub enum Expression {
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
    Identifier {
        name: String,
        span: Span,
    },
    StructLiteral {
        type_name: TypeName,
        fields: Vec<StructLiteralField>,
        span: Span,
    },
    FieldAccess {
        target: Box<Expression>,
        field: String,
        field_span: Span,
        span: Span,
    },
    Call {
        callee: Box<Expression>,
        type_arguments: Vec<TypeName>,
        arguments: Vec<Expression>,
        span: Span,
    },
    Unary {
        operator: UnaryOperator,
        expression: Box<Expression>,
        span: Span,
    },
    Binary {
        operator: BinaryOperator,
        left: Box<Expression>,
        right: Box<Expression>,
        span: Span,
    },
    Match {
        target: Box<Expression>,
        arms: Vec<MatchArm>,
        span: Span,
    },
    Matches {
        value: Box<Expression>,
        type_name: TypeName,
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
    NotEqual,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOperator {
    Not,
    Negate,
}

#[derive(Clone, Debug)]
pub struct TypeName {
    pub names: Vec<TypeNameAtom>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct TypeNameAtom {
    pub name: String,
    pub type_arguments: Vec<TypeName>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct TypeParameter {
    pub name: String,
    pub constraint: Option<TypeName>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct StructLiteralField {
    pub name: String,
    pub name_span: Span,
    pub value: Expression,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct MatchArm {
    pub pattern: MatchPattern,
    pub value: Expression,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum MatchPattern {
    Type {
        type_name: TypeName,
        span: Span,
    },
    Binding {
        name: String,
        name_span: Span,
        type_name: TypeName,
        span: Span,
    },
}

impl MatchPattern {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            MatchPattern::Type { span, .. } | MatchPattern::Binding { span, .. } => span.clone(),
        }
    }
}
