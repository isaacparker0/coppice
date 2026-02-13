use crate::ast::{
    BinaryOperator, Block, ConstantDeclaration, DocComment, Expression, FieldDeclaration, File,
    FunctionDeclaration, MatchArm, MatchPattern, MethodDeclaration, ParameterDeclaration,
    Statement, StructLiteralField, TypeDeclaration, TypeDeclarationKind, TypeName, TypeNameAtom,
    UnaryOperator, Visibility,
};
use crate::diagnostics::{Diagnostic, Span};
use crate::lexer::{Keyword, Symbol, Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    position: usize,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            position: 0,
            diagnostics: Vec::new(),
        }
    }

    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }

    pub fn parse_file(&mut self) -> File {
        let mut types = Vec::new();
        let mut constants = Vec::new();
        let mut functions = Vec::new();
        while !self.at_eof() {
            self.skip_statement_terminators();
            let mut doc = self.parse_leading_doc_comment_block();
            if self.at_eof() {
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
            if self.peek_is_keyword(Keyword::Public) {
                let visibility = self.parse_visibility();
                if self.peek_is_keyword(Keyword::Type) {
                    if let Some(type_declaration) = self.parse_type_declaration(visibility, doc) {
                        types.push(type_declaration);
                    } else {
                        self.synchronize();
                    }
                } else if self.peek_is_keyword(Keyword::Function) {
                    if let Some(function_declaration) = self.parse_function(visibility, doc) {
                        functions.push(function_declaration);
                    } else {
                        self.synchronize();
                    }
                } else if self.peek_is_identifier() {
                    if let Some(constant_declaration) = self.parse_constant_declaration(visibility)
                    {
                        let constant_declaration = ConstantDeclaration {
                            doc,
                            ..constant_declaration
                        };
                        constants.push(constant_declaration);
                    } else {
                        self.synchronize();
                    }
                } else {
                    if let Some(doc) = doc {
                        self.error("doc comment must document a declaration", doc.span);
                    }
                    let span = self.peek_span();
                    self.error("expected declaration after 'public'", span);
                    self.synchronize();
                }
            } else if self.peek_is_keyword(Keyword::Type) {
                if let Some(type_declaration) =
                    self.parse_type_declaration(Visibility::Private, doc)
                {
                    types.push(type_declaration);
                } else {
                    self.synchronize();
                }
            } else if self.peek_is_keyword(Keyword::Function) {
                if let Some(function_declaration) = self.parse_function(Visibility::Private, doc) {
                    functions.push(function_declaration);
                } else {
                    self.synchronize();
                }
            } else if self.peek_is_identifier() && self.peek_second_is_symbol(Symbol::DoubleColon) {
                if let Some(doc) = doc {
                    self.error("doc comment must document a declaration", doc.span);
                }
                let span = self.peek_span();
                self.error("expected keyword 'type' before type declaration", span);
                self.advance();
                self.synchronize();
            } else if self.peek_is_identifier() {
                if let Some(constant_declaration) =
                    self.parse_constant_declaration(Visibility::Private)
                {
                    let constant_declaration = ConstantDeclaration {
                        doc,
                        ..constant_declaration
                    };
                    constants.push(constant_declaration);
                } else {
                    self.synchronize();
                }
            } else {
                if let Some(doc) = doc {
                    self.error("doc comment must document a declaration", doc.span);
                }
                let span = self.peek_span();
                self.error("expected declaration", span);
                self.synchronize();
            }
        }
        File {
            types,
            constants,
            functions,
        }
    }

    fn parse_type_declaration(
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

    fn parse_struct_members(&mut self) -> (Vec<FieldDeclaration>, Vec<MethodDeclaration>) {
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

    fn parse_field_declaration(
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

    fn parse_method_declaration(
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

    fn parse_method_parameters(&mut self) -> Option<(Span, bool, Vec<ParameterDeclaration>)> {
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

    fn parse_function(
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

    fn parse_constant_declaration(
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

    fn parse_parameters(&mut self) -> Vec<ParameterDeclaration> {
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

    fn parse_parameter(&mut self) -> Option<ParameterDeclaration> {
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

    fn parse_block(&mut self) -> Option<Block> {
        let start = self.expect_symbol(Symbol::LeftBrace)?;
        let mut statements = Vec::new();
        self.skip_statement_terminators();
        while !self.peek_is_symbol(Symbol::RightBrace) && !self.at_eof() {
            if let Some(statement) = self.parse_statement() {
                statements.push(statement);
            } else {
                self.synchronize_statement();
            }
            self.skip_statement_terminators();
        }
        let end = self.expect_symbol(Symbol::RightBrace)?;
        Some(Block {
            statements,
            span: Span {
                start: start.start,
                end: end.end,
                line: start.line,
                column: start.column,
            },
        })
    }

    fn parse_statement(&mut self) -> Option<Statement> {
        if self.peek_is_doc_comment() {
            if let Some(doc) = self.parse_leading_doc_comment_block() {
                self.error("doc comment must document a declaration", doc.span);
            }
            return None;
        }
        if self.peek_is_keyword(Keyword::Return) {
            let span = self.expect_keyword(Keyword::Return)?;
            let value = self.parse_expression()?;
            return Some(Statement::Return { value, span });
        }
        if self.peek_is_keyword(Keyword::Abort) {
            let start = self.expect_keyword(Keyword::Abort)?;
            self.expect_symbol(Symbol::LeftParenthesis)?;
            let message = self.parse_expression()?;
            let right_parenthesis = self.expect_symbol(Symbol::RightParenthesis)?;
            let span = Span {
                start: start.start,
                end: right_parenthesis.end,
                line: start.line,
                column: start.column,
            };
            return Some(Statement::Abort { message, span });
        }
        if self.peek_is_keyword(Keyword::Break) {
            let span = self.expect_keyword(Keyword::Break)?;
            return Some(Statement::Break { span });
        }
        if self.peek_is_keyword(Keyword::Continue) {
            let span = self.expect_keyword(Keyword::Continue)?;
            return Some(Statement::Continue { span });
        }
        if self.peek_is_keyword(Keyword::If) {
            let start = self.expect_keyword(Keyword::If)?;
            let condition = self.parse_expression()?;
            let then_block = self.parse_block()?;
            let else_block = if self.peek_is_keyword(Keyword::Else) {
                self.advance();
                Some(self.parse_block()?)
            } else {
                None
            };
            let end_span = else_block
                .as_ref()
                .map_or_else(|| then_block.span.clone(), |block| block.span.clone());
            let span = Span {
                start: start.start,
                end: end_span.end,
                line: start.line,
                column: start.column,
            };
            return Some(Statement::If {
                condition,
                then_block,
                else_block,
                span,
            });
        }
        if self.peek_is_keyword(Keyword::For) {
            let start = self.expect_keyword(Keyword::For)?;
            let condition = if self.peek_is_symbol(Symbol::LeftBrace) {
                None
            } else {
                Some(self.parse_expression()?)
            };
            let body = self.parse_block()?;
            let span = Span {
                start: start.start,
                end: body.span.end,
                line: start.line,
                column: start.column,
            };
            return Some(Statement::For {
                condition,
                body,
                span,
            });
        }

        if self.peek_is_keyword(Keyword::Mut) {
            self.advance();
            let (name, name_span) = self.expect_identifier()?;
            let type_name = if self.peek_is_symbol(Symbol::Colon) {
                self.advance();
                Some(self.parse_type_name()?)
            } else {
                None
            };
            self.expect_symbol(Symbol::Assign)?;
            let initializer = self.parse_expression()?;
            let span = Span {
                start: name_span.start,
                end: initializer.span().end,
                line: name_span.line,
                column: name_span.column,
            };
            return Some(Statement::Let {
                name,
                mutable: true,
                type_name,
                initializer,
                span,
            });
        }

        if self.peek_is_identifier() && self.peek_second_is_symbol(Symbol::Equal) {
            let (name, name_span) = self.expect_identifier()?;
            self.advance();
            let value = self.parse_expression()?;
            let span = Span {
                start: name_span.start,
                end: value.span().end,
                line: name_span.line,
                column: name_span.column,
            };
            return Some(Statement::Assign {
                name,
                name_span,
                value,
                span,
            });
        }

        if self.peek_is_identifier()
            && (self.peek_second_is_symbol(Symbol::Colon)
                || self.peek_second_is_symbol(Symbol::Assign))
        {
            let (name, name_span) = self.expect_identifier()?;
            let type_name = if self.peek_is_symbol(Symbol::Colon) {
                self.advance();
                Some(self.parse_type_name()?)
            } else {
                None
            };
            self.expect_symbol(Symbol::Assign)?;
            let initializer = self.parse_expression()?;
            let span = Span {
                start: name_span.start,
                end: initializer.span().end,
                line: name_span.line,
                column: name_span.column,
            };
            return Some(Statement::Let {
                name,
                mutable: false,
                type_name,
                initializer,
                span,
            });
        }

        let value = self.parse_expression()?;
        let span = value.span();
        Some(Statement::Expression { value, span })
    }

    fn parse_type_name(&mut self) -> Option<TypeName> {
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

    fn parse_expression(&mut self) -> Option<Expression> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Option<Expression> {
        let mut expression = self.parse_and()?;
        loop {
            if !self.peek_is_keyword(Keyword::Or) {
                break;
            }
            let operator_span = self.advance().span.clone();
            let right = self.parse_and()?;
            let span = Span {
                start: expression.span().start,
                end: right.span().end,
                line: operator_span.line,
                column: operator_span.column,
            };
            expression = Expression::Binary {
                operator: BinaryOperator::Or,
                left: Box::new(expression),
                right: Box::new(right),
                span,
            };
        }
        Some(expression)
    }

    fn parse_and(&mut self) -> Option<Expression> {
        let mut expression = self.parse_equality()?;
        loop {
            if !self.peek_is_keyword(Keyword::And) {
                break;
            }
            let operator_span = self.advance().span.clone();
            let right = self.parse_equality()?;
            let span = Span {
                start: expression.span().start,
                end: right.span().end,
                line: operator_span.line,
                column: operator_span.column,
            };
            expression = Expression::Binary {
                operator: BinaryOperator::And,
                left: Box::new(expression),
                right: Box::new(right),
                span,
            };
        }
        Some(expression)
    }

    fn parse_equality(&mut self) -> Option<Expression> {
        let mut expression = self.parse_comparison()?;
        loop {
            if self.peek_is_symbol(Symbol::Equal) {
                let operator_span = self.advance().span.clone();
                self.error("unexpected '=' in expression", operator_span);
                let _ = self.parse_comparison();
                continue;
            }
            let operator = if self.peek_is_symbol(Symbol::EqualEqual) {
                BinaryOperator::EqualEqual
            } else if self.peek_is_symbol(Symbol::BangEqual) {
                BinaryOperator::NotEqual
            } else {
                break;
            };
            let operator_span = self.advance().span.clone();
            let right = self.parse_comparison()?;
            let span = Span {
                start: expression.span().start,
                end: right.span().end,
                line: operator_span.line,
                column: operator_span.column,
            };
            expression = Expression::Binary {
                operator,
                left: Box::new(expression),
                right: Box::new(right),
                span,
            };
        }
        Some(expression)
    }

    fn parse_comparison(&mut self) -> Option<Expression> {
        let mut expression = self.parse_additive()?;
        loop {
            if self.peek_is_keyword(Keyword::Matches) {
                let operator_span = self.advance().span.clone();
                let type_name = self.parse_type_name()?;
                let span = Span {
                    start: expression.span().start,
                    end: type_name.span.end,
                    line: operator_span.line,
                    column: operator_span.column,
                };
                expression = Expression::Matches {
                    value: Box::new(expression),
                    type_name,
                    span,
                };
                continue;
            }
            let operator = if self.peek_is_symbol(Symbol::Less) {
                BinaryOperator::LessThan
            } else if self.peek_is_symbol(Symbol::LessEqual) {
                BinaryOperator::LessThanOrEqual
            } else if self.peek_is_symbol(Symbol::Greater) {
                BinaryOperator::GreaterThan
            } else if self.peek_is_symbol(Symbol::GreaterEqual) {
                BinaryOperator::GreaterThanOrEqual
            } else {
                break;
            };
            let operator_span = self.advance().span.clone();
            let right = self.parse_additive()?;
            let span = Span {
                start: expression.span().start,
                end: right.span().end,
                line: operator_span.line,
                column: operator_span.column,
            };
            expression = Expression::Binary {
                operator,
                left: Box::new(expression),
                right: Box::new(right),
                span,
            };
        }
        Some(expression)
    }

    fn parse_additive(&mut self) -> Option<Expression> {
        let mut expression = self.parse_multiplicative()?;
        loop {
            let operator = if self.peek_is_symbol(Symbol::Plus) {
                BinaryOperator::Add
            } else if self.peek_is_symbol(Symbol::Minus) {
                BinaryOperator::Subtract
            } else {
                break;
            };
            let operator_span = self.advance().span.clone();
            let right = self.parse_multiplicative()?;
            let span = Span {
                start: expression.span().start,
                end: right.span().end,
                line: operator_span.line,
                column: operator_span.column,
            };
            expression = Expression::Binary {
                operator,
                left: Box::new(expression),
                right: Box::new(right),
                span,
            };
        }
        Some(expression)
    }

    fn parse_multiplicative(&mut self) -> Option<Expression> {
        let mut expression = self.parse_unary()?;
        loop {
            let operator = if self.peek_is_symbol(Symbol::Star) {
                BinaryOperator::Multiply
            } else if self.peek_is_symbol(Symbol::Slash) {
                BinaryOperator::Divide
            } else {
                break;
            };
            let operator_span = self.advance().span.clone();
            let right = self.parse_postfix()?;
            let span = Span {
                start: expression.span().start,
                end: right.span().end,
                line: operator_span.line,
                column: operator_span.column,
            };
            expression = Expression::Binary {
                operator,
                left: Box::new(expression),
                right: Box::new(right),
                span,
            };
        }
        Some(expression)
    }

    fn parse_postfix(&mut self) -> Option<Expression> {
        let mut expression = self.parse_primary()?;
        loop {
            if self.peek_is_symbol(Symbol::LeftParenthesis) {
                let left_parenthesis = self.expect_symbol(Symbol::LeftParenthesis)?;
                let arguments = self.parse_arguments();
                let right_parenthesis = self.expect_symbol(Symbol::RightParenthesis)?;
                let span = Span {
                    start: expression.span().start,
                    end: right_parenthesis.end,
                    line: left_parenthesis.line,
                    column: left_parenthesis.column,
                };
                expression = Expression::Call {
                    callee: Box::new(expression),
                    arguments,
                    span,
                };
                continue;
            }
            if self.peek_is_symbol(Symbol::Dot) {
                let dot = self.expect_symbol(Symbol::Dot)?;
                let (field, field_span) = self.expect_identifier()?;
                let span = Span {
                    start: expression.span().start,
                    end: field_span.end,
                    line: dot.line,
                    column: dot.column,
                };
                expression = Expression::FieldAccess {
                    target: Box::new(expression),
                    field,
                    field_span,
                    span,
                };
                continue;
            }
            break;
        }
        Some(expression)
    }

    fn parse_unary(&mut self) -> Option<Expression> {
        if self.peek_is_keyword(Keyword::Not) {
            let operator_span = self.advance().span.clone();
            let expression = self.parse_unary()?;
            let span = Span {
                start: operator_span.start,
                end: expression.span().end,
                line: operator_span.line,
                column: operator_span.column,
            };
            return Some(Expression::Unary {
                operator: UnaryOperator::Not,
                expression: Box::new(expression),
                span,
            });
        }
        if self.peek_is_symbol(Symbol::Minus) {
            let operator_span = self.advance().span.clone();
            let expression = self.parse_unary()?;
            let span = Span {
                start: operator_span.start,
                end: expression.span().end,
                line: operator_span.line,
                column: operator_span.column,
            };
            return Some(Expression::Unary {
                operator: UnaryOperator::Negate,
                expression: Box::new(expression),
                span,
            });
        }
        self.parse_postfix()
    }

    fn parse_arguments(&mut self) -> Vec<Expression> {
        let mut arguments = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightParenthesis) {
            return arguments;
        }
        loop {
            self.skip_statement_terminators();
            if let Some(argument) = self.parse_expression() {
                arguments.push(argument);
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
        arguments
    }

    fn parse_primary(&mut self) -> Option<Expression> {
        let token = self.advance();
        match token.kind {
            TokenKind::IntegerLiteral(value) => Some(Expression::IntegerLiteral {
                value,
                span: token.span,
            }),
            TokenKind::Keyword(Keyword::Nil) => Some(Expression::NilLiteral { span: token.span }),
            TokenKind::StringLiteral(value) => Some(Expression::StringLiteral {
                value,
                span: token.span,
            }),
            TokenKind::BooleanLiteral(value) => Some(Expression::BooleanLiteral {
                value,
                span: token.span,
            }),
            TokenKind::Identifier(name) => {
                if self.peek_is_symbol(Symbol::LeftBrace)
                    && name
                        .chars()
                        .next()
                        .is_some_and(|ch| ch.is_ascii_uppercase())
                {
                    let type_name = TypeName {
                        names: vec![TypeNameAtom {
                            name,
                            span: token.span.clone(),
                        }],
                        span: token.span,
                    };
                    return self.parse_struct_literal(type_name);
                }
                Some(Expression::Identifier {
                    name,
                    span: token.span,
                })
            }
            TokenKind::Keyword(Keyword::Match) => self.parse_match_expression(&token.span),
            TokenKind::Symbol(Symbol::LeftParenthesis) => {
                let expression = self.parse_expression()?;
                self.expect_symbol(Symbol::RightParenthesis)?;
                Some(expression)
            }
            TokenKind::Error(_message) => None,
            _ => {
                self.error("expected expression", token.span);
                None
            }
        }
    }

    fn parse_struct_literal(&mut self, type_name: TypeName) -> Option<Expression> {
        let left_brace = self.expect_symbol(Symbol::LeftBrace)?;
        let fields = self.parse_struct_literal_fields();
        let right_brace = self.expect_symbol(Symbol::RightBrace)?;
        let span = Span {
            start: type_name.span.start,
            end: right_brace.end,
            line: left_brace.line,
            column: left_brace.column,
        };
        Some(Expression::StructLiteral {
            type_name,
            fields,
            span,
        })
    }

    fn parse_match_expression(&mut self, start_span: &Span) -> Option<Expression> {
        let target = self.parse_expression()?;
        self.expect_symbol(Symbol::LeftBrace)?;
        let arms = self.parse_match_arms();
        let right_brace = self.expect_symbol(Symbol::RightBrace)?;
        let span = Span {
            start: start_span.start,
            end: right_brace.end,
            line: start_span.line,
            column: start_span.column,
        };
        Some(Expression::Match {
            target: Box::new(target),
            arms,
            span,
        })
    }

    fn parse_match_arms(&mut self) -> Vec<MatchArm> {
        let mut arms = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightBrace) {
            return arms;
        }
        loop {
            self.skip_statement_terminators();
            if let Some(arm) = self.parse_match_arm() {
                arms.push(arm);
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
            if self.peek_is_symbol(Symbol::RightBrace) {
                break;
            }
            if self.peek_is_identifier() {
                continue;
            }
            break;
        }
        arms
    }

    fn parse_match_arm(&mut self) -> Option<MatchArm> {
        let pattern = self.parse_match_pattern()?;
        self.expect_symbol(Symbol::FatArrow)?;
        let value = self.parse_expression()?;
        let span = Span {
            start: pattern.span().start,
            end: value.span().end,
            line: pattern.span().line,
            column: pattern.span().column,
        };
        Some(MatchArm {
            pattern,
            value,
            span,
        })
    }

    fn parse_match_pattern(&mut self) -> Option<MatchPattern> {
        let (name, name_span) = self.expect_identifier()?;
        if self.peek_is_symbol(Symbol::Colon) {
            self.advance();
            let type_name = self.parse_type_name()?;
            let span = Span {
                start: name_span.start,
                end: type_name.span.end,
                line: name_span.line,
                column: name_span.column,
            };
            return Some(MatchPattern::Binding {
                name,
                name_span,
                type_name,
                span,
            });
        }

        let type_name = TypeName {
            names: vec![TypeNameAtom {
                name,
                span: name_span.clone(),
            }],
            span: name_span.clone(),
        };
        Some(MatchPattern::Type {
            type_name,
            span: name_span,
        })
    }

    fn parse_union_type_declaration(&mut self) -> Option<Vec<TypeName>> {
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

    fn parse_struct_literal_fields(&mut self) -> Vec<StructLiteralField> {
        let mut fields = Vec::new();
        self.skip_statement_terminators();
        if self.peek_is_symbol(Symbol::RightBrace) {
            return fields;
        }
        loop {
            self.skip_statement_terminators();
            if let Some(field) = self.parse_struct_literal_field() {
                fields.push(field);
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
        fields
    }

    fn parse_struct_literal_field(&mut self) -> Option<StructLiteralField> {
        let (name, name_span) = self.expect_identifier()?;
        self.expect_symbol(Symbol::Colon)?;
        let value = self.parse_expression()?;
        let span = Span {
            start: name_span.start,
            end: value.span().end,
            line: name_span.line,
            column: name_span.column,
        };
        Some(StructLiteralField {
            name,
            name_span,
            value,
            span,
        })
    }

    fn expect_identifier(&mut self) -> Option<(String, Span)> {
        let token = self.advance();
        match token.kind {
            TokenKind::Identifier(name) => Some((name, token.span)),
            TokenKind::Keyword(keyword) => {
                self.error(
                    format!(
                        "reserved keyword '{}' cannot be used as an identifier",
                        keyword.as_str()
                    ),
                    token.span,
                );
                None
            }
            _ => {
                self.error("expected identifier", token.span);
                None
            }
        }
    }

    fn expect_type_name_part(&mut self) -> Option<(String, Span)> {
        let token = self.advance();
        match token.kind {
            TokenKind::Identifier(name) => Some((name, token.span)),
            TokenKind::Keyword(Keyword::Nil) => {
                Some((Keyword::Nil.as_str().to_string(), token.span))
            }
            TokenKind::Keyword(keyword) => {
                self.error(
                    format!(
                        "reserved keyword '{}' cannot be used as an identifier",
                        keyword.as_str()
                    ),
                    token.span,
                );
                None
            }
            _ => {
                self.error("expected identifier", token.span);
                None
            }
        }
    }

    fn expect_keyword(&mut self, keyword: Keyword) -> Option<Span> {
        let token = self.advance();
        match token.kind {
            TokenKind::Keyword(found) if found == keyword => Some(token.span),
            _ => {
                self.error(format!("expected keyword '{keyword:?}'"), token.span);
                None
            }
        }
    }

    fn expect_symbol(&mut self, symbol: Symbol) -> Option<Span> {
        let token = self.advance();
        match token.kind {
            TokenKind::Symbol(found) if found == symbol => Some(token.span),
            _ => {
                self.error("expected symbol", token.span);
                None
            }
        }
    }

    fn parse_visibility(&mut self) -> Visibility {
        if self.peek_is_keyword(Keyword::Public) {
            self.advance();
            Visibility::Public
        } else {
            Visibility::Private
        }
    }

    fn peek_is_keyword(&self, keyword: Keyword) -> bool {
        matches!(self.peek().kind, TokenKind::Keyword(found) if found == keyword)
    }

    fn peek_is_identifier(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Identifier(_))
    }

    fn peek_is_doc_comment(&self) -> bool {
        matches!(self.peek().kind, TokenKind::DocComment(_))
    }

    fn peek_is_symbol(&self, symbol: Symbol) -> bool {
        matches!(self.peek().kind, TokenKind::Symbol(found) if found == symbol)
    }

    fn peek_second_is_symbol(&self, symbol: Symbol) -> bool {
        matches!(self.peek_n(1).kind, TokenKind::Symbol(found) if found == symbol)
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek().kind, TokenKind::EndOfFile)
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.position]
    }

    fn peek_n(&self, n: usize) -> &Token {
        let index = self.position + n;
        if index < self.tokens.len() {
            &self.tokens[index]
        } else {
            self.tokens
                .last()
                .expect("token stream must include EOF token")
        }
    }

    fn advance(&mut self) -> Token {
        let token = self.tokens[self.position].clone();
        if !matches!(token.kind, TokenKind::EndOfFile) {
            self.position += 1;
        }
        token
    }

    fn skip_statement_terminators(&mut self) {
        while matches!(self.peek().kind, TokenKind::StatementTerminator) {
            self.advance();
        }
    }

    fn parse_leading_doc_comment_block(&mut self) -> Option<DocComment> {
        if !self.peek_is_doc_comment() {
            return None;
        }
        let mut lines = Vec::new();
        let start_span = self.peek().span.clone();
        let mut end = start_span.end;
        let mut end_line = start_span.line;
        while let TokenKind::DocComment(line) = self.peek().kind.clone() {
            let token = self.advance();
            lines.push(line);
            end = token.span.end;
            end_line = token.span.line;
        }
        Some(DocComment {
            lines,
            span: Span {
                start: start_span.start,
                end,
                line: start_span.line,
                column: start_span.column,
            },
            end_line,
        })
    }

    fn peek_span(&self) -> Span {
        self.peek().span.clone()
    }

    fn error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics.push(Diagnostic::new(message, span));
    }

    fn synchronize(&mut self) {
        while !self.at_eof() {
            if self.peek_is_keyword(Keyword::Type) || self.peek_is_keyword(Keyword::Function) {
                return;
            }
            if self.peek_is_identifier()
                && (self.peek_second_is_symbol(Symbol::Assign)
                    || self.peek_second_is_symbol(Symbol::DoubleColon))
            {
                return;
            }
            self.advance();
        }
    }

    fn synchronize_list_item(&mut self, separator: Symbol, end: Symbol) {
        while !self.at_eof() {
            self.skip_statement_terminators();
            if self.peek_is_symbol(separator) || self.peek_is_symbol(end) {
                return;
            }
            self.advance();
        }
    }

    fn synchronize_statement(&mut self) {
        while !self.at_eof() {
            if self.peek_is_symbol(Symbol::RightBrace) {
                return;
            }
            if self.peek_is_keyword(Keyword::Return)
                || self.peek_is_keyword(Keyword::Abort)
                || self.peek_is_keyword(Keyword::Break)
                || self.peek_is_keyword(Keyword::Continue)
                || self.peek_is_keyword(Keyword::If)
                || self.peek_is_keyword(Keyword::For)
            {
                return;
            }
            if matches!(self.peek().kind, TokenKind::StatementTerminator) {
                self.advance();
                return;
            }
            self.advance();
        }
    }
}
trait ExpressionSpan {
    fn span(&self) -> Span;
}

impl ExpressionSpan for Expression {
    fn span(&self) -> Span {
        match self {
            Expression::IntegerLiteral { span, .. }
            | Expression::NilLiteral { span, .. }
            | Expression::BooleanLiteral { span, .. }
            | Expression::StringLiteral { span, .. }
            | Expression::Identifier { span, .. }
            | Expression::StructLiteral { span, .. }
            | Expression::FieldAccess { span, .. }
            | Expression::Call { span, .. }
            | Expression::Unary { span, .. }
            | Expression::Binary { span, .. }
            | Expression::Match { span, .. }
            | Expression::Matches { span, .. } => span.clone(),
        }
    }
}
