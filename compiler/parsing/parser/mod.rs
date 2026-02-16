use crate::lexer::{Keyword, Symbol, Token, TokenKind};
use compiler__diagnostics::Diagnostic;
use compiler__source::FileRole;
use compiler__source::Span;
use compiler__syntax::{Declaration, DocComment, Expression, FileItem, ParsedFile, Visibility};

mod declarations;
mod exports;
mod expressions;
mod imports;
mod recovery;
mod statements;
mod types;

#[derive(Clone, Debug)]
pub(super) enum UnexpectedTokenKind {
    ExpectedIdentifier,
    ReservedKeywordAsIdentifier { keyword: Keyword },
    ExpectedExpression,
}

#[derive(Clone, Debug)]
pub(super) enum MissingTokenKind {
    Keyword { keyword: Keyword },
    Symbol,
}

#[derive(Clone, Debug)]
pub(super) enum InvalidConstructKind {
    DocCommentMustDocumentDeclaration,
    FirstMethodParameterMustBeSelf,
    ConstantsRequireExplicitTypeAnnotation,
    TypeArgumentsMustBeFollowedByCall,
}

#[derive(Clone, Debug)]
pub(super) enum RecoveredKind {
    ExpectedDeclarationAfterPublic,
    ExpectedTypeKeywordBeforeTypeDeclaration,
    ExpectedDeclaration,
    MethodReceiverSelfMustNotHaveTypeAnnotation,
    TypeParameterListMustNotBeEmpty,
    EnumDeclarationMustIncludeAtLeastOneVariant,
    ExpectedCommaOrRightBraceAfterEnumVariant,
    UnexpectedEqualsInExpression,
}

#[derive(Clone, Debug)]
pub(super) enum ParseError {
    UnexpectedToken {
        kind: UnexpectedTokenKind,
        span: Span,
    },
    MissingToken {
        kind: MissingTokenKind,
        span: Span,
    },
    InvalidConstruct {
        kind: InvalidConstructKind,
        span: Span,
    },
    Recovered {
        kind: RecoveredKind,
        span: Span,
    },
    UnparsableToken,
}

pub(super) type ParseResult<T> = Result<T, ParseError>;

