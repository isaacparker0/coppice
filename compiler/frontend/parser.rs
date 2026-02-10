use crate::ast::{
    BinaryOperator, Block, ConstantDeclaration, Expression, File, FunctionDeclaration, Parameter,
    Statement, StructField, StructLiteralField, TypeDeclaration, TypeName, UnaryOperator,
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
        let mut type_declarations = Vec::new();
        let mut constant_declarations = Vec::new();
        let mut function_declarations = Vec::new();
        while !self.at_eof() {
            if self.peek_is_keyword(Keyword::Function) {
                if let Some(function) = self.parse_function() {
                    function_declarations.push(function);
                } else {
                    self.synchronize();
                }
            } else if self.peek_is_identifier() && self.peek_second_is_symbol(Symbol::DoubleColon) {
                if let Some(type_declaration) = self.parse_type_declaration() {
                    type_declarations.push(type_declaration);
                } else {
                    self.synchronize();
                }
            } else if self.peek_is_identifier() {
                if let Some(constant) = self.parse_constant_declaration() {
                    constant_declarations.push(constant);
                } else {
                    self.synchronize();
                }
            } else {
                let span = self.peek_span();
                self.error("expected declaration", span);
                self.synchronize();
            }
        }
        File {
            type_declarations,
            constant_declarations,
            function_declarations,
        }
    }

    fn parse_type_declaration(&mut self) -> Option<TypeDeclaration> {
        let (name, name_span) = self.expect_identifier()?;
        self.expect_symbol(Symbol::DoubleColon)?;
        self.expect_keyword(Keyword::Struct)?;
        let start = name_span.clone();
        self.expect_symbol(Symbol::LeftBrace)?;
        let fields = self.parse_struct_fields();
        let right_brace = self.expect_symbol(Symbol::RightBrace)?;
        let span = Span {
            start: start.start,
            end: right_brace.end,
            line: start.line,
            column: start.column,
        };
        Some(TypeDeclaration { name, fields, span })
    }

    fn parse_struct_fields(&mut self) -> Vec<StructField> {
        let mut fields = Vec::new();
        if self.peek_is_symbol(Symbol::RightBrace) {
            return fields;
        }
        loop {
            if let Some(field) = self.parse_struct_field() {
                fields.push(field);
            } else {
                self.synchronize_list_item(Symbol::Comma, Symbol::RightBrace);
                if self.peek_is_symbol(Symbol::RightBrace) {
                    break;
                }
            }

            if self.peek_is_symbol(Symbol::Comma) {
                self.advance();
                if self.peek_is_symbol(Symbol::RightBrace) {
                    break;
                }
                continue;
            }
            break;
        }
        fields
    }

    fn parse_struct_field(&mut self) -> Option<StructField> {
        let (name, name_span) = self.expect_identifier()?;
        self.expect_symbol(Symbol::Colon)?;
        let type_name = self.parse_type_name()?;
        let span = Span {
            start: name_span.start,
            end: type_name.span.end,
            line: name_span.line,
            column: name_span.column,
        };
        Some(StructField {
            name,
            type_name,
            span,
        })
    }

    fn parse_function(&mut self) -> Option<FunctionDeclaration> {
        let start = self.expect_keyword(Keyword::Function)?;
        let (name, name_span) = self.expect_identifier()?;
        self.expect_symbol(Symbol::LeftParen)?;
        let parameters = self.parse_parameters();
        self.expect_symbol(Symbol::RightParen)?;
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
            span: Span {
                start: start.start,
                end: body_end,
                line: start.line,
                column: start.column,
            },
        })
    }

    fn parse_constant_declaration(&mut self) -> Option<ConstantDeclaration> {
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
            span,
        })
    }

    fn parse_parameters(&mut self) -> Vec<Parameter> {
        let mut parameters = Vec::new();
        if self.peek_is_symbol(Symbol::RightParen) {
            return parameters;
        }
        loop {
            if let Some(parameter) = self.parse_parameter() {
                parameters.push(parameter);
            } else {
                self.synchronize_list_item(Symbol::Comma, Symbol::RightParen);
                if self.peek_is_symbol(Symbol::RightParen) {
                    break;
                }
            }

            if self.peek_is_symbol(Symbol::Comma) {
                self.advance();
                if self.peek_is_symbol(Symbol::RightParen) {
                    break;
                }
                continue;
            }
            break;
        }
        parameters
    }

    fn parse_parameter(&mut self) -> Option<Parameter> {
        let (name, name_span) = self.expect_identifier()?;
        self.expect_symbol(Symbol::Colon)?;
        let type_name = self.parse_type_name()?;
        let span = Span {
            start: name_span.start,
            end: type_name.span.end,
            line: name_span.line,
            column: name_span.column,
        };
        Some(Parameter {
            name,
            type_name,
            span,
        })
    }

    fn parse_block(&mut self) -> Option<Block> {
        let start = self.expect_symbol(Symbol::LeftBrace)?;
        let mut statements = Vec::new();
        while !self.peek_is_symbol(Symbol::RightBrace) && !self.at_eof() {
            if let Some(statement) = self.parse_statement() {
                statements.push(statement);
            } else {
                self.synchronize_statement();
            }
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
        if self.peek_is_keyword(Keyword::Return) {
            let span = self.expect_keyword(Keyword::Return)?;
            let expression = self.parse_expression()?;
            return Some(Statement::Return { expression, span });
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

        let mutable = if self.peek_is_keyword(Keyword::Mut) {
            self.advance();
            true
        } else {
            false
        };

        if let Some((name, name_span)) = self.expect_identifier() {
            if mutable {
                let type_name = if self.peek_is_symbol(Symbol::Colon) {
                    self.advance();
                    Some(self.parse_type_name()?)
                } else {
                    None
                };
                self.expect_symbol(Symbol::Assign)?;
                let expression = self.parse_expression()?;
                let span = Span {
                    start: name_span.start,
                    end: expression.span().end,
                    line: name_span.line,
                    column: name_span.column,
                };
                return Some(Statement::Let {
                    name,
                    mutable,
                    type_name,
                    expression,
                    span,
                });
            }

            if self.peek_is_symbol(Symbol::Colon) || self.peek_is_symbol(Symbol::Assign) {
                let type_name = if self.peek_is_symbol(Symbol::Colon) {
                    self.advance();
                    Some(self.parse_type_name()?)
                } else {
                    None
                };
                self.expect_symbol(Symbol::Assign)?;
                let expression = self.parse_expression()?;
                let span = Span {
                    start: name_span.start,
                    end: expression.span().end,
                    line: name_span.line,
                    column: name_span.column,
                };
                return Some(Statement::Let {
                    name,
                    mutable: false,
                    type_name,
                    expression,
                    span,
                });
            }

            if self.peek_is_symbol(Symbol::Equal) {
                self.advance();
                let expression = self.parse_expression()?;
                let span = Span {
                    start: name_span.start,
                    end: expression.span().end,
                    line: name_span.line,
                    column: name_span.column,
                };
                return Some(Statement::Assign {
                    name,
                    name_span,
                    expression,
                    span,
                });
            }
        }
        None
    }

    fn parse_type_name(&mut self) -> Option<TypeName> {
        let (name, span) = self.expect_identifier()?;
        Some(TypeName { name, span })
    }

    fn parse_expression(&mut self) -> Option<Expression> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Option<Expression> {
        let mut expression = self.parse_and()?;
        while self.peek_is_keyword(Keyword::Or) {
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
        while self.peek_is_keyword(Keyword::And) {
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
            if self.peek_is_symbol(Symbol::LeftParen) {
                let left_paren = self.expect_symbol(Symbol::LeftParen)?;
                let arguments = self.parse_arguments();
                let right_paren = self.expect_symbol(Symbol::RightParen)?;
                let span = Span {
                    start: expression.span().start,
                    end: right_paren.end,
                    line: left_paren.line,
                    column: left_paren.column,
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
        if self.peek_is_symbol(Symbol::RightParen) {
            return arguments;
        }
        loop {
            if let Some(argument) = self.parse_expression() {
                arguments.push(argument);
            } else {
                self.synchronize_list_item(Symbol::Comma, Symbol::RightParen);
                if self.peek_is_symbol(Symbol::RightParen) {
                    break;
                }
            }

            if self.peek_is_symbol(Symbol::Comma) {
                self.advance();
                if self.peek_is_symbol(Symbol::RightParen) {
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
                        name,
                        span: token.span,
                    };
                    return self.parse_struct_literal(type_name);
                }
                Some(Expression::Identifier {
                    name,
                    span: token.span,
                })
            }
            TokenKind::Symbol(Symbol::LeftParen) => {
                let expression = self.parse_expression()?;
                self.expect_symbol(Symbol::RightParen)?;
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

    fn parse_struct_literal_fields(&mut self) -> Vec<StructLiteralField> {
        let mut fields = Vec::new();
        if self.peek_is_symbol(Symbol::RightBrace) {
            return fields;
        }
        loop {
            if let Some(field) = self.parse_struct_literal_field() {
                fields.push(field);
            } else {
                self.synchronize_list_item(Symbol::Comma, Symbol::RightBrace);
                if self.peek_is_symbol(Symbol::RightBrace) {
                    break;
                }
            }

            if self.peek_is_symbol(Symbol::Comma) {
                self.advance();
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

    fn peek_is_keyword(&self, keyword: Keyword) -> bool {
        matches!(self.peek().kind, TokenKind::Keyword(found) if found == keyword)
    }

    fn peek_is_identifier(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Identifier(_))
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

    fn peek_span(&self) -> Span {
        self.peek().span.clone()
    }

    fn error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics.push(Diagnostic::new(message, span));
    }

    fn synchronize(&mut self) {
        while !self.at_eof() {
            if self.peek_is_keyword(Keyword::Function) {
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
            if self.peek_is_keyword(Keyword::Return) || self.peek_is_keyword(Keyword::If) {
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
            | Expression::BooleanLiteral { span, .. }
            | Expression::StringLiteral { span, .. }
            | Expression::Identifier { span, .. }
            | Expression::StructLiteral { span, .. }
            | Expression::FieldAccess { span, .. }
            | Expression::Call { span, .. }
            | Expression::Unary { span, .. }
            | Expression::Binary { span, .. } => span.clone(),
        }
    }
}
