use crate::lexer::{Keyword, Symbol};
use compiler__source::Span;
use compiler__syntax::{
    ConstantDeclaration, FieldDeclaration, FunctionDeclaration, MethodDeclaration,
    ParameterDeclaration, StructMemberItem, TypeDeclaration, TypeDeclarationKind, Visibility,
};

use super::{ExpressionSpan, InvalidConstructKind, ParseError, ParseResult, Parser, RecoveredKind};

impl Parser {
    pub(super) fn parse_type_declaration(
        &mut self,
        visibility: Visibility,
    ) -> ParseResult<TypeDeclaration> {
        self.expect_keyword(Keyword::Type)?;
        let (name, name_span) = self.expect_identifier()?;
        let type_parameters = self.parse_type_parameter_list()?;
        self.expect_symbol(Symbol::DoubleColon)?;
        let start = name_span.clone();
        if self.peek_is_keyword(Keyword::Struct) {
            self.expect_keyword(Keyword::Struct)?;
            self.expect_symbol(Symbol::LeftBrace)?;
            let items = self.parse_struct_members();
            let right_brace = self.expect_symbol(Symbol::RightBrace)?;
            let span = Span {
                start: start.start,
                end: right_brace.end,
                line: start.line,
                column: start.column,
            };
            return Ok(TypeDeclaration {
                name,
                type_parameters,
                kind: TypeDeclarationKind::Struct { items },
                visibility,
                span,
            });
        }
        if self.peek_is_keyword(Keyword::Enum) {
            self.expect_keyword(Keyword::Enum)?;
            let variants = self.parse_enum_type_declaration()?;
            let right_brace = self.expect_symbol(Symbol::RightBrace)?;
            let span = Span {
                start: start.start,
                end: right_brace.end,
                line: start.line,
                column: start.column,
            };
            return Ok(TypeDeclaration {
                name,
                type_parameters,
                kind: TypeDeclarationKind::Enum { variants },
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
        Ok(TypeDeclaration {
            name,
            type_parameters,
            kind: TypeDeclarationKind::Union { variants },
            visibility,
            span,
        })
    }

    pub(super) fn parse_struct_members(&mut self) -> Vec<StructMemberItem> {
        let mut items = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightBrace) {
            return items;
        }
        loop {
            self.skip_statement_terminators();
            if let Some(doc_comment) = self.parse_leading_doc_comment_block() {
                items.push(StructMemberItem::DocComment(doc_comment));
            }
            if self.peek_is_symbol(Symbol::RightBrace) {
                break;
            }
            let visibility = self.parse_visibility();
            if self.peek_is_keyword(Keyword::Function) {
                match self.parse_method_declaration(visibility) {
                    Ok(method) => {
                        items.push(StructMemberItem::Method(Box::new(method.clone())));
                    }
                    Err(error) => {
                        self.report_parse_error(&error);
                        self.synchronize_list_item(Symbol::Comma, Symbol::RightBrace);
                        if self.peek_is_symbol(Symbol::RightBrace) {
                            break;
                        }
                    }
                }
            } else {
                match self.parse_field_declaration(visibility) {
                    Ok(field) => {
                        items.push(StructMemberItem::Field(Box::new(field.clone())));
                    }
                    Err(error) => {
                        self.report_parse_error(&error);
                        self.synchronize_list_item(Symbol::Comma, Symbol::RightBrace);
                        if self.peek_is_symbol(Symbol::RightBrace) {
                            break;
                        }
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
            if self.peek_is_symbol(Symbol::RightBrace) {
                break;
            }
            break;
        }
        items
    }

    pub(super) fn parse_field_declaration(
        &mut self,
        visibility: Visibility,
    ) -> ParseResult<FieldDeclaration> {
        let (name, name_span) = self.expect_identifier()?;
        self.expect_symbol(Symbol::Colon)?;
        let type_name = self.parse_type_name()?;
        let span = Span {
            start: name_span.start,
            end: type_name.span.end,
            line: name_span.line,
            column: name_span.column,
        };
        Ok(FieldDeclaration {
            name,
            type_name,
            visibility,
            span,
        })
    }

    pub(super) fn parse_method_declaration(
        &mut self,
        visibility: Visibility,
    ) -> ParseResult<MethodDeclaration> {
        let start = self.expect_keyword(Keyword::Function)?;
        let (name, name_span) = self.expect_identifier()?;
        self.expect_symbol(Symbol::LeftParenthesis)?;
        let (self_span, self_mutable, parameters) = self.parse_method_parameters()?;
        self.expect_symbol(Symbol::RightParenthesis)?;
        self.expect_symbol(Symbol::Arrow)?;
        let return_type = self.parse_type_name()?;
        let body = self.parse_block()?;
        let body_end = body.span.end;
        Ok(MethodDeclaration {
            name,
            name_span,
            self_span,
            self_mutable,
            parameters,
            return_type,
            body,
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
    ) -> ParseResult<(Span, bool, Vec<ParameterDeclaration>)> {
        let self_mutable = if self.peek_is_keyword(Keyword::Mut) {
            self.advance();
            true
        } else {
            false
        };
        let (receiver_name, receiver_span) = self.expect_identifier()?;
        if receiver_name != "self" {
            return Err(ParseError::InvalidConstruct {
                kind: InvalidConstructKind::FirstMethodParameterMustBeSelf,
                span: receiver_span,
            });
        }
        if self.peek_is_symbol(Symbol::Colon) {
            let span = self.expect_symbol(Symbol::Colon)?;
            self.report_parse_error(&ParseError::Recovered {
                kind: RecoveredKind::MethodReceiverSelfMustNotHaveTypeAnnotation,
                span,
            });
            let _ = self.parse_type_name();
        }
        if !self.peek_is_symbol(Symbol::Comma) {
            return Ok((receiver_span, self_mutable, Vec::new()));
        }

        self.advance();
        let mut parameters = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightParenthesis) {
            return Ok((receiver_span, self_mutable, parameters));
        }
        loop {
            self.skip_statement_terminators();
            match self.parse_parameter() {
                Ok(parameter) => parameters.push(parameter),
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
        Ok((receiver_span, self_mutable, parameters))
    }

    pub(super) fn parse_function(
        &mut self,
        visibility: Visibility,
    ) -> ParseResult<FunctionDeclaration> {
        let start = self.expect_keyword(Keyword::Function)?;
        let (name, name_span) = self.expect_identifier()?;
        let type_parameters = self.parse_type_parameter_list()?;
        self.expect_symbol(Symbol::LeftParenthesis)?;
        let parameters = self.parse_parameters();
        self.expect_symbol(Symbol::RightParenthesis)?;
        self.expect_symbol(Symbol::Arrow)?;
        let return_type = self.parse_type_name()?;
        let body = self.parse_block()?;
        let body_end = body.span.end;
        Ok(FunctionDeclaration {
            name,
            name_span,
            type_parameters,
            parameters,
            return_type,
            body,
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
    ) -> ParseResult<ConstantDeclaration> {
        let (name, name_span) = self.expect_identifier()?;
        if self.peek_is_symbol(Symbol::Assign) {
            let span = self.peek_span();
            return Err(ParseError::InvalidConstruct {
                kind: InvalidConstructKind::ConstantsRequireExplicitTypeAnnotation,
                span,
            });
        }
        self.expect_symbol(Symbol::Colon)?;
        let type_name = self.parse_type_name()?;
        self.expect_symbol(Symbol::Assign)?;
        let expression = self.parse_expression()?;
        let span = Span {
            start: name_span.start,
            end: expression.span().end,
            line: name_span.line,
            column: name_span.column,
        };
        Ok(ConstantDeclaration {
            name,
            type_name,
            expression,
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
            match self.parse_parameter() {
                Ok(parameter) => parameters.push(parameter),
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
        parameters
    }

    pub(super) fn parse_parameter(&mut self) -> ParseResult<ParameterDeclaration> {
        let (name, name_span) = self.expect_identifier()?;
        self.expect_symbol(Symbol::Colon)?;
        let type_name = self.parse_type_name()?;
        let span = Span {
            start: name_span.start,
            end: type_name.span.end,
            line: name_span.line,
            column: name_span.column,
        };
        Ok(ParameterDeclaration {
            name,
            type_name,
            span,
        })
    }
}
