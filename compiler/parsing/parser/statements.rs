use crate::lexer::{Keyword, Symbol, TokenKind};
use compiler__source::Span;
use compiler__syntax::{
    SyntaxAssignTarget, SyntaxBlock, SyntaxBlockItem, SyntaxExpression, SyntaxStatement,
};

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
            let value = if self.can_start_return_expression() {
                Some(self.parse_expression()?)
            } else {
                None
            };
            return Ok(SyntaxStatement::Return { value, span });
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
                name_span,
                mutable: true,
                type_name,
                initializer,
                span,
            });
        }

        if self.peek_is_identifier() {
            let checkpoint = self.checkpoint();
            let assignment_target = self.parse_postfix();
            if let Ok(assignment_target) = assignment_target
                && self.peek_is_symbol(Symbol::Equal)
            {
                self.advance();
                let value = self.parse_expression()?;
                return match assignment_target {
                    SyntaxExpression::NameReference { name, span, .. } => {
                        let statement_span = Span {
                            start: span.start,
                            end: value.span().end,
                            line: span.line,
                            column: span.column,
                        };
                        Ok(SyntaxStatement::Assign {
                            target: SyntaxAssignTarget::Name {
                                name,
                                name_span: span.clone(),
                                span,
                            },
                            value,
                            span: statement_span,
                        })
                    }
                    SyntaxExpression::IndexAccess {
                        target,
                        index,
                        span,
                    } => {
                        let statement_span = Span {
                            start: span.start,
                            end: value.span().end,
                            line: span.line,
                            column: span.column,
                        };
                        Ok(SyntaxStatement::Assign {
                            target: SyntaxAssignTarget::Index {
                                target,
                                index,
                                span,
                            },
                            value,
                            span: statement_span,
                        })
                    }
                    _ => {
                        self.restore(checkpoint);
                        let value = self.parse_expression()?;
                        let span = value.span();
                        Ok(SyntaxStatement::Expression { value, span })
                    }
                };
            }
            self.restore(checkpoint);
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
                name_span,
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

    fn can_start_return_expression(&self) -> bool {
        !matches!(
            self.peek().kind,
            TokenKind::StatementTerminator
                | TokenKind::EndOfFile
                | TokenKind::Symbol(Symbol::RightBrace)
        )
    }
}
