use crate::lexer::{Keyword, Symbol, TokenKind};
use compiler__source::Span;

use super::{MissingTokenKind, ParseError, ParseResult, Parser, UnexpectedTokenKind};

impl Parser {
    pub(super) fn expect_identifier(&mut self) -> ParseResult<(String, Span)> {
        let token = self.advance();
        match token.kind {
            TokenKind::Identifier(name) => Ok((name, token.span)),
            TokenKind::Keyword(keyword) => Err(ParseError::UnexpectedToken {
                kind: UnexpectedTokenKind::ReservedKeywordAsIdentifier { keyword },
                span: token.span,
            }),
            _ => Err(ParseError::UnexpectedToken {
                kind: UnexpectedTokenKind::ExpectedIdentifier,
                span: token.span,
            }),
        }
    }

    pub(super) fn expect_type_name_part(&mut self) -> ParseResult<(String, Span)> {
        let token = self.advance();
        match token.kind {
            TokenKind::Identifier(name) => {
                let mut full_name = name;
                let mut span = token.span;
                while self.peek_is_symbol(Symbol::Dot) {
                    self.advance();
                    let (segment, segment_span) = self.expect_identifier()?;
                    full_name.push('.');
                    full_name.push_str(&segment);
                    span.end = segment_span.end;
                }
                Ok((full_name, span))
            }
            TokenKind::Keyword(Keyword::Nil) => Ok((Keyword::Nil.as_str().to_string(), token.span)),
            TokenKind::Keyword(keyword) => Err(ParseError::UnexpectedToken {
                kind: UnexpectedTokenKind::ReservedKeywordAsIdentifier { keyword },
                span: token.span,
            }),
            _ => Err(ParseError::UnexpectedToken {
                kind: UnexpectedTokenKind::ExpectedIdentifier,
                span: token.span,
            }),
        }
    }

    pub(super) fn expect_keyword(&mut self, keyword: Keyword) -> ParseResult<Span> {
        let token = self.advance();
        match token.kind {
            TokenKind::Keyword(found) if found == keyword => Ok(token.span),
            _ => Err(ParseError::MissingToken {
                kind: MissingTokenKind::Keyword { keyword },
                span: token.span,
            }),
        }
    }

    pub(super) fn expect_symbol(&mut self, symbol: Symbol) -> ParseResult<Span> {
        let token = self.advance();
        match token.kind {
            TokenKind::Symbol(found) if found == symbol => Ok(token.span),
            _ => Err(ParseError::MissingToken {
                kind: MissingTokenKind::Symbol,
                span: token.span,
            }),
        }
    }

    pub(super) fn synchronize(&mut self) {
        while !self.at_eof() {
            if self.peek_is_keyword(Keyword::Import)
                || self.peek_is_keyword(Keyword::Exports)
                || self.peek_is_keyword(Keyword::Type)
                || self.peek_is_keyword(Keyword::Function)
            {
                return;
            }
            if self.peek_is_identifier()
                && (self.peek_second_is_symbol(Symbol::Assign)
                    || self.peek_second_is_symbol(Symbol::DoubleColon))
            {
                return;
            }
            self.advance();
        }
    }

    pub(super) fn synchronize_list_item(&mut self, separator: Symbol, end: Symbol) {
        while !self.at_eof() {
            self.skip_statement_terminators();
            if self.peek_is_symbol(separator) || self.peek_is_symbol(end) {
                return;
            }
            self.advance();
        }
    }

    pub(super) fn synchronize_statement(&mut self) {
        while !self.at_eof() {
            if self.peek_is_symbol(Symbol::RightBrace) {
                return;
            }
            if self.peek_is_keyword(Keyword::Return)
                || self.peek_is_keyword(Keyword::Abort)
                || self.peek_is_keyword(Keyword::Break)
                || self.peek_is_keyword(Keyword::Continue)
                || self.peek_is_keyword(Keyword::If)
                || self.peek_is_keyword(Keyword::For)
            {
                return;
            }
            if matches!(self.peek().kind, TokenKind::StatementTerminator) {
                self.advance();
                return;
            }
            self.advance();
        }
    }
}
