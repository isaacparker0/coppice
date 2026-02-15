use crate::lexer::{Keyword, Symbol};
use compiler__source::Span;
use compiler__syntax::{ImportDeclaration, ImportMember};

use super::Parser;

impl Parser {
    pub(super) fn parse_import_declaration(&mut self) -> Option<ImportDeclaration> {
        let start = self.expect_keyword(Keyword::Import)?;
        let package_path = self.parse_import_package_path()?;
        self.expect_symbol(Symbol::LeftBrace)?;
        let members = self.parse_import_members();
        let end = self.expect_symbol(Symbol::RightBrace)?;
        Some(ImportDeclaration {
            package_path,
            members,
            span: Span {
                start: start.start,
                end: end.end,
                line: start.line,
                column: start.column,
            },
        })
    }

    fn parse_import_package_path(&mut self) -> Option<String> {
        let (first_segment, _) = self.expect_identifier()?;
        let mut segments = vec![first_segment];
        while self.peek_is_symbol(Symbol::Slash) {
            self.advance();
            let (segment, _) = self.expect_identifier()?;
            segments.push(segment);
        }
        Some(segments.join("/"))
    }

    fn parse_import_members(&mut self) -> Vec<ImportMember> {
        let mut members = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightBrace) {
            return members;
        }

        loop {
            self.skip_statement_terminators();
            if let Some(member) = self.parse_import_member() {
                members.push(member);
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

    fn parse_import_member(&mut self) -> Option<ImportMember> {
        let (name, name_span) = self.expect_identifier()?;
        let mut alias = None;
        let mut alias_span = None;
        let mut end = name_span.end;
        if self.peek_is_keyword(Keyword::As) {
            self.advance();
            let (alias_name, parsed_alias_span) = self.expect_identifier()?;
            alias = Some(alias_name);
            end = parsed_alias_span.end;
            alias_span = Some(parsed_alias_span);
        }
        Some(ImportMember {
            name,
            alias,
            alias_span,
            span: Span {
                start: name_span.start,
                end,
                line: name_span.line,
                column: name_span.column,
            },
        })
    }
}
