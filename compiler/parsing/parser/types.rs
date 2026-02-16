use crate::lexer::Symbol;
use compiler__source::Span;
use compiler__syntax::{EnumVariant, TypeName, TypeNameAtom, TypeParameter};

use super::Parser;

impl Parser {
    pub(super) fn parse_type_name(&mut self) -> Option<TypeName> {
        let first = self.parse_type_name_atom()?;
        let mut names = vec![first];
        while self.peek_is_symbol(Symbol::Pipe) {
            self.advance();
            let next = self.parse_type_name_atom()?;
            names.push(next);
        }
        let first_span = names[0].span.clone();
        let end = names.last().map_or(first_span.end, |atom| atom.span.end);
        Some(TypeName {
            names,
            span: Span {
                start: first_span.start,
                end,
                line: first_span.line,
                column: first_span.column,
            },
        })
    }

    pub(super) fn parse_union_type_declaration(&mut self) -> Option<Vec<TypeName>> {
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
        Some(variants)
    }

    pub(super) fn parse_type_name_atom(&mut self) -> Option<TypeNameAtom> {
        let (name, mut span) = self.expect_type_name_part()?;
        let mut type_arguments = Vec::new();
        if self.peek_is_symbol(Symbol::LeftBracket) {
            let (arguments, right_bracket) = self.parse_type_argument_list()?;
            type_arguments = arguments;
            span.end = right_bracket.end;
        }
        Some(TypeNameAtom {
            name,
            type_arguments,
            span,
        })
    }

    pub(super) fn parse_type_argument_list(&mut self) -> Option<(Vec<TypeName>, Span)> {
        self.expect_symbol(Symbol::LeftBracket)?;
        let mut arguments = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightBracket) {
            let right_bracket = self.expect_symbol(Symbol::RightBracket)?;
            return Some((arguments, right_bracket));
        }
        loop {
            self.skip_statement_terminators();
            if let Some(type_argument) = self.parse_type_name() {
                arguments.push(type_argument);
            } else {
                self.synchronize_list_item(Symbol::Comma, Symbol::RightBracket);
                if self.peek_is_symbol(Symbol::RightBracket) {
                    let right_bracket = self.expect_symbol(Symbol::RightBracket)?;
                    return Some((arguments, right_bracket));
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
        Some((arguments, right_bracket))
    }

    pub(super) fn parse_type_parameter_list(&mut self) -> Option<Vec<TypeParameter>> {
        if !self.peek_is_symbol(Symbol::LeftBracket) {
            return Some(Vec::new());
        }
        self.expect_symbol(Symbol::LeftBracket)?;
        let mut type_parameters = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightBracket) {
            self.error("type parameter list must not be empty", self.peek_span());
            self.expect_symbol(Symbol::RightBracket)?;
            return Some(type_parameters);
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
        Some(type_parameters)
    }

    pub(super) fn parse_enum_type_declaration(&mut self) -> Option<Vec<EnumVariant>> {
        self.expect_symbol(Symbol::LeftBrace)?;
        let mut variants = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightBrace) {
            self.error(
                "enum declaration must include at least one variant",
                self.peek_span(),
            );
            return Some(variants);
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
            self.error("expected ',' or '}' after enum variant", self.peek_span());
            self.synchronize_list_item(Symbol::Comma, Symbol::RightBrace);
            if self.peek_is_symbol(Symbol::Comma) {
                self.advance();
                continue;
            }
            if self.peek_is_symbol(Symbol::RightBrace) {
                break;
            }
        }

        Some(variants)
    }
}
