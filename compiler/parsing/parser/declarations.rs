use crate::ast::{
    ConstantDeclaration, DocComment, FieldDeclaration, FunctionDeclaration, MethodDeclaration,
    ParameterDeclaration, TypeDeclaration, TypeDeclarationKind, Visibility,
};
use crate::diagnostics::Span;
use crate::lexer::{Keyword, Symbol};

use super::{ExpressionSpan, Parser};

impl Parser {
    pub(super) fn parse_type_declaration(
        &mut self,
        visibility: Visibility,
        doc: Option<DocComment>,
    ) -> Option<TypeDeclaration> {
        self.expect_keyword(Keyword::Type)?;
        let (name, name_span) = self.expect_identifier()?;
        self.expect_symbol(Symbol::DoubleColon)?;
        let start = name_span.clone();
        if self.peek_is_keyword(Keyword::Struct) {
            self.expect_keyword(Keyword::Struct)?;
            self.expect_symbol(Symbol::LeftBrace)?;
            let (fields, methods) = self.parse_struct_members();
            let right_brace = self.expect_symbol(Symbol::RightBrace)?;
            let span = Span {
                start: start.start,
                end: right_brace.end,
                line: start.line,
                column: start.column,
            };
            return Some(TypeDeclaration {
                name,
                kind: TypeDeclarationKind::Struct { fields, methods },
                doc,
                visibility,
                span,
            });
        }
        let variants = self.parse_union_type_declaration()?;
        let end = variants
            .last()
            .map_or(start.end, |variant| variant.span.end);
        let span = Span {
            start: start.start,
            end,
            line: start.line,
            column: start.column,
        };
        Some(TypeDeclaration {
            name,
            kind: TypeDeclarationKind::Union { variants },
            doc,
            visibility,
            span,
        })
    }

    pub(super) fn parse_struct_members(
        &mut self,
    ) -> (Vec<FieldDeclaration>, Vec<MethodDeclaration>) {
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightBrace) {
            return (fields, methods);
        }
        loop {
            self.skip_statement_terminators();
            let mut doc = self.parse_leading_doc_comment_block();
            if self.peek_is_symbol(Symbol::RightBrace) {
                if let Some(doc) = doc {
                    self.error("doc comment must document a declaration", doc.span);
                }
                break;
            }
            if let Some(found_doc) = doc.as_ref()
                && self.peek_span().line != found_doc.end_line + 1
            {
                self.error(
                    "doc comment must document a declaration",
                    found_doc.span.clone(),
                );
                doc = None;
            }
            let visibility = self.parse_visibility();
            if self.peek_is_keyword(Keyword::Function) {
                if let Some(method) = self.parse_method_declaration(visibility, doc.clone()) {
                    methods.push(method);
                } else {
                    self.synchronize_list_item(Symbol::Comma, Symbol::RightBrace);
                    if self.peek_is_symbol(Symbol::RightBrace) {
                        break;
                    }
                }
            } else if let Some(field) = self.parse_field_declaration(visibility, doc.clone()) {
                fields.push(field);
            } else {
                if let Some(doc) = doc {
                    self.error("doc comment must document a declaration", doc.span);
                }
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
            if self.peek_is_symbol(Symbol::RightBrace) {
                break;
            }
            break;
        }
        (fields, methods)
    }

    pub(super) fn parse_field_declaration(
        &mut self,
        visibility: Visibility,
        doc: Option<DocComment>,
    ) -> Option<FieldDeclaration> {
        let (name, name_span) = self.expect_identifier()?;
        self.expect_symbol(Symbol::Colon)?;
        let type_name = self.parse_type_name()?;
        let span = Span {
            start: name_span.start,
            end: type_name.span.end,
            line: name_span.line,
            column: name_span.column,
        };
        Some(FieldDeclaration {
            name,
            type_name,
            doc,
            visibility,
            span,
        })
    }

