use crate::lexer::{Keyword, Symbol};
use compiler__source::Span;
use compiler__syntax::{SyntaxBlock, SyntaxBlockItem, SyntaxStatement};

use super::{ExpressionSpan, ParseResult, Parser};

impl Parser {
    pub(super) fn parse_block(&mut self) -> ParseResult<SyntaxBlock> {
        let start = self.expect_symbol(Symbol::LeftBrace)?;
        let mut items = Vec::new();
        self.skip_statement_terminators();
        while !self.peek_is_symbol(Symbol::RightBrace) && !self.at_eof() {
            if let Some(doc_comment) = self.parse_leading_doc_comment_block() {
                items.push(SyntaxBlockItem::DocComment(doc_comment));
                self.skip_statement_terminators();
                continue;
            }
            match self.parse_statement() {
                Ok(statement) => items.push(SyntaxBlockItem::Statement(statement)),
                Err(error) => {
                    self.report_parse_error(&error);
                    self.synchronize_statement();
                }
            }
            self.skip_statement_terminators();
        }
        let end = self.expect_symbol(Symbol::RightBrace)?;
        Ok(SyntaxBlock {
            items,
            span: Span {
                start: start.start,
                end: end.end,
                line: start.line,
                column: start.column,
            },
        })
    }

    pub(super) fn parse_statement(&mut self) -> ParseResult<SyntaxStatement> {
        if self.peek_is_keyword(Keyword::Return) {
            let span = self.expect_keyword(Keyword::Return)?;
            let value = self.parse_expression()?;
            return Ok(SyntaxStatement::Return { value, span });
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
            return Ok(SyntaxStatement::Abort { message, span });
        }
        if self.peek_is_keyword(Keyword::Break) {
            let span = self.expect_keyword(Keyword::Break)?;
            return Ok(SyntaxStatement::Break { span });
        }
        if self.peek_is_keyword(Keyword::Continue) {
            let span = self.expect_keyword(Keyword::Continue)?;
            return Ok(SyntaxStatement::Continue { span });
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
            return Ok(SyntaxStatement::If {
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
            return Ok(SyntaxStatement::For {
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
            return Ok(SyntaxStatement::Binding {
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
            return Ok(SyntaxStatement::Assign {
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
            return Ok(SyntaxStatement::Binding {
                name,
                mutable: false,
                type_name,
                initializer,
                span,
            });
        }

        let value = self.parse_expression()?;
        let span = value.span();
        Ok(SyntaxStatement::Expression { value, span })
    }
}
