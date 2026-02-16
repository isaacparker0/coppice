use crate::lexer::Symbol;
use compiler__source::Span;
use compiler__syntax::{EnumVariant, TypeName, TypeNameAtom, TypeParameter};

use super::{ParseError, ParseResult, Parser, RecoveredKind};

impl Parser {
    pub(super) fn parse_type_name(&mut self) -> ParseResult<TypeName> {
        let first = self.parse_type_name_atom()?;
        let mut names = vec![first];
        while self.peek_is_symbol(Symbol::Pipe) {
            self.advance();
            let next = self.parse_type_name_atom()?;
            names.push(next);
        }
        let first_span = names[0].span.clone();
        let end = names.last().map_or(first_span.end, |atom| atom.span.end);
        Ok(TypeName {
            names,
            span: Span {
                start: first_span.start,
                end,
                line: first_span.line,
                column: first_span.column,
            },
        })
    }

    pub(super) fn parse_union_type_declaration(&mut self) -> ParseResult<Vec<TypeName>> {
        let mut variants = Vec::new();
        let first_atom = self.parse_type_name_atom()?;
        variants.push(TypeName {
            names: vec![first_atom.clone()],
            span: first_atom.span.clone(),
        });
        while self.peek_is_symbol(Symbol::Pipe) {
            self.advance();
            let atom = self.parse_type_name_atom()?;
            variants.push(TypeName {
                names: vec![atom.clone()],
                span: atom.span.clone(),
            });
        }
        Ok(variants)
    }

    pub(super) fn parse_type_name_atom(&mut self) -> ParseResult<TypeNameAtom> {
        let (name, mut span) = self.expect_type_name_part()?;
        let mut type_arguments = Vec::new();
        if self.peek_is_symbol(Symbol::LeftBracket) {
            let (arguments, right_bracket) = self.parse_type_argument_list()?;
            type_arguments = arguments;
            span.end = right_bracket.end;
        }
        Ok(TypeNameAtom {
            name,
            type_arguments,
            span,
        })
    }

    pub(super) fn parse_type_argument_list(&mut self) -> ParseResult<(Vec<TypeName>, Span)> {
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
    ) -> ParseResult<(Vec<TypeParameter>, Vec<ParseError>)> {
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
            type_parameters.push(TypeParameter { name, span });
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
    ) -> ParseResult<(Vec<EnumVariant>, Vec<ParseError>)> {
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
            variants.push(EnumVariant {
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
