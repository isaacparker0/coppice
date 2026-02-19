use crate::lexer::{Keyword, Symbol, Token, TokenKind};
use compiler__diagnostics::PhaseDiagnostic;
use compiler__source::FileRole;
use compiler__source::Span;
use compiler__syntax::{
    SyntaxDeclaration, SyntaxDocComment, SyntaxExpression, SyntaxFileItem, SyntaxMemberVisibility,
    SyntaxParsedFile, SyntaxTopLevelVisibility,
};

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
    FirstMethodParameterMustBeSelf,
    ConstantsRequireExplicitTypeAnnotation,
    TypeArgumentsMustBeFollowedByCall,
    PatternTypeArgumentsNotSupported,
}

#[derive(Clone, Debug)]
pub(super) enum RecoveredKind {
    ExpectedDeclarationAfterVisible,
    ExpectedTypeKeywordBeforeTypeDeclaration,
    ExpectedDeclaration,
    MethodReceiverSelfMustNotHaveTypeAnnotation,
    TypeParameterListMustNotBeEmpty,
    EnumDeclarationMustIncludeAtLeastOneVariant,
    ExpectedCommaOrRightBraceAfterEnumVariant,
    UnexpectedEqualsInExpression,
}

// Parser error variants represent syntactic construction failures. Parseable
// structural policy diagnostics are owned by later phases.
//
// Future parser recovery/tooling work may enrich this model with machine-usable
// metadata (for example expected-vs-found classifications and recovery hints)
// while remaining focused on parsing validity.
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

pub(crate) struct Parser {
    tokens: Vec<Token>,
    position: usize,
    parse_errors: Vec<ParseError>,
    deferred_parse_errors: Vec<ParseError>,
}

