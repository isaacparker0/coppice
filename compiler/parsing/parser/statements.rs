use crate::lexer::{Keyword, Symbol};
use compiler__source::Span;
use compiler__syntax::{Block, Statement};

use super::{ExpressionSpan, ParseError, ParseResult, Parser};

impl Parser {
    pub(super) fn parse_block(&mut self) -> ParseResult<Block> {
        let start = self.expect_symbol(Symbol::LeftBrace)?;
        let mut statements = Vec::new();
        self.skip_statement_terminators();
        while !self.peek_is_symbol(Symbol::RightBrace) && !self.at_eof() {
            match self.parse_statement() {
                Ok(statement) => statements.push(statement),
                Err(error) => {
                    self.report_parse_error(&error);
                    self.synchronize_statement();
                }
            }
            self.skip_statement_terminators();
        }
        let end = self.expect_symbol(Symbol::RightBrace)?;
        Ok(Block {
            statements,
            span: Span {
                start: start.start,
                end: end.end,
                line: start.line,
                column: start.column,
            },
        })
    }

    pub(super) fn parse_statement(&mut self) -> ParseResult<Statement> {
        if self.peek_is_doc_comment() {
            if let Some(doc) = self.parse_leading_doc_comment_block() {
                return Err(ParseError::InvalidConstruct {
                    message: "doc comment must document a declaration".to_string(),
                    span: doc.span,
                });
            }
            return Err(ParseError::InvalidConstruct {
                message: "doc comment must document a declaration".to_string(),
                span: self.peek_span(),
            });
        }
        if self.peek_is_keyword(Keyword::Return) {
            let span = self.expect_keyword(Keyword::Return)?;
            let value = self.parse_expression()?;
            return Ok(Statement::Return { value, span });
        }
        if self.peek_is_keyword(Keyword::Abort) {
            let start = self.expect_keyword(Keyword::Abort)?;
            self.expect_symbol(Symbol::LeftParenthesis)?;
            let message = self.parse_expression()?;
            let right_parenthesis = self.expect_symbol(Symbol::RightParenthesis)?;
            let span = Span {
                start: start.start,
                end: right_parenthesis.end,
                line: start.line,
                column: start.column,
            };
            return Ok(Statement::Abort { message, span });
        }
        if self.peek_is_keyword(Keyword::Break) {
            let span = self.expect_keyword(Keyword::Break)?;
            return Ok(Statement::Break { span });
        }
        if self.peek_is_keyword(Keyword::Continue) {
            let span = self.expect_keyword(Keyword::Continue)?;
            return Ok(Statement::Continue { span });
        }
        if self.peek_is_keyword(Keyword::If) {
            let start = self.expect_keyword(Keyword::If)?;
            let condition = self.parse_expression()?;
            let then_block = self.parse_block()?;
            let else_block = if self.peek_is_keyword(Keyword::Else) {
                self.advance();
                Some(self.parse_block()?)
            } else {
                None
            };
            let end_span = else_block
                .as_ref()
                .map_or_else(|| then_block.span.clone(), |block| block.span.clone());
            let span = Span {
                start: start.start,
                end: end_span.end,
                line: start.line,
                column: start.column,
            };
            return Ok(Statement::If {
                condition,
                then_block,
                else_block,
                span,
            });
        }
        if self.peek_is_keyword(Keyword::For) {
            let start = self.expect_keyword(Keyword::For)?;
            let condition = if self.peek_is_symbol(Symbol::LeftBrace) {
                None
            } else {
                Some(self.parse_expression()?)
            };
            let body = self.parse_block()?;
            let span = Span {
                start: start.start,
                end: body.span.end,
                line: start.line,
                column: start.column,
            };
            return Ok(Statement::For {
                condition,
                body,
                span,
            });
        }

        if self.peek_is_keyword(Keyword::Mut) {
            self.advance();
            let (name, name_span) = self.expect_identifier()?;
            let type_name = if self.peek_is_symbol(Symbol::Colon) {
                self.advance();
                Some(self.parse_type_name()?)
            } else {
                None
            };
            self.expect_symbol(Symbol::Assign)?;
            let initializer = self.parse_expression()?;
            let span = Span {
                start: name_span.start,
                end: initializer.span().end,
                line: name_span.line,
                column: name_span.column,
            };
            return Ok(Statement::Let {
                name,
                mutable: true,
                type_name,
                initializer,
                span,
            });
        }

        if self.peek_is_identifier() && self.peek_second_is_symbol(Symbol::Equal) {
            let (name, name_span) = self.expect_identifier()?;
            self.advance();
            let value = self.parse_expression()?;
            let span = Span {
                start: name_span.start,
                end: value.span().end,
                line: name_span.line,
                column: name_span.column,
            };
            return Ok(Statement::Assign {
                name,
                name_span,
                value,
                span,
            });
        }

        if self.peek_is_identifier()
            && (self.peek_second_is_symbol(Symbol::Colon)
                || self.peek_second_is_symbol(Symbol::Assign))
        {
            let (name, name_span) = self.expect_identifier()?;
            let type_name = if self.peek_is_symbol(Symbol::Colon) {
                self.advance();
                Some(self.parse_type_name()?)
            } else {
                None
            };
            self.expect_symbol(Symbol::Assign)?;
            let initializer = self.parse_expression()?;
            let span = Span {
                start: name_span.start,
                end: initializer.span().end,
                line: name_span.line,
                column: name_span.column,
            };
            return Ok(Statement::Let {
                name,
                mutable: false,
                type_name,
                initializer,
                span,
            });
        }

        let value = self.parse_expression()?;
        let span = value.span();
        Ok(Statement::Expression { value, span })
    }
}
