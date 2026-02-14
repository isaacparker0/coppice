use crate::lexer::{Keyword, Symbol, TokenKind};
use compiler__source::Span;
use compiler__syntax::{
    BinaryOperator, Expression, MatchArm, MatchPattern, StructLiteralField, TypeName, TypeNameAtom,
    UnaryOperator,
};

use super::{ExpressionSpan, Parser};

impl Parser {
    pub(super) fn parse_expression(&mut self) -> Option<Expression> {
        self.parse_or()
    }

    pub(super) fn parse_or(&mut self) -> Option<Expression> {
        let mut expression = self.parse_and()?;
        loop {
            if !self.peek_is_keyword(Keyword::Or) {
                break;
            }
            let operator_span = self.advance().span.clone();
            let right = self.parse_and()?;
            let span = Span {
                start: expression.span().start,
                end: right.span().end,
                line: operator_span.line,
                column: operator_span.column,
            };
            expression = Expression::Binary {
                operator: BinaryOperator::Or,
                left: Box::new(expression),
                right: Box::new(right),
                span,
            };
        }
        Some(expression)
    }

    pub(super) fn parse_and(&mut self) -> Option<Expression> {
        let mut expression = self.parse_equality()?;
        loop {
            if !self.peek_is_keyword(Keyword::And) {
                break;
            }
            let operator_span = self.advance().span.clone();
            let right = self.parse_equality()?;
            let span = Span {
                start: expression.span().start,
                end: right.span().end,
                line: operator_span.line,
                column: operator_span.column,
            };
            expression = Expression::Binary {
                operator: BinaryOperator::And,
                left: Box::new(expression),
                right: Box::new(right),
                span,
            };
        }
        Some(expression)
    }

    pub(super) fn parse_equality(&mut self) -> Option<Expression> {
        let mut expression = self.parse_comparison()?;
        loop {
            if self.peek_is_symbol(Symbol::Equal) {
                let operator_span = self.advance().span.clone();
                self.error("unexpected '=' in expression", operator_span);
                let _ = self.parse_comparison();
                continue;
            }
            let operator = if self.peek_is_symbol(Symbol::EqualEqual) {
                BinaryOperator::EqualEqual
            } else if self.peek_is_symbol(Symbol::BangEqual) {
                BinaryOperator::NotEqual
            } else {
                break;
            };
            let operator_span = self.advance().span.clone();
            let right = self.parse_comparison()?;
            let span = Span {
                start: expression.span().start,
                end: right.span().end,
                line: operator_span.line,
                column: operator_span.column,
            };
            expression = Expression::Binary {
                operator,
                left: Box::new(expression),
                right: Box::new(right),
                span,
            };
        }
        Some(expression)
    }

    pub(super) fn parse_comparison(&mut self) -> Option<Expression> {
        let mut expression = self.parse_additive()?;
        loop {
            if self.peek_is_keyword(Keyword::Matches) {
                let operator_span = self.advance().span.clone();
                let type_name = self.parse_type_name()?;
                let span = Span {
                    start: expression.span().start,
                    end: type_name.span.end,
                    line: operator_span.line,
                    column: operator_span.column,
                };
                expression = Expression::Matches {
                    value: Box::new(expression),
                    type_name,
                    span,
                };
                continue;
            }
            let operator = if self.peek_is_symbol(Symbol::Less) {
                BinaryOperator::LessThan
            } else if self.peek_is_symbol(Symbol::LessEqual) {
                BinaryOperator::LessThanOrEqual
            } else if self.peek_is_symbol(Symbol::Greater) {
                BinaryOperator::GreaterThan
            } else if self.peek_is_symbol(Symbol::GreaterEqual) {
                BinaryOperator::GreaterThanOrEqual
            } else {
                break;
            };
            let operator_span = self.advance().span.clone();
            let right = self.parse_additive()?;
            let span = Span {
                start: expression.span().start,
                end: right.span().end,
                line: operator_span.line,
                column: operator_span.column,
            };
            expression = Expression::Binary {
                operator,
                left: Box::new(expression),
                right: Box::new(right),
                span,
            };
        }
        Some(expression)
    }

    pub(super) fn parse_additive(&mut self) -> Option<Expression> {
        let mut expression = self.parse_multiplicative()?;
        loop {
            let operator = if self.peek_is_symbol(Symbol::Plus) {
                BinaryOperator::Add
            } else if self.peek_is_symbol(Symbol::Minus) {
                BinaryOperator::Subtract
            } else {
                break;
            };
            let operator_span = self.advance().span.clone();
            let right = self.parse_multiplicative()?;
            let span = Span {
                start: expression.span().start,
                end: right.span().end,
                line: operator_span.line,
                column: operator_span.column,
            };
            expression = Expression::Binary {
                operator,
                left: Box::new(expression),
                right: Box::new(right),
                span,
            };
        }
        Some(expression)
    }