pub struct Parser {
    tokens: Vec<Token>,
    position: usize,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            position: 0,
            diagnostics: Vec::new(),
        }
    }

    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }

    pub fn parse_file_tokens(&mut self, role: FileRole) -> ParsedFile {
        let items = self.parse_declarations();

        ParsedFile { role, items }
    }

    fn parse_declarations(&mut self) -> Vec<FileItem> {
        let mut items = Vec::new();
        while !self.at_eof() {
            self.skip_statement_terminators();
            if let Some(doc_comment) = self.parse_leading_doc_comment_block() {
                items.push(FileItem::DocComment(doc_comment));
            }
            if self.at_eof() {
                break;
            }

            let parse_result: Option<ParseResult<Declaration>> = if self
                .peek_is_keyword(Keyword::Public)
            {
                let visibility = self.parse_visibility();
                if self.peek_is_keyword(Keyword::Type) {
                    Some(
                        self.parse_type_declaration(visibility)
                            .map(Declaration::Type),
                    )
                } else if self.peek_is_keyword(Keyword::Function) {
                    Some(self.parse_function(visibility).map(Declaration::Function))
                } else if self.peek_is_identifier() {
                    Some(
                        self.parse_constant_declaration(visibility)
                            .map(Declaration::Constant),
                    )
                } else {
                    let span = self.peek_span();
                    self.report_parse_error(&ParseError::Recovered {
                        kind: RecoveredKind::ExpectedDeclarationAfterPublic,
                        span,
                    });
                    self.synchronize();
                    None
                }
            } else if self.peek_is_keyword(Keyword::Type) {
                Some(
                    self.parse_type_declaration(Visibility::Private)
                        .map(Declaration::Type),
                )
            } else if self.peek_is_keyword(Keyword::Import) {
                Some(self.parse_import_declaration().map(Declaration::Import))
            } else if self.peek_is_keyword(Keyword::Exports) {
                Some(self.parse_exports_declaration().map(Declaration::Exports))
            } else if self.peek_is_keyword(Keyword::Function) {
                Some(
                    self.parse_function(Visibility::Private)
                        .map(Declaration::Function),
                )
            } else if self.peek_is_identifier() && self.peek_second_is_symbol(Symbol::DoubleColon) {
                let span = self.peek_span();
                self.report_parse_error(&ParseError::Recovered {
                    kind: RecoveredKind::ExpectedTypeKeywordBeforeTypeDeclaration,
                    span,
                });
                self.advance();
                self.synchronize();
                None
            } else if self.peek_is_identifier() {
                Some(
                    self.parse_constant_declaration(Visibility::Private)
                        .map(Declaration::Constant),
                )
            } else {
                let span = self.peek_span();
                self.report_parse_error(&ParseError::Recovered {
                    kind: RecoveredKind::ExpectedDeclaration,
                    span,
                });
                self.synchronize();
                None
            };

            if let Some(result) = parse_result {
                match result {
                    Ok(declaration) => {
                        items.push(FileItem::Declaration(Box::new(declaration.clone())));
                    }
                    Err(error) => {
                        self.report_parse_error(&error);
                        self.synchronize();
                    }
                }
            }
        }
        items
    }

    fn parse_visibility(&mut self) -> Visibility {
        if self.peek_is_keyword(Keyword::Public) {
            self.advance();
            Visibility::Public
        } else {
            Visibility::Private
        }
    }

    fn peek_is_keyword(&self, keyword: Keyword) -> bool {
        matches!(self.peek().kind, TokenKind::Keyword(found) if found == keyword)
    }

    fn peek_is_identifier(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Identifier(_))
    }

    fn peek_is_doc_comment(&self) -> bool {
        matches!(self.peek().kind, TokenKind::DocComment(_))
    }

    fn peek_is_symbol(&self, symbol: Symbol) -> bool {
        matches!(self.peek().kind, TokenKind::Symbol(found) if found == symbol)
    }

    fn peek_second_is_symbol(&self, symbol: Symbol) -> bool {
        matches!(self.peek_n(1).kind, TokenKind::Symbol(found) if found == symbol)
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek().kind, TokenKind::EndOfFile)
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.position]
    }

    fn peek_n(&self, n: usize) -> &Token {
        let index = self.position + n;
        if index < self.tokens.len() {
            &self.tokens[index]
        } else {
            self.tokens
                .last()
                .expect("token stream must include EOF token")
        }
    }

    fn advance(&mut self) -> Token {
        let token = self.tokens[self.position].clone();
        if !matches!(token.kind, TokenKind::EndOfFile) {
            self.position += 1;
        }
        token
    }

    fn skip_statement_terminators(&mut self) {
        while matches!(self.peek().kind, TokenKind::StatementTerminator) {
            self.advance();
        }
    }

    fn parse_leading_doc_comment_block(&mut self) -> Option<DocComment> {
        if !self.peek_is_doc_comment() {
            return None;
        }
        let mut lines = Vec::new();
        let start_span = self.peek().span.clone();
        let mut end = start_span.end;
        let mut end_line = start_span.line;
        while let TokenKind::DocComment(line) = self.peek().kind.clone() {
            let token = self.advance();
            lines.push(line);
            end = token.span.end;
            end_line = token.span.line;
        }
        Some(DocComment {
            lines,
            span: Span {
                start: start_span.start,
                end,
                line: start_span.line,
                column: start_span.column,
            },
            end_line,
        })
    }

    fn peek_span(&self) -> Span {
        self.peek().span.clone()
    }

    fn error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics.push(Diagnostic::new(message, span));
    }

    fn report_parse_error(&mut self, error: &ParseError) {
        match error {
            ParseError::UnexpectedToken { kind, span } => {
                let message = match kind {
                    UnexpectedTokenKind::ExpectedIdentifier => "expected identifier".to_string(),
                    UnexpectedTokenKind::ReservedKeywordAsIdentifier { keyword } => format!(
                        "reserved keyword '{}' cannot be used as an identifier",
                        keyword.as_str()
                    ),
                    UnexpectedTokenKind::ExpectedExpression => "expected expression".to_string(),
                };
                self.error(message, span.clone());
            }
            ParseError::MissingToken { kind, span } => {
                let message = match kind {
                    MissingTokenKind::Keyword { keyword } => {
                        format!("expected keyword '{keyword:?}'")
                    }
                    MissingTokenKind::Symbol => "expected symbol".to_string(),
                };
                self.error(message, span.clone());
            }
            ParseError::InvalidConstruct { kind, span } => {
                let message = match kind {
                    InvalidConstructKind::DocCommentMustDocumentDeclaration => {
                        "doc comment must document a declaration".to_string()
                    }
                    InvalidConstructKind::FirstMethodParameterMustBeSelf => {
                        "first method parameter must be 'self'".to_string()
                    }
                    InvalidConstructKind::ConstantsRequireExplicitTypeAnnotation => {
                        "constants require an explicit type annotation".to_string()
                    }
                    InvalidConstructKind::TypeArgumentsMustBeFollowedByCall => {
                        "type arguments must be followed by a call".to_string()
                    }
                };
                self.error(message, span.clone());
            }
            ParseError::Recovered { kind, span } => {
                let message = match kind {
                    RecoveredKind::ExpectedDeclarationAfterPublic => {
                        "expected declaration after 'public'".to_string()
                    }
                    RecoveredKind::ExpectedTypeKeywordBeforeTypeDeclaration => {
                        "expected keyword 'type' before type declaration".to_string()
                    }
                    RecoveredKind::ExpectedDeclaration => "expected declaration".to_string(),
                    RecoveredKind::MethodReceiverSelfMustNotHaveTypeAnnotation => {
                        "method receiver 'self' must not have a type annotation".to_string()
                    }
                    RecoveredKind::TypeParameterListMustNotBeEmpty => {
                        "type parameter list must not be empty".to_string()
                    }
                    RecoveredKind::EnumDeclarationMustIncludeAtLeastOneVariant => {
                        "enum declaration must include at least one variant".to_string()
                    }
                    RecoveredKind::ExpectedCommaOrRightBraceAfterEnumVariant => {
                        "expected ',' or '}' after enum variant".to_string()
                    }
                    RecoveredKind::UnexpectedEqualsInExpression => {
                        "unexpected '=' in expression".to_string()
                    }
                };
                self.error(message, span.clone());
            }
            ParseError::UnparsableToken => {}
        }
    }
}

trait ExpressionSpan {
    fn span(&self) -> Span;
}

impl ExpressionSpan for Expression {
    fn span(&self) -> Span {
        match self {
            Expression::IntegerLiteral { span, .. }
            | Expression::NilLiteral { span, .. }
            | Expression::BooleanLiteral { span, .. }
            | Expression::StringLiteral { span, .. }
            | Expression::Identifier { span, .. }
            | Expression::StructLiteral { span, .. }
            | Expression::FieldAccess { span, .. }
            | Expression::Call { span, .. }
            | Expression::Unary { span, .. }
            | Expression::Binary { span, .. }
            | Expression::Match { span, .. }
            | Expression::Matches { span, .. } => span.clone(),
        }
    }
}
