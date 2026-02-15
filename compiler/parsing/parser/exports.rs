use crate::lexer::{Keyword, Symbol};
use compiler__source::Span;
use compiler__syntax::{ExportDeclaration, ExportMember};

use super::Parser;

impl Parser {
    pub(super) fn parse_export_declaration(&mut self) -> Option<ExportDeclaration> {
        let start = self.expect_keyword(Keyword::Export)?;
        self.expect_symbol(Symbol::LeftBrace)?;
        let members = self.parse_export_members();
        let end = self.expect_symbol(Symbol::RightBrace)?;
        Some(ExportDeclaration {
            members,
            span: Span {
                start: start.start,
                end: end.end,
                line: start.line,
                column: start.column,
            },
        })
    }

    fn parse_export_members(&mut self) -> Vec<ExportMember> {
        let mut members = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightBrace) {
            return members;
        }

        loop {
            self.skip_statement_terminators();
            if let Some((name, span)) = self.expect_identifier() {
                members.push(ExportMember { name, span });
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

        members
    }
}
