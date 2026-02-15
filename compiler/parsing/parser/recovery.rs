use crate::lexer::{Keyword, Symbol, TokenKind};
use compiler__source::Span;

use super::Parser;

impl Parser {
    pub(super) fn expect_identifier(&mut self) -> Option<(String, Span)> {
        let token = self.advance();
        match token.kind {
            TokenKind::Identifier(name) => Some((name, token.span)),
            TokenKind::Keyword(keyword) => {
                self.error(
                    format!(
                        "reserved keyword '{}' cannot be used as an identifier",
                        keyword.as_str()
                    ),
                    token.span,
                );
                None
            }
            _ => {
                self.error("expected identifier", token.span);
                None
            }
        }
    }

    pub(super) fn expect_type_name_part(&mut self) -> Option<(String, Span)> {
        let token = self.advance();
        match token.kind {
            TokenKind::Identifier(name) => Some((name, token.span)),
            TokenKind::Keyword(Keyword::Nil) => {
                Some((Keyword::Nil.as_str().to_string(), token.span))
            }
            TokenKind::Keyword(keyword) => {
                self.error(
                    format!(
                        "reserved keyword '{}' cannot be used as an identifier",
                        keyword.as_str()
                    ),
                    token.span,
                );
                None
            }
            _ => {
                self.error("expected identifier", token.span);
                None
            }
        }
    }

    pub(super) fn expect_keyword(&mut self, keyword: Keyword) -> Option<Span> {
        let token = self.advance();
        match token.kind {
            TokenKind::Keyword(found) if found == keyword => Some(token.span),
            _ => {
                self.error(format!("expected keyword '{keyword:?}'"), token.span);
                None
            }
        }
    }

    pub(super) fn expect_symbol(&mut self, symbol: Symbol) -> Option<Span> {
        let token = self.advance();
        match token.kind {
            TokenKind::Symbol(found) if found == symbol => Some(token.span),
            _ => {
                self.error("expected symbol", token.span);
                None
            }
        }
    }

    pub(super) fn synchronize(&mut self) {
        while !self.at_eof() {
            if self.peek_is_keyword(Keyword::Import)
                || self.peek_is_keyword(Keyword::Export)
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