impl Parser {
    pub(crate) fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            position: 0,
            parse_errors: Vec::new(),
            deferred_parse_errors: Vec::new(),
        }
    }

    pub(crate) fn into_diagnostics(self) -> Vec<PhaseDiagnostic> {
        self.parse_errors
            .iter()
            .filter_map(Self::render_parse_error)
            .collect()
    }

    pub(crate) fn parse_file_tokens(&mut self, role: FileRole) -> SyntaxParsedFile {
        let items = self.parse_declarations();

        SyntaxParsedFile { role, items }
    }

    fn parse_declarations(&mut self) -> Vec<SyntaxFileItem> {
        let mut items = Vec::new();
        while !self.at_eof() {
            self.skip_statement_terminators();
            if let Some(doc_comment) = self.parse_leading_doc_comment_block() {
                items.push(SyntaxFileItem::DocComment(doc_comment));
            }
            if self.at_eof() {
                break;
            }
            match self.parse_declaration() {
                Ok(declaration) => {
                    items.push(SyntaxFileItem::Declaration(Box::new(declaration)));
                }
                Err(error) => {
                    self.report_parse_error(&error);
                    self.synchronize();
                }
            }
            self.flush_deferred_parse_errors();
        }
        items
    }

    fn parse_declaration(&mut self) -> ParseResult<SyntaxDeclaration> {
        if self.peek_is_keyword(Keyword::Visible) {
            let visibility = self.parse_top_level_visibility();
            if self.peek_is_keyword(Keyword::Type) {
                return self
                    .parse_type_declaration(visibility)
                    .map(SyntaxDeclaration::Type);
            }
            if self.peek_is_keyword(Keyword::Function) {
                return self
                    .parse_function(visibility)
                    .map(SyntaxDeclaration::Function);
            }
            if self.peek_is_identifier() {
                return self
                    .parse_constant_declaration(visibility)
                    .map(SyntaxDeclaration::Constant);
            }
            return Err(ParseError::Recovered {
                kind: RecoveredKind::ExpectedDeclarationAfterVisible,
                span: self.peek_span(),
            });
        }

        if self.peek_is_keyword(Keyword::Type) {
            return self
                .parse_type_declaration(SyntaxTopLevelVisibility::Private)
                .map(SyntaxDeclaration::Type);
        }
        if self.peek_is_keyword(Keyword::Import) {
            return self
                .parse_import_declaration()
                .map(SyntaxDeclaration::Import);
        }
        if self.peek_is_keyword(Keyword::Exports) {
            return self
                .parse_exports_declaration()
                .map(SyntaxDeclaration::Exports);
        }
        if self.peek_is_keyword(Keyword::Function) {
            return self
                .parse_function(SyntaxTopLevelVisibility::Private)
                .map(SyntaxDeclaration::Function);
        }
        if self.peek_is_identifier() && self.peek_second_is_symbol(Symbol::DoubleColon) {
            let span = self.peek_span();
            self.advance();
            return Err(ParseError::Recovered {
                kind: RecoveredKind::ExpectedTypeKeywordBeforeTypeDeclaration,
                span,
            });
        }
        if self.peek_is_identifier() {
            return self
                .parse_constant_declaration(SyntaxTopLevelVisibility::Private)
                .map(SyntaxDeclaration::Constant);
        }
        Err(ParseError::Recovered {
            kind: RecoveredKind::ExpectedDeclaration,
            span: self.peek_span(),
        })
    }

    fn parse_top_level_visibility(&mut self) -> SyntaxTopLevelVisibility {
        if self.peek_is_keyword(Keyword::Visible) {
            self.advance();
            SyntaxTopLevelVisibility::Visible
        } else {
            SyntaxTopLevelVisibility::Private
        }
    }

    fn parse_member_visibility(&mut self) -> SyntaxMemberVisibility {
        if self.peek_is_keyword(Keyword::Public) {
            self.advance();
            SyntaxMemberVisibility::Public
        } else {
            SyntaxMemberVisibility::Private
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

    fn parse_leading_doc_comment_block(&mut self) -> Option<SyntaxDocComment> {
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
        Some(SyntaxDocComment {
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

    fn report_parse_error(&mut self, error: &ParseError) {
        self.parse_errors.push(error.clone());
    }

    fn defer_parse_error(&mut self, error: ParseError) {
        self.deferred_parse_errors.push(error);
    }

    fn flush_deferred_parse_errors(&mut self) {
        let deferred = std::mem::take(&mut self.deferred_parse_errors);
        for error in deferred {
            self.report_parse_error(&error);
        }
    }

    fn render_parse_error(error: &ParseError) -> Option<PhaseDiagnostic> {
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
                Some(PhaseDiagnostic::new(message, span.clone()))
            }
            ParseError::MissingToken { kind, span } => {
                let message = match kind {
                    MissingTokenKind::Keyword { keyword } => {
                        format!("expected keyword '{keyword:?}'")
                    }
                    MissingTokenKind::Symbol => "expected symbol".to_string(),
                };
                Some(PhaseDiagnostic::new(message, span.clone()))
            }
            ParseError::InvalidConstruct { kind, span } => {
                let message = match kind {
                    InvalidConstructKind::FirstMethodParameterMustBeSelf => {
                        "first method parameter must be 'self'".to_string()
                    }
                    InvalidConstructKind::ConstantsRequireExplicitTypeAnnotation => {
                        "constants require an explicit type annotation".to_string()
                    }
                    InvalidConstructKind::TypeArgumentsMustBeFollowedByCall => {
                        "type arguments must be followed by a call".to_string()
                    }
                    InvalidConstructKind::PatternTypeArgumentsNotSupported => {
                        "match patterns must not include type arguments".to_string()
                    }
                };
                Some(PhaseDiagnostic::new(message, span.clone()))
            }
            ParseError::Recovered { kind, span } => {
                let message = match kind {
                    RecoveredKind::ExpectedDeclarationAfterVisible => {
                        "expected declaration after 'visible'".to_string()
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
                Some(PhaseDiagnostic::new(message, span.clone()))
            }
            ParseError::UnparsableToken => None,
        }
    }
}

trait ExpressionSpan {
    fn span(&self) -> Span;
}

impl ExpressionSpan for SyntaxExpression {
    fn span(&self) -> Span {
        match self {
            SyntaxExpression::IntegerLiteral { span, .. }
            | SyntaxExpression::NilLiteral { span, .. }
            | SyntaxExpression::BooleanLiteral { span, .. }
            | SyntaxExpression::StringLiteral { span, .. }
            | SyntaxExpression::NameReference { span, .. }
            | SyntaxExpression::StructLiteral { span, .. }
            | SyntaxExpression::FieldAccess { span, .. }
            | SyntaxExpression::Call { span, .. }
            | SyntaxExpression::Unary { span, .. }
            | SyntaxExpression::Binary { span, .. }
            | SyntaxExpression::Match { span, .. }
            | SyntaxExpression::Matches { span, .. } => span.clone(),
        }
    }
}