    pub(super) fn parse_multiplicative(&mut self) -> Option<Expression> {
        let mut expression = self.parse_unary()?;
        loop {
            let operator = if self.peek_is_symbol(Symbol::Star) {
                BinaryOperator::Multiply
            } else if self.peek_is_symbol(Symbol::Slash) {
                BinaryOperator::Divide
            } else {
                break;
            };
            let operator_span = self.advance().span.clone();
            let right = self.parse_postfix()?;
            let span = Span {
                start: expression.span().start,
                end: right.span().end,
                line: operator_span.line,
                column: operator_span.column,
            };
            expression = Expression::Binary {
                operator,
                left: Box::new(expression),
                right: Box::new(right),
                span,
            };
        }
        Some(expression)
    }

    pub(super) fn parse_postfix(&mut self) -> Option<Expression> {
        let mut expression = self.parse_primary()?;
        loop {
            if self.peek_is_symbol(Symbol::LeftParenthesis) {
                let left_parenthesis = self.expect_symbol(Symbol::LeftParenthesis)?;
                let arguments = self.parse_arguments();
                let right_parenthesis = self.expect_symbol(Symbol::RightParenthesis)?;
                let span = Span {
                    start: expression.span().start,
                    end: right_parenthesis.end,
                    line: left_parenthesis.line,
                    column: left_parenthesis.column,
                };
                expression = Expression::Call {
                    callee: Box::new(expression),
                    arguments,
                    span,
                };
                continue;
            }
            if self.peek_is_symbol(Symbol::Dot) {
                let dot = self.expect_symbol(Symbol::Dot)?;
                let (field, field_span) = self.expect_identifier()?;
                let span = Span {
                    start: expression.span().start,
                    end: field_span.end,
                    line: dot.line,
                    column: dot.column,
                };
                expression = Expression::FieldAccess {
                    target: Box::new(expression),
                    field,
                    field_span,
                    span,
                };
                continue;
            }
            break;
        }
        Some(expression)
    }

    pub(super) fn parse_unary(&mut self) -> Option<Expression> {
        if self.peek_is_keyword(Keyword::Not) {
            let operator_span = self.advance().span.clone();
            let expression = self.parse_unary()?;
            let span = Span {
                start: operator_span.start,
                end: expression.span().end,
                line: operator_span.line,
                column: operator_span.column,
            };
            return Some(Expression::Unary {
                operator: UnaryOperator::Not,
                expression: Box::new(expression),
                span,
            });
        }
        if self.peek_is_symbol(Symbol::Minus) {
            let operator_span = self.advance().span.clone();
            let expression = self.parse_unary()?;
            let span = Span {
                start: operator_span.start,
                end: expression.span().end,
                line: operator_span.line,
                column: operator_span.column,
            };
            return Some(Expression::Unary {
                operator: UnaryOperator::Negate,
                expression: Box::new(expression),
                span,
            });
        }
        self.parse_postfix()
    }

