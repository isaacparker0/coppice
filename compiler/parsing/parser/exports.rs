use crate::lexer::{Keyword, Symbol};
use compiler__source::Span;
use compiler__syntax::{ExportsDeclaration, ExportsMember};

use super::Parser;

impl Parser {
    pub(super) fn parse_exports_declaration(&mut self) -> Option<ExportsDeclaration> {
        let start = self.expect_keyword(Keyword::Exports)?;
        self.expect_symbol(Symbol::LeftBrace)?;
        let members = self.parse_exports_members();
        let end = self.expect_symbol(Symbol::RightBrace)?;
        Some(ExportsDeclaration {
            members,
            span: Span {
                start: start.start,
                end: end.end,
                line: start.line,
                column: start.column,
            },
        })
    }

    fn parse_exports_members(&mut self) -> Vec<ExportsMember> {
        let mut members = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightBrace) {
            return members;
        }

        loop {
            self.skip_statement_terminators();
            if let Some((name, span)) = self.expect_identifier() {
                members.push(ExportsMember { name, span });
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
