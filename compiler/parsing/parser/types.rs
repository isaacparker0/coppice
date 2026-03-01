use crate::lexer::Symbol;
use compiler__source::Span;
use compiler__syntax::{
    SyntaxEnumVariant, SyntaxTypeName, SyntaxTypeNameSegment, SyntaxTypeParameter,
};

use super::{ParseError, ParseResult, Parser, RecoveredKind};

impl Parser {
    pub(super) fn parse_type_name(&mut self) -> ParseResult<SyntaxTypeName> {
        let first = self.parse_type_name_member()?;
        let mut names = vec![first];
        while self.peek_is_symbol(Symbol::Pipe) {
            self.advance();
            let next = self.parse_type_name_member()?;
            names.push(next);
        }
        let first_span = names[0].span.clone();
        let end = names
            .last()
            .map_or(first_span.end, |segment| segment.span.end);
        Ok(SyntaxTypeName {
            names,
            span: Span {
                start: first_span.start,
                end,
                line: first_span.line,
                column: first_span.column,
            },
        })
    }

    pub(super) fn parse_union_type_declaration(&mut self) -> ParseResult<Vec<SyntaxTypeName>> {
        let mut variants = Vec::new();
        let first_segment = self.parse_type_name_member()?;
        let first_segment_span = first_segment.span.clone();
        variants.push(SyntaxTypeName {
            names: vec![first_segment],
            span: first_segment_span,
        });
        while self.peek_is_symbol(Symbol::Pipe) {
            self.advance();
            let segment = self.parse_type_name_member()?;
            variants.push(SyntaxTypeName {
                names: vec![segment.clone()],
                span: segment.span.clone(),
            });
        }
        Ok(variants)
    }

    pub(super) fn parse_type_name_member(&mut self) -> ParseResult<SyntaxTypeNameSegment> {
        if self.peek_is_keyword(crate::lexer::Keyword::Function) {
            return self.parse_function_type_name_segment();
        }
        self.parse_type_name_segment()
    }

    pub(super) fn parse_type_name_segment(&mut self) -> ParseResult<SyntaxTypeNameSegment> {
        let (name, mut span) = self.expect_type_name_part()?;
        let mut type_arguments = Vec::new();
        if self.peek_is_symbol(Symbol::LeftBracket) {
            let (arguments, right_bracket) = self.parse_type_argument_list()?;
            type_arguments = arguments;
            span.end = right_bracket.end;
        }
        Ok(SyntaxTypeNameSegment {
            name,
            type_arguments,
            span,
        })
    }

    pub(super) fn parse_function_type_name_segment(
        &mut self,
    ) -> ParseResult<SyntaxTypeNameSegment> {
        let function_span = self.expect_keyword(crate::lexer::Keyword::Function)?;
        self.expect_symbol(Symbol::LeftParenthesis)?;
        let mut parameter_type_names = Vec::new();
        self.skip_statement_terminators();
        if !self.peek_is_symbol(Symbol::RightParenthesis) {
            loop {
                self.skip_statement_terminators();
                match self.parse_type_name() {
                    Ok(parameter_type_name) => parameter_type_names.push(parameter_type_name),
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
        }
        self.expect_symbol(Symbol::RightParenthesis)?;
        self.expect_symbol(Symbol::Arrow)?;
        let return_type_name = self.parse_type_name()?;
        let mut function_type_arguments = parameter_type_names;
        function_type_arguments.push(return_type_name.clone());
        Ok(SyntaxTypeNameSegment {
            name: "function".to_string(),
            type_arguments: function_type_arguments,
            span: Span {
                start: function_span.start,
                end: return_type_name.span.end,
                line: function_span.line,
                column: function_span.column,
            },
        })
    }

    pub(super) fn parse_type_argument_list(&mut self) -> ParseResult<(Vec<SyntaxTypeName>, Span)> {
        self.expect_symbol(Symbol::LeftBracket)?;
        let mut arguments = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightBracket) {
            let right_bracket = self.expect_symbol(Symbol::RightBracket)?;
            return Ok((arguments, right_bracket));
        }
        loop {
            self.skip_statement_terminators();
            match self.parse_type_name() {
                Ok(type_argument) => arguments.push(type_argument),
                Err(error) => {
                    self.report_parse_error(&error);
                    self.synchronize_list_item(Symbol::Comma, Symbol::RightBracket);
                    if self.peek_is_symbol(Symbol::RightBracket) {
                        let right_bracket = self.expect_symbol(Symbol::RightBracket)?;
                        return Ok((arguments, right_bracket));
                    }
                }
            }
            self.skip_statement_terminators();
            if self.peek_is_symbol(Symbol::Comma) {
                self.advance();
                self.skip_statement_terminators();
                if self.peek_is_symbol(Symbol::RightBracket) {
                    break;
                }
                continue;
            }
            break;
        }
        let right_bracket = self.expect_symbol(Symbol::RightBracket)?;
        Ok((arguments, right_bracket))
    }

    pub(super) fn parse_type_parameter_list(
        &mut self,
    ) -> ParseResult<(Vec<SyntaxTypeParameter>, Vec<ParseError>)> {
        if !self.peek_is_symbol(Symbol::LeftBracket) {
            return Ok((Vec::new(), Vec::new()));
        }
        self.expect_symbol(Symbol::LeftBracket)?;
        let mut type_parameters = Vec::new();
        let mut recoveries = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightBracket) {
            recoveries.push(ParseError::Recovered {
                kind: RecoveredKind::TypeParameterListMustNotBeEmpty,
                span: self.peek_span(),
            });
            self.expect_symbol(Symbol::RightBracket)?;
            return Ok((type_parameters, recoveries));
        }
        loop {
            self.skip_statement_terminators();
            let (name, span) = self.expect_identifier()?;
            let constraint = if self.peek_is_symbol(Symbol::Colon) {
                self.advance();
                Some(self.parse_type_name()?)
            } else {
                None
            };
            type_parameters.push(SyntaxTypeParameter {
                name,
                constraint,
                span,
            });
            self.skip_statement_terminators();
            if self.peek_is_symbol(Symbol::Comma) {
                self.advance();
                self.skip_statement_terminators();
                if self.peek_is_symbol(Symbol::RightBracket) {
                    break;
                }
                continue;
            }
            break;
        }
        self.expect_symbol(Symbol::RightBracket)?;
        Ok((type_parameters, recoveries))
    }

    pub(super) fn parse_enum_type_declaration(
        &mut self,
    ) -> ParseResult<(Vec<SyntaxEnumVariant>, Vec<ParseError>)> {
        self.expect_symbol(Symbol::LeftBrace)?;
        let mut variants = Vec::new();
        let mut recoveries = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightBrace) {
            recoveries.push(ParseError::Recovered {
                kind: RecoveredKind::EnumDeclarationMustIncludeAtLeastOneVariant,
                span: self.peek_span(),
            });
            return Ok((variants, recoveries));
        }

        loop {
            self.skip_statement_terminators();
            let (name, span) = self.expect_identifier()?;
            variants.push(SyntaxEnumVariant {
                name,
                span: span.clone(),
            });

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
            recoveries.push(ParseError::Recovered {
                kind: RecoveredKind::ExpectedCommaOrRightBraceAfterEnumVariant,
                span: self.peek_span(),
            });
            self.synchronize_list_item(Symbol::Comma, Symbol::RightBrace);
            if self.peek_is_symbol(Symbol::Comma) {
                self.advance();
                continue;
            }
            if self.peek_is_symbol(Symbol::RightBrace) {
                break;
            }
        }

        Ok((variants, recoveries))
    }
}
