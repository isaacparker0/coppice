use crate::lexer::Symbol;
use compiler__source::Span;
use compiler__syntax::{EnumVariant, TypeName, TypeNameAtom};

use super::Parser;

impl Parser {
    pub(super) fn parse_type_name(&mut self) -> Option<TypeName> {
        let (name, span) = self.expect_type_name_part()?;
        let mut names = vec![TypeNameAtom {
            name,
            span: span.clone(),
        }];
        while self.peek_is_symbol(Symbol::Pipe) {
            self.advance();
            let (name, name_span) = self.expect_type_name_part()?;
            names.push(TypeNameAtom {
                name,
                span: name_span,
            });
        }
        let end = names.last().map_or(span.end, |atom| atom.span.end);
        Some(TypeName {
            names,
            span: Span {
                start: span.start,
                end,
                line: span.line,
                column: span.column,
            },
        })
    }

    pub(super) fn parse_union_type_declaration(&mut self) -> Option<Vec<TypeName>> {
        let mut variants = Vec::new();
        let (name, span) = self.expect_type_name_part()?;
        variants.push(TypeName {
            names: vec![TypeNameAtom {
                name,
                span: span.clone(),
            }],
            span,
        });
        while self.peek_is_symbol(Symbol::Pipe) {
            self.advance();
            let (name, span) = self.expect_type_name_part()?;
            variants.push(TypeName {
                names: vec![TypeNameAtom {
                    name,
                    span: span.clone(),
                }],
                span,
            });
        }
        Some(variants)
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