    pub(super) fn parse_arguments(&mut self) -> Vec<Expression> {
        let mut arguments = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightParenthesis) {
            return arguments;
        }
        loop {
            self.skip_statement_terminators();
            if let Some(argument) = self.parse_expression() {
                arguments.push(argument);
            } else {
                self.synchronize_list_item(Symbol::Comma, Symbol::RightParenthesis);
                if self.peek_is_symbol(Symbol::RightParenthesis) {
                    break;
                }
            }

            self.skip_statement_terminators();
            if self.peek_is_symbol(Symbol::Comma) {
                self.advance();
                self.skip_statement_terminators();
                if self.peek_is_symbol(Symbol::RightParenthesis) {
                    break;
                }
                continue;
            }
            break;
        }
        arguments
    }

    pub(super) fn parse_primary(&mut self) -> Option<Expression> {
        let token = self.advance();
        match token.kind {
            TokenKind::IntegerLiteral(value) => Some(Expression::IntegerLiteral {
                value,
                span: token.span,
            }),
            TokenKind::Keyword(Keyword::Nil) => Some(Expression::NilLiteral { span: token.span }),
            TokenKind::StringLiteral(value) => Some(Expression::StringLiteral {
                value,
                span: token.span,
            }),
            TokenKind::BooleanLiteral(value) => Some(Expression::BooleanLiteral {
                value,
                span: token.span,
            }),
            TokenKind::Identifier(name) => {
                if self.peek_is_symbol(Symbol::LeftBrace)
                    && name
                        .chars()
                        .next()
                        .is_some_and(|ch| ch.is_ascii_uppercase())
                {
                    let type_name = TypeName {
                        names: vec![TypeNameAtom {
                            name,
                            span: token.span.clone(),
                        }],
                        span: token.span,
                    };
                    return self.parse_struct_literal(type_name);
                }
                Some(Expression::Identifier {
                    name,
                    span: token.span,
                })
            }
            TokenKind::Keyword(Keyword::Match) => self.parse_match_expression(&token.span),
            TokenKind::Symbol(Symbol::LeftParenthesis) => {
                let expression = self.parse_expression()?;
                self.expect_symbol(Symbol::RightParenthesis)?;
                Some(expression)
            }
            TokenKind::Error(_message) => None,
            _ => {
                self.error("expected expression", token.span);
                None
            }
        }
    }

    pub(super) fn parse_struct_literal(&mut self, type_name: TypeName) -> Option<Expression> {
        let left_brace = self.expect_symbol(Symbol::LeftBrace)?;
        let fields = self.parse_struct_literal_fields();
        let right_brace = self.expect_symbol(Symbol::RightBrace)?;
        let span = Span {
            start: type_name.span.start,
            end: right_brace.end,
            line: left_brace.line,
            column: left_brace.column,
        };
        Some(Expression::StructLiteral {
            type_name,
            fields,
            span,
        })
    }

    pub(super) fn parse_match_expression(&mut self, start_span: &Span) -> Option<Expression> {
        let target = self.parse_expression()?;
        self.expect_symbol(Symbol::LeftBrace)?;
        let arms = self.parse_match_arms();
        let right_brace = self.expect_symbol(Symbol::RightBrace)?;
        let span = Span {
            start: start_span.start,
            end: right_brace.end,
            line: start_span.line,
            column: start_span.column,
        };
        Some(Expression::Match {
            target: Box::new(target),
            arms,
            span,
        })
    }

    pub(super) fn parse_match_arms(&mut self) -> Vec<MatchArm> {
        let mut arms = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightBrace) {
            return arms;
        }
        loop {
            self.skip_statement_terminators();
            if let Some(arm) = self.parse_match_arm() {
                arms.push(arm);
            } else {
                self.synchronize_list_item(Symbol::Comma, Symbol::RightBrace);
                if self.peek_is_symbol(Symbol::RightBrace) {
                    break;
                }
            }

            self.skip_statement_terminators();
            if self.peek_is_symbol(Symbol::Comma) {
                self.advance();
                self.skip_statement_terminators();
                if self.peek_is_symbol(Symbol::RightBrace) {
                    break;
                }
                continue;
            }
            if self.peek_is_symbol(Symbol::RightBrace) {
                break;
            }
            if self.peek_is_identifier() {
                continue;
            }
            break;
        }
        arms
    }

    pub(super) fn parse_match_arm(&mut self) -> Option<MatchArm> {
        let pattern = self.parse_match_pattern()?;
        self.expect_symbol(Symbol::FatArrow)?;
        let value = self.parse_expression()?;
        let span = Span {
            start: pattern.span().start,
            end: value.span().end,
            line: pattern.span().line,
            column: pattern.span().column,
        };
        Some(MatchArm {
            pattern,
            value,
            span,
        })
    }

    pub(super) fn parse_match_pattern(&mut self) -> Option<MatchPattern> {
        let (name, name_span) = self.expect_identifier()?;
        if self.peek_is_symbol(Symbol::Colon) {
            self.advance();
            let type_name = self.parse_type_name()?;
            let span = Span {
                start: name_span.start,
                end: type_name.span.end,
                line: name_span.line,
                column: name_span.column,
            };
            return Some(MatchPattern::Binding {
                name,
                name_span,
                type_name,
                span,
            });
        }

        let type_name = TypeName {
            names: vec![TypeNameAtom {
                name,
                span: name_span.clone(),
            }],
            span: name_span.clone(),
        };
        Some(MatchPattern::Type {
            type_name,
            span: name_span,
        })
    }

    pub(super) fn parse_struct_literal_fields(&mut self) -> Vec<StructLiteralField> {
        let mut fields = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightBrace) {
            return fields;
        }
        loop {
            self.skip_statement_terminators();
            if let Some(field) = self.parse_struct_literal_field() {
                fields.push(field);
            } else {
                self.synchronize_list_item(Symbol::Comma, Symbol::RightBrace);
                if self.peek_is_symbol(Symbol::RightBrace) {
                    break;
                }
            }

            self.skip_statement_terminators();
            if self.peek_is_symbol(Symbol::Comma) {
                self.advance();
                self.skip_statement_terminators();
                if self.peek_is_symbol(Symbol::RightBrace) {
                    break;
                }
                continue;
            }
            break;
        }
        fields
    }

    pub(super) fn parse_struct_literal_field(&mut self) -> Option<StructLiteralField> {
        let (name, name_span) = self.expect_identifier()?;
        self.expect_symbol(Symbol::Colon)?;
        let value = self.parse_expression()?;
        let span = Span {
            start: name_span.start,
            end: value.span().end,
            line: name_span.line,
            column: name_span.column,
        };
        Some(StructLiteralField {
            name,
            name_span,
            value,
            span,
        })
    }
}
