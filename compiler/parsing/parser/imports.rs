use crate::lexer::{Keyword, Symbol};
use compiler__source::Span;
use compiler__syntax::{SyntaxImportDeclaration, SyntaxImportMember};

use super::{ParseResult, Parser};

impl Parser {
    pub(super) fn parse_import_declaration(&mut self) -> ParseResult<SyntaxImportDeclaration> {
        let start = self.expect_keyword(Keyword::Import)?;
        let package_path = self.parse_import_package_path()?;
        self.expect_symbol(Symbol::LeftBrace)?;
        let members = self.parse_import_members();
        let end = self.expect_symbol(Symbol::RightBrace)?;
        Ok(SyntaxImportDeclaration {
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

    fn parse_import_package_path(&mut self) -> ParseResult<String> {
        let (first_segment, _) = self.expect_identifier()?;
        let mut segments = vec![first_segment];
        while self.peek_is_symbol(Symbol::Slash) {
            self.advance();
            let (segment, _) = self.expect_identifier()?;
            segments.push(segment);
        }
        Ok(segments.join("/"))
    }

    fn parse_import_members(&mut self) -> Vec<SyntaxImportMember> {
        let mut members = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightBrace) {
            return members;
        }

        loop {
            self.skip_statement_terminators();
            match self.parse_import_member() {
                Ok(member) => members.push(member),
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

        members
    }

    fn parse_import_member(&mut self) -> ParseResult<SyntaxImportMember> {
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
        Ok(SyntaxImportMember {
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