    pub(super) fn parse_method_declaration(
        &mut self,
        visibility: Visibility,
        doc: Option<DocComment>,
    ) -> Option<MethodDeclaration> {
        let start = self.expect_keyword(Keyword::Function)?;
        let (name, name_span) = self.expect_identifier()?;
        self.expect_symbol(Symbol::LeftParenthesis)?;
        let (self_span, self_mutable, parameters) = self.parse_method_parameters()?;
        self.expect_symbol(Symbol::RightParenthesis)?;
        self.expect_symbol(Symbol::Arrow)?;
        let return_type = self.parse_type_name()?;
        let body = self.parse_block()?;
        let body_end = body.span.end;
        Some(MethodDeclaration {
            name,
            name_span,
            self_span,
            self_mutable,
            parameters,
            return_type,
            body,
            doc,
            visibility,
            span: Span {
                start: start.start,
                end: body_end,
                line: start.line,
                column: start.column,
            },
        })
    }

    pub(super) fn parse_method_parameters(
        &mut self,
    ) -> Option<(Span, bool, Vec<ParameterDeclaration>)> {
        let self_mutable = if self.peek_is_keyword(Keyword::Mut) {
            self.advance();
            true
        } else {
            false
        };
        let (receiver_name, receiver_span) = self.expect_identifier()?;
        if receiver_name != "self" {
            self.error("first method parameter must be 'self'", receiver_span);
            return None;
        }
        if self.peek_is_symbol(Symbol::Colon) {
            let span = self.expect_symbol(Symbol::Colon)?;
            self.error(
                "method receiver 'self' must not have a type annotation",
                span,
            );
            let _ = self.parse_type_name();
        }
        if !self.peek_is_symbol(Symbol::Comma) {
            return Some((receiver_span, self_mutable, Vec::new()));
        }

        self.advance();
        let mut parameters = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightParenthesis) {
            return Some((receiver_span, self_mutable, parameters));
        }
        loop {
            self.skip_statement_terminators();
            if let Some(parameter) = self.parse_parameter() {
                parameters.push(parameter);
            } else {
                self.synchronize_list_item(Symbol::Comma, Symbol::RightParenthesis);
                if self.peek_is_symbol(Symbol::RightParenthesis) {
                    break;
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
        Some((receiver_span, self_mutable, parameters))
    }

    pub(super) fn parse_function(
        &mut self,
        visibility: Visibility,
        doc: Option<DocComment>,
    ) -> Option<FunctionDeclaration> {
        let start = self.expect_keyword(Keyword::Function)?;
        let (name, name_span) = self.expect_identifier()?;
        self.expect_symbol(Symbol::LeftParenthesis)?;
        let parameters = self.parse_parameters();
        self.expect_symbol(Symbol::RightParenthesis)?;
        self.expect_symbol(Symbol::Arrow)?;
        let return_type = self.parse_type_name()?;
        let body = self.parse_block()?;
        let body_end = body.span.end;
        Some(FunctionDeclaration {
            name,
            name_span,
            parameters,
            return_type,
            body,
            doc,
            visibility,
            span: Span {
                start: start.start,
                end: body_end,
                line: start.line,
                column: start.column,
            },
        })
    }

    pub(super) fn parse_constant_declaration(
        &mut self,
        visibility: Visibility,
    ) -> Option<ConstantDeclaration> {
        let (name, name_span) = self.expect_identifier()?;
        self.expect_symbol(Symbol::Assign)?;
        let expression = self.parse_expression()?;
        let span = Span {
            start: name_span.start,
            end: expression.span().end,
            line: name_span.line,
            column: name_span.column,
        };
        Some(ConstantDeclaration {
            name,
            expression,
            doc: None,
            visibility,
            span,
        })
    }

    pub(super) fn parse_parameters(&mut self) -> Vec<ParameterDeclaration> {
        let mut parameters = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightParenthesis) {
            return parameters;
        }
        loop {
            self.skip_statement_terminators();
            if let Some(parameter) = self.parse_parameter() {
                parameters.push(parameter);
            } else {
                self.synchronize_list_item(Symbol::Comma, Symbol::RightParenthesis);
                if self.peek_is_symbol(Symbol::RightParenthesis) {
                    break;
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
        parameters
    }

    pub(super) fn parse_parameter(&mut self) -> Option<ParameterDeclaration> {
        let (name, name_span) = self.expect_identifier()?;
        self.expect_symbol(Symbol::Colon)?;
        let type_name = self.parse_type_name()?;
        let span = Span {
            start: name_span.start,
            end: type_name.span.end,
            line: name_span.line,
            column: name_span.column,
        };
        Some(ParameterDeclaration {
            name,
            type_name,
            span,
        })
    }
}
