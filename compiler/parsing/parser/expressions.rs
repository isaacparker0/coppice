use crate::lexer::{Keyword, Symbol, TokenKind};
use compiler__source::Span;
use compiler__syntax::{
    BinaryOperator, Expression, MatchArm, MatchPattern, StructLiteralField, TypeName, TypeNameAtom,
    UnaryOperator,
};

use super::{ExpressionSpan, ParseError, ParseResult, Parser};

impl Parser {
    pub(super) fn parse_expression(&mut self) -> ParseResult<Expression> {
        self.parse_or()
    }

    pub(super) fn parse_or(&mut self) -> ParseResult<Expression> {
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
        Ok(expression)
    }

    pub(super) fn parse_and(&mut self) -> ParseResult<Expression> {
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
        Ok(expression)
    }

    pub(super) fn parse_equality(&mut self) -> ParseResult<Expression> {
        let mut expression = self.parse_comparison()?;
        loop {
            if self.peek_is_symbol(Symbol::Equal) {
                let operator_span = self.advance().span.clone();
                self.report_parse_error(&ParseError::Recovered {
                    message: "unexpected '=' in expression".to_string(),
                    span: operator_span,
                });
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
        Ok(expression)
    }

    pub(super) fn parse_comparison(&mut self) -> ParseResult<Expression> {
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
        Ok(expression)
    }

    pub(super) fn parse_additive(&mut self) -> ParseResult<Expression> {
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
        Ok(expression)
    }

    pub(super) fn parse_multiplicative(&mut self) -> ParseResult<Expression> {
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
        Ok(expression)
    }

    pub(super) fn parse_postfix(&mut self) -> ParseResult<Expression> {
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
                    type_arguments: Vec::new(),
                    arguments,
                    span,
                };
                continue;
            }
            if self.peek_is_symbol(Symbol::LeftBracket) {
                let (type_arguments, right_bracket) = self.parse_type_argument_list()?;
                let Ok(left_parenthesis) = self.expect_symbol(Symbol::LeftParenthesis) else {
                    return Err(ParseError::InvalidConstruct {
                        message: "type arguments must be followed by a call".to_string(),
                        span: right_bracket,
                    });
                };
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
                    type_arguments,
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
        Ok(expression)
    }

    pub(super) fn parse_unary(&mut self) -> ParseResult<Expression> {
        if self.peek_is_keyword(Keyword::Not) {
            let operator_span = self.advance().span.clone();
            let expression = self.parse_unary()?;
            let span = Span {
                start: operator_span.start,
                end: expression.span().end,
                line: operator_span.line,
                column: operator_span.column,
            };
            return Ok(Expression::Unary {
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
            return Ok(Expression::Unary {
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
            match self.parse_expression() {
                Ok(argument) => arguments.push(argument),
                Err(error) => {
                    self.report_parse_error(&error);
                    self.synchronize_list_item(Symbol::Comma, Symbol::RightParenthesis);
                    if self.peek_is_symbol(Symbol::RightParenthesis) {
                        break;
                    }
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

    pub(super) fn parse_primary(&mut self) -> ParseResult<Expression> {
        let token = self.advance();
        match token.kind {
            TokenKind::IntegerLiteral(value) => Ok(Expression::IntegerLiteral {
                value,
                span: token.span,
            }),
            TokenKind::Keyword(Keyword::Nil) => Ok(Expression::NilLiteral { span: token.span }),
            TokenKind::StringLiteral(value) => Ok(Expression::StringLiteral {
                value,
                span: token.span,
            }),
            TokenKind::BooleanLiteral(value) => Ok(Expression::BooleanLiteral {
                value,
                span: token.span,
            }),
            TokenKind::Identifier(name) => {
                let starts_with_uppercase = name
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_uppercase());
                if starts_with_uppercase
                    && (self.peek_is_symbol(Symbol::LeftBrace)
                        || self.peek_is_symbol(Symbol::LeftBracket))
                {
                    let mut atom = TypeNameAtom {
                        name,
                        type_arguments: Vec::new(),
                        span: token.span.clone(),
                    };
                    if self.peek_is_symbol(Symbol::LeftBracket) {
                        let (type_arguments, right_bracket) = self.parse_type_argument_list()?;
                        atom.type_arguments = type_arguments;
                        atom.span.end = right_bracket.end;
                    }
                    let type_name = TypeName {
                        names: vec![atom.clone()],
                        span: atom.span,
                    };
                    return self.parse_struct_literal(type_name);
                }
                Ok(Expression::Identifier {
                    name,
                    span: token.span,
                })
            }
            TokenKind::Keyword(Keyword::Match) => self.parse_match_expression(&token.span),
            TokenKind::Symbol(Symbol::LeftParenthesis) => {
                let expression = self.parse_expression()?;
                self.expect_symbol(Symbol::RightParenthesis)?;
                Ok(expression)
            }
            TokenKind::Error(_message) => Err(ParseError::UnparsableToken),
            _ => Err(ParseError::UnexpectedToken {
                message: "expected expression".to_string(),
                span: token.span,
            }),
        }
    }

    pub(super) fn parse_struct_literal(&mut self, type_name: TypeName) -> ParseResult<Expression> {
        let left_brace = self.expect_symbol(Symbol::LeftBrace)?;
        let fields = self.parse_struct_literal_fields();
        let right_brace = self.expect_symbol(Symbol::RightBrace)?;
        let span = Span {
            start: type_name.span.start,
            end: right_brace.end,
            line: left_brace.line,
            column: left_brace.column,
        };
        Ok(Expression::StructLiteral {
            type_name,
            fields,
            span,
        })
    }

    pub(super) fn parse_match_expression(&mut self, start_span: &Span) -> ParseResult<Expression> {
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
        Ok(Expression::Match {
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
            match self.parse_match_arm() {
                Ok(arm) => arms.push(arm),
                Err(error) => {
                    self.report_parse_error(&error);
                    self.synchronize_list_item(Symbol::Comma, Symbol::RightBrace);
                    if self.peek_is_symbol(Symbol::RightBrace) {
                        break;
                    }
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

    pub(super) fn parse_match_arm(&mut self) -> ParseResult<MatchArm> {
        let pattern = self.parse_match_pattern()?;
        self.expect_symbol(Symbol::FatArrow)?;
        let value = self.parse_expression()?;
        let span = Span {
            start: pattern.span().start,
            end: value.span().end,
            line: pattern.span().line,
            column: pattern.span().column,
        };
        Ok(MatchArm {
            pattern,
            value,
            span,
        })
    }

    pub(super) fn parse_match_pattern(&mut self) -> ParseResult<MatchPattern> {
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
            return Ok(MatchPattern::Binding {
                name,
                name_span,
                type_name,
                span,
            });
        }

        let mut qualified_name = name;
        let mut qualified_span = name_span.clone();
        while self.peek_is_symbol(Symbol::Dot) {
            self.advance();
            let (segment, segment_span) = self.expect_identifier()?;
            qualified_name.push('.');
            qualified_name.push_str(&segment);
            qualified_span.end = segment_span.end;
        }

        let type_name = TypeName {
            names: vec![TypeNameAtom {
                name: qualified_name,
                type_arguments: Vec::new(),
                span: qualified_span.clone(),
            }],
            span: qualified_span.clone(),
        };
        Ok(MatchPattern::Type {
            type_name,
            span: qualified_span,
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
            match self.parse_struct_literal_field() {
                Ok(field) => fields.push(field),
                Err(error) => {
                    self.report_parse_error(&error);
                    self.synchronize_list_item(Symbol::Comma, Symbol::RightBrace);
                    if self.peek_is_symbol(Symbol::RightBrace) {
                        break;
                    }
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

    pub(super) fn parse_struct_literal_field(&mut self) -> ParseResult<StructLiteralField> {
        let (name, name_span) = self.expect_identifier()?;
        self.expect_symbol(Symbol::Colon)?;
        let value = self.parse_expression()?;
        let span = Span {
            start: name_span.start,
            end: value.span().end,
            line: name_span.line,
            column: name_span.column,
        };
        Ok(StructLiteralField {
            name,
            name_span,
            value,
            span,
        })
    }
}
