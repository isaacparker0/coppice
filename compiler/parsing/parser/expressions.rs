use crate::lexer::{Keyword, Symbol, TokenKind};
use compiler__source::Span;
use compiler__syntax::{
    SyntaxBinaryOperator, SyntaxExpression, SyntaxMatchArm, SyntaxMatchPattern,
    SyntaxNameReferenceKind, SyntaxStructLiteralField, SyntaxTypeName, SyntaxTypeNameSegment,
    SyntaxUnaryOperator,
};

use super::{
    ExpressionSpan, InvalidConstructKind, ParseError, ParseResult, Parser, RecoveredKind,
    UnexpectedTokenKind,
};

impl Parser {
    pub(super) fn parse_expression(&mut self) -> ParseResult<SyntaxExpression> {
        let result = self.parse_or();
        self.flush_deferred_parse_errors();
        result
    }

    pub(super) fn parse_or(&mut self) -> ParseResult<SyntaxExpression> {
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
            expression = SyntaxExpression::Binary {
                operator: SyntaxBinaryOperator::Or,
                left: Box::new(expression),
                right: Box::new(right),
                span,
            };
        }
        Ok(expression)
    }

    pub(super) fn parse_and(&mut self) -> ParseResult<SyntaxExpression> {
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
            expression = SyntaxExpression::Binary {
                operator: SyntaxBinaryOperator::And,
                left: Box::new(expression),
                right: Box::new(right),
                span,
            };
        }
        Ok(expression)
    }

    pub(super) fn parse_equality(&mut self) -> ParseResult<SyntaxExpression> {
        let mut expression = self.parse_comparison()?;
        loop {
            if self.peek_is_symbol(Symbol::Equal) {
                let operator_span = self.advance().span.clone();
                self.defer_parse_error(ParseError::Recovered {
                    kind: RecoveredKind::UnexpectedEqualsInExpression,
                    span: operator_span,
                });
                let _ = self.parse_comparison();
                continue;
            }
            let operator = if self.peek_is_symbol(Symbol::EqualEqual) {
                SyntaxBinaryOperator::EqualEqual
            } else if self.peek_is_symbol(Symbol::BangEqual) {
                SyntaxBinaryOperator::NotEqual
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
            expression = SyntaxExpression::Binary {
                operator,
                left: Box::new(expression),
                right: Box::new(right),
                span,
            };
        }
        Ok(expression)
    }

    pub(super) fn parse_comparison(&mut self) -> ParseResult<SyntaxExpression> {
        let mut expression = self.parse_additive()?;
        loop {
            if self.peek_is_keyword(Keyword::Matches) {
                let operator_span = self.advance().span.clone();
                let type_name = self.parse_type_name()?;
                if type_name
                    .names
                    .iter()
                    .any(|segment| !segment.type_arguments.is_empty())
                {
                    return Err(ParseError::InvalidConstruct {
                        kind: InvalidConstructKind::PatternTypeArgumentsNotSupported,
                        span: type_name.span.clone(),
                    });
                }
                let span = Span {
                    start: expression.span().start,
                    end: type_name.span.end,
                    line: operator_span.line,
                    column: operator_span.column,
                };
                expression = SyntaxExpression::Matches {
                    value: Box::new(expression),
                    type_name,
                    span,
                };
                continue;
            }
            let operator = if self.peek_is_symbol(Symbol::Less) {
                SyntaxBinaryOperator::LessThan
            } else if self.peek_is_symbol(Symbol::LessEqual) {
                SyntaxBinaryOperator::LessThanOrEqual
            } else if self.peek_is_symbol(Symbol::Greater) {
                SyntaxBinaryOperator::GreaterThan
            } else if self.peek_is_symbol(Symbol::GreaterEqual) {
                SyntaxBinaryOperator::GreaterThanOrEqual
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
            expression = SyntaxExpression::Binary {
                operator,
                left: Box::new(expression),
                right: Box::new(right),
                span,
            };
        }
        Ok(expression)
    }

    pub(super) fn parse_additive(&mut self) -> ParseResult<SyntaxExpression> {
        let mut expression = self.parse_multiplicative()?;
        loop {
            let operator = if self.peek_is_symbol(Symbol::Plus) {
                SyntaxBinaryOperator::Add
            } else if self.peek_is_symbol(Symbol::Minus) {
                SyntaxBinaryOperator::Subtract
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
            expression = SyntaxExpression::Binary {
                operator,
                left: Box::new(expression),
                right: Box::new(right),
                span,
            };
        }
        Ok(expression)
    }

    pub(super) fn parse_multiplicative(&mut self) -> ParseResult<SyntaxExpression> {
        let mut expression = self.parse_unary()?;
        loop {
            let operator = if self.peek_is_symbol(Symbol::Star) {
                SyntaxBinaryOperator::Multiply
            } else if self.peek_is_symbol(Symbol::Slash) {
                SyntaxBinaryOperator::Divide
            } else if self.peek_is_symbol(Symbol::Percent) {
                SyntaxBinaryOperator::Modulo
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
            expression = SyntaxExpression::Binary {
                operator,
                left: Box::new(expression),
                right: Box::new(right),
                span,
            };
        }
        Ok(expression)
    }

    pub(super) fn parse_postfix(&mut self) -> ParseResult<SyntaxExpression> {
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
                expression = SyntaxExpression::Call {
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
                        kind: InvalidConstructKind::TypeArgumentsMustBeFollowedByCall,
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
                expression = SyntaxExpression::Call {
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
                expression = SyntaxExpression::FieldAccess {
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

    pub(super) fn parse_unary(&mut self) -> ParseResult<SyntaxExpression> {
        if self.peek_is_keyword(Keyword::Not) {
            let operator_span = self.advance().span.clone();
            let expression = self.parse_unary()?;
            let span = Span {
                start: operator_span.start,
                end: expression.span().end,
                line: operator_span.line,
                column: operator_span.column,
            };
            return Ok(SyntaxExpression::Unary {
                operator: SyntaxUnaryOperator::Not,
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
            return Ok(SyntaxExpression::Unary {
                operator: SyntaxUnaryOperator::Negate,
                expression: Box::new(expression),
                span,
            });
        }
        self.parse_postfix()
    }

    pub(super) fn parse_arguments(&mut self) -> Vec<SyntaxExpression> {
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

    pub(super) fn parse_primary(&mut self) -> ParseResult<SyntaxExpression> {
        let token = self.advance();
        match token.kind {
            TokenKind::IntegerLiteral(value) => Ok(SyntaxExpression::IntegerLiteral {
                value,
                span: token.span,
            }),
            TokenKind::Keyword(Keyword::Nil) => {
                Ok(SyntaxExpression::NilLiteral { span: token.span })
            }
            TokenKind::StringLiteral(value) => Ok(SyntaxExpression::StringLiteral {
                value,
                span: token.span,
            }),
            TokenKind::BooleanLiteral(value) => Ok(SyntaxExpression::BooleanLiteral {
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
                    let mut segment = SyntaxTypeNameSegment {
                        name,
                        type_arguments: Vec::new(),
                        span: token.span.clone(),
                    };
                    if self.peek_is_symbol(Symbol::LeftBracket) {
                        let (type_arguments, right_bracket) = self.parse_type_argument_list()?;
                        segment.type_arguments = type_arguments;
                        segment.span.end = right_bracket.end;
                    }
                    let type_name = SyntaxTypeName {
                        names: vec![segment.clone()],
                        span: segment.span,
                    };
                    return self.parse_struct_literal(type_name);
                }
                Ok(SyntaxExpression::NameReference {
                    name,
                    kind: SyntaxNameReferenceKind::UserDefined,
                    span: token.span,
                })
            }
            TokenKind::Keyword(Keyword::Abort) => Ok(SyntaxExpression::NameReference {
                name: "abort".to_string(),
                kind: SyntaxNameReferenceKind::Builtin,
                span: token.span,
            }),
            TokenKind::Keyword(Keyword::Print) => Ok(SyntaxExpression::NameReference {
                name: "print".to_string(),
                kind: SyntaxNameReferenceKind::Builtin,
                span: token.span,
            }),
            TokenKind::Keyword(Keyword::Match) => self.parse_match_expression(&token.span),
            TokenKind::Symbol(Symbol::LeftParenthesis) => {
                let expression = self.parse_expression()?;
                self.expect_symbol(Symbol::RightParenthesis)?;
                Ok(expression)
            }
            TokenKind::Error => Err(ParseError::UnparsableToken),
            _ => Err(ParseError::UnexpectedToken {
                kind: UnexpectedTokenKind::ExpectedExpression,
                span: token.span,
            }),
        }
    }

    pub(super) fn parse_struct_literal(
        &mut self,
        type_name: SyntaxTypeName,
    ) -> ParseResult<SyntaxExpression> {
        let left_brace = self.expect_symbol(Symbol::LeftBrace)?;
        let fields = self.parse_struct_literal_fields();
        let right_brace = self.expect_symbol(Symbol::RightBrace)?;
        let span = Span {
            start: type_name.span.start,
            end: right_brace.end,
            line: left_brace.line,
            column: left_brace.column,
        };
        Ok(SyntaxExpression::StructLiteral {
            type_name,
            fields,
            span,
        })
    }

    pub(super) fn parse_match_expression(
        &mut self,
        start_span: &Span,
    ) -> ParseResult<SyntaxExpression> {
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
        Ok(SyntaxExpression::Match {
            target: Box::new(target),
            arms,
            span,
        })
    }

    pub(super) fn parse_match_arms(&mut self) -> Vec<SyntaxMatchArm> {
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
            if self.peek_is_identifier() || self.peek_is_keyword(Keyword::Nil) {
                continue;
            }
            break;
        }
        arms
    }

    pub(super) fn parse_match_arm(&mut self) -> ParseResult<SyntaxMatchArm> {
        let pattern = self.parse_match_pattern()?;
        self.expect_symbol(Symbol::FatArrow)?;
        let value = self.parse_expression()?;
        let span = Span {
            start: pattern.span().start,
            end: value.span().end,
            line: pattern.span().line,
            column: pattern.span().column,
        };
        Ok(SyntaxMatchArm {
            pattern,
            value,
            span,
        })
    }

    pub(super) fn parse_match_pattern(&mut self) -> ParseResult<SyntaxMatchPattern> {
        let (name, name_span, can_be_binding_name) = if self.peek_is_keyword(Keyword::Nil) {
            let (name, span) = self.expect_type_name_part()?;
            (name, span, false)
        } else {
            let (name, span) = self.expect_identifier()?;
            (name, span, true)
        };
        if can_be_binding_name && self.peek_is_symbol(Symbol::Colon) {
            self.advance();
            let type_name = self.parse_type_name()?;
            if type_name
                .names
                .iter()
                .any(|segment| !segment.type_arguments.is_empty())
            {
                return Err(ParseError::InvalidConstruct {
                    kind: InvalidConstructKind::PatternTypeArgumentsNotSupported,
                    span: type_name.span.clone(),
                });
            }
            let span = Span {
                start: name_span.start,
                end: type_name.span.end,
                line: name_span.line,
                column: name_span.column,
            };
            return Ok(SyntaxMatchPattern::Binding {
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
        if self.peek_is_symbol(Symbol::LeftBracket) {
            let (_, right_bracket) = self.parse_type_argument_list()?;
            return Err(ParseError::InvalidConstruct {
                kind: InvalidConstructKind::PatternTypeArgumentsNotSupported,
                span: Span {
                    start: name_span.start,
                    end: right_bracket.end,
                    line: name_span.line,
                    column: name_span.column,
                },
            });
        }

        let type_name = SyntaxTypeName {
            names: vec![SyntaxTypeNameSegment {
                name: qualified_name,
                type_arguments: Vec::new(),
                span: qualified_span.clone(),
            }],
            span: qualified_span.clone(),
        };
        Ok(SyntaxMatchPattern::Type {
            type_name,
            span: qualified_span,
        })
    }

    pub(super) fn parse_struct_literal_fields(&mut self) -> Vec<SyntaxStructLiteralField> {
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

    pub(super) fn parse_struct_literal_field(&mut self) -> ParseResult<SyntaxStructLiteralField> {
        let (name, name_span) = self.expect_identifier()?;
        self.expect_symbol(Symbol::Colon)?;
        let value = self.parse_expression()?;
        let span = Span {
            start: name_span.start,
            end: value.span().end,
            line: name_span.line,
            column: name_span.column,
        };
        Ok(SyntaxStructLiteralField {
            name,
            name_span,
            value,
            span,
        })
    }
}
