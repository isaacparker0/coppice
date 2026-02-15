use crate::lexer::{Keyword, Symbol, Token, TokenKind};
use compiler__diagnostics::Diagnostic;
use compiler__source::FileRole;
use compiler__source::Span;
use compiler__syntax::{
    ConstantDeclaration, Declaration, DocComment, Expression, ParsedFile, Visibility,
};

mod declarations;
mod exports;
mod expressions;
mod imports;
mod recovery;
mod statements;
mod types;

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
        let declarations = self.parse_declarations();

        ParsedFile { role, declarations }
    }

    fn parse_declarations(&mut self) -> Vec<Declaration> {
        let mut declarations = Vec::new();
        let mut saw_non_import_declaration = false;
        while !self.at_eof() {
            self.skip_statement_terminators();
            let mut doc = self.parse_leading_doc_comment_block();
            if self.at_eof() {
                if let Some(doc) = doc {
                    self.error("doc comment must document a declaration", doc.span);
                }
                break;
            }
            if let Some(found_doc) = doc.as_ref()
                && self.peek_span().line != found_doc.end_line + 1
            {
                self.error(
                    "doc comment must document a declaration",
                    found_doc.span.clone(),
                );
                doc = None;
            }
            if self.peek_is_keyword(Keyword::Public) {
                saw_non_import_declaration = true;
                let visibility = self.parse_visibility();
                if self.peek_is_keyword(Keyword::Type) {
                    if let Some(type_declaration) = self.parse_type_declaration(visibility, doc) {
                        declarations.push(Declaration::Type(type_declaration));
                    } else {
                        self.synchronize();
                    }
                } else if self.peek_is_keyword(Keyword::Function) {
                    if let Some(function_declaration) = self.parse_function(visibility, doc) {
                        declarations.push(Declaration::Function(function_declaration));
                    } else {
                        self.synchronize();
                    }
                } else if self.peek_is_identifier() {
                    if let Some(constant_declaration) = self.parse_constant_declaration(visibility)
                    {
                        declarations.push(Declaration::Constant(ConstantDeclaration {
                            doc,
                            ..constant_declaration
                        }));
                    } else {
                        self.synchronize();
                    }
                } else {
                    if let Some(doc) = doc {
                        self.error("doc comment must document a declaration", doc.span);
                    }
                    let span = self.peek_span();
                    self.error("expected declaration after 'public'", span);
                    self.synchronize();
                }
            } else if self.peek_is_keyword(Keyword::Type) {
                if let Some(type_declaration) =
                    self.parse_type_declaration(Visibility::Private, doc)
                {
                    saw_non_import_declaration = true;
                    declarations.push(Declaration::Type(type_declaration));
                } else {
                    saw_non_import_declaration = true;
                    self.synchronize();
                }
            } else if self.peek_is_keyword(Keyword::Import) {
                if let Some(import_declaration) = self.parse_import_declaration() {
                    if saw_non_import_declaration {
                        self.error(
                            "import declarations must appear before top-level declarations",
                            import_declaration.span.clone(),
                        );
                    }
                    declarations.push(Declaration::Import(import_declaration));
                } else {
                    self.synchronize();
                }
            } else if self.peek_is_keyword(Keyword::Exports) {
                saw_non_import_declaration = true;
                if let Some(exports_declaration) = self.parse_exports_declaration() {
                    declarations.push(Declaration::Exports(exports_declaration));
                } else {
                    self.synchronize();
                }
            } else if self.peek_is_keyword(Keyword::Function) {
                saw_non_import_declaration = true;
                if let Some(function_declaration) = self.parse_function(Visibility::Private, doc) {
                    declarations.push(Declaration::Function(function_declaration));
                } else {
                    self.synchronize();
                }
            } else if self.peek_is_identifier() && self.peek_second_is_symbol(Symbol::DoubleColon) {
                saw_non_import_declaration = true;
                if let Some(doc) = doc {
                    self.error("doc comment must document a declaration", doc.span);
                }
                let span = self.peek_span();
                self.error("expected keyword 'type' before type declaration", span);
                self.advance();
                self.synchronize();
            } else if self.peek_is_identifier() {
                saw_non_import_declaration = true;
                if let Some(constant_declaration) =
                    self.parse_constant_declaration(Visibility::Private)
                {
                    declarations.push(Declaration::Constant(ConstantDeclaration {
                        doc,
                        ..constant_declaration
                    }));
                } else {
                    self.synchronize();
                }
            } else {
                saw_non_import_declaration = true;
                if let Some(doc) = doc {
                    self.error("doc comment must document a declaration", doc.span);
                }
                let span = self.peek_span();
                self.error("expected declaration", span);
                self.synchronize();
            }
        }
        declarations
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
