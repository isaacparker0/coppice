use crate::lexer::Symbol;
use compiler__syntax::Span;
use compiler__syntax::{TypeName, TypeNameAtom};

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
}
