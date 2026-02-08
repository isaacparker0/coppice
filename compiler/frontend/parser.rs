use crate::ast::*;
use crate::diagnostics::{Diagnostic, Span};
use crate::lexer::{Keyword, Symbol, Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            diagnostics: Vec::new(),
        }
    }

    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }

    pub fn parse_file(&mut self) -> File {
        let mut functions = Vec::new();
        while !self.at_eof() {
            if self.peek_is_keyword(Keyword::Function) {
                if let Some(func) = self.parse_function() {
                    functions.push(func);
                } else {
                    self.synchronize();
                }
            } else {
                let span = self.peek_span();
                self.error("expected 'function'", span);
                self.synchronize();
            }
        }
        File { functions }
    }

    fn parse_function(&mut self) -> Option<Function> {
        let start = self.expect_keyword(Keyword::Function)?;
        let name = self.expect_ident()?;
        self.expect_symbol(Symbol::LParen)?;
        let params = self.parse_params();
        self.expect_symbol(Symbol::RParen)?;
        self.expect_symbol(Symbol::Arrow)?;
        let return_type = self.parse_type_name()?;
        let body = self.parse_block()?;
        let body_end = body.span.end;
        Some(Function {
            name: name.0,
            params,
            return_type,
            body,
            span: Span {
                start: start.start,
                end: body_end,
                line: start.line,
                col: start.col,
            },
        })
    }

    fn parse_params(&mut self) -> Vec<Param> {
        let mut params = Vec::new();
        if self.peek_is_symbol(Symbol::RParen) {
            return params;
        }
        loop {
            let (name, name_span) = match self.expect_ident() {
                Some(v) => v,
                None => break,
            };
            if self.expect_symbol(Symbol::Colon).is_none() {
                break;
            }
            let ty = match self.parse_type_name() {
                Some(t) => t,
                None => break,
            };
            let span = Span {
                start: name_span.start,
                end: ty.span.end,
                line: name_span.line,
                col: name_span.col,
            };
            params.push(Param {
                name,
                ty,
                span,
            });
            if self.peek_is_symbol(Symbol::Comma) {
                self.advance();
                continue;
            }
            break;
        }
        params
    }

    fn parse_block(&mut self) -> Option<Block> {
        let start = self.expect_symbol(Symbol::LBrace)?;
        let mut stmts = Vec::new();
        while !self.peek_is_symbol(Symbol::RBrace) && !self.at_eof() {
            if let Some(stmt) = self.parse_stmt() {
                stmts.push(stmt);
            } else {
                self.synchronize_stmt();
            }
        }
        let end = self.expect_symbol(Symbol::RBrace)?;
        Some(Block {
            stmts,
            span: Span {
                start: start.start,
                end: end.end,
                line: start.line,
                col: start.col,
            },
        })
    }

    fn parse_stmt(&mut self) -> Option<Stmt> {
        if self.peek_is_keyword(Keyword::Return) {
            let span = self.expect_keyword(Keyword::Return)?;
            let expr = self.parse_expr()?;
            return Some(Stmt::Return { expr, span });
        }
        if self.peek_is_keyword(Keyword::If) {
            let start = self.expect_keyword(Keyword::If)?;
            let condition = self.parse_expr()?;
            let then_block = self.parse_block()?;
            let span = Span {
                start: start.start,
                end: then_block.span.end,
                line: start.line,
                col: start.col,
            };
            return Some(Stmt::If {
                condition,
                then_block,
                span,
            });
        }

        let mutable = if self.peek_is_keyword(Keyword::Mut) {
            self.advance();
            true
        } else {
            false
        };

        if let Some((name, name_span)) = self.expect_ident() {
            if self.expect_symbol(Symbol::Assign).is_none() {
                return None;
            }
            let expr = self.parse_expr()?;
            let span = Span {
                start: name_span.start,
                end: expr.span().end,
                line: name_span.line,
                col: name_span.col,
            };
            return Some(Stmt::Let {
                name,
                mutable,
                expr,
                span,
            });
        }
        None
    }

    fn parse_type_name(&mut self) -> Option<TypeName> {
        let (name, span) = self.expect_ident()?;
        Some(TypeName { name, span })
    }

    fn parse_expr(&mut self) -> Option<Expr> {
        self.parse_equality()
    }

    fn parse_equality(&mut self) -> Option<Expr> {
        let mut expr = self.parse_additive()?;
        while self.peek_is_symbol(Symbol::EqEq) {
            let op_span = self.advance().span.clone();
            let right = self.parse_additive()?;
            let span = Span {
                start: expr.span().start,
                end: right.span().end,
                line: op_span.line,
                col: op_span.col,
            };
            expr = Expr::Binary {
                op: BinOp::EqEq,
                left: Box::new(expr),
                right: Box::new(right),
                span,
            };
        }
        Some(expr)
    }

    fn parse_additive(&mut self) -> Option<Expr> {
        let mut expr = self.parse_multiplicative()?;
        loop {
            let op = if self.peek_is_symbol(Symbol::Plus) {
                BinOp::Add
            } else if self.peek_is_symbol(Symbol::Minus) {
                BinOp::Sub
            } else {
                break;
            };
            let op_span = self.advance().span.clone();
            let right = self.parse_multiplicative()?;
            let span = Span {
                start: expr.span().start,
                end: right.span().end,
                line: op_span.line,
                col: op_span.col,
            };
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(right),
                span,
            };
        }
        Some(expr)
    }

    fn parse_multiplicative(&mut self) -> Option<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            let op = if self.peek_is_symbol(Symbol::Star) {
                BinOp::Mul
            } else if self.peek_is_symbol(Symbol::Slash) {
                BinOp::Div
            } else {
                break;
            };
            let op_span = self.advance().span.clone();
            let right = self.parse_primary()?;
            let span = Span {
                start: expr.span().start,
                end: right.span().end,
                line: op_span.line,
                col: op_span.col,
            };
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(right),
                span,
            };
        }
        Some(expr)
    }

    fn parse_primary(&mut self) -> Option<Expr> {
        let tok = self.advance();
        match tok.kind {
            TokenKind::IntLiteral(value) => Some(Expr::IntLiteral {
                value,
                span: tok.span,
            }),
            TokenKind::StringLiteral(value) => Some(Expr::StringLiteral {
                value,
                span: tok.span,
            }),
            TokenKind::BoolLiteral(value) => Some(Expr::BoolLiteral {
                value,
                span: tok.span,
            }),
            TokenKind::Ident(name) => Some(Expr::Ident {
                name,
                span: tok.span,
            }),
            TokenKind::Symbol(Symbol::LParen) => {
                let expr = self.parse_expr()?;
                if self.expect_symbol(Symbol::RParen).is_none() {
                    return None;
                }
                Some(expr)
            }
            TokenKind::Error(message) => {
                self.error(message, tok.span);
                None
            }
            _ => {
                self.error("expected expression", tok.span);
                None
            }
        }
    }

    fn expect_ident(&mut self) -> Option<(String, Span)> {
        let tok = self.advance();
        match tok.kind {
            TokenKind::Ident(name) => Some((name, tok.span)),
            _ => {
                self.error("expected identifier", tok.span);
                None
            }
        }
    }

    fn expect_keyword(&mut self, kw: Keyword) -> Option<Span> {
        let tok = self.advance();
        match tok.kind {
            TokenKind::Keyword(k) if k == kw => Some(tok.span),
            _ => {
                self.error(format!("expected keyword '{:?}'", kw), tok.span);
                None
            }
        }
    }

    fn expect_symbol(&mut self, sym: Symbol) -> Option<Span> {
        let tok = self.advance();
        match tok.kind {
            TokenKind::Symbol(s) if s == sym => Some(tok.span),
            _ => {
                self.error("expected symbol", tok.span);
                None
            }
        }
    }

    fn peek_is_keyword(&self, kw: Keyword) -> bool {
        matches!(self.peek().kind, TokenKind::Keyword(k) if k == kw)
    }

    fn peek_is_symbol(&self, sym: Symbol) -> bool {
        matches!(self.peek().kind, TokenKind::Symbol(s) if s == sym)
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens[self.pos].clone();
        if !matches!(tok.kind, TokenKind::Eof) {
            self.pos += 1;
        }
        tok
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
            self.advance();
        }
    }

    fn synchronize_stmt(&mut self) {
        while !self.at_eof() {
            if self.peek_is_symbol(Symbol::RBrace) {
                return;
            }
            if self.peek_is_keyword(Keyword::Return) || self.peek_is_keyword(Keyword::If) {
                return;
            }
            self.advance();
        }
    }
}

trait ExprSpan {
    fn span(&self) -> Span;
}

impl ExprSpan for Expr {
    fn span(&self) -> Span {
        match self {
            Expr::IntLiteral { span, .. } => span.clone(),
            Expr::BoolLiteral { span, .. } => span.clone(),
            Expr::StringLiteral { span, .. } => span.clone(),
            Expr::Ident { span, .. } => span.clone(),
            Expr::Binary { span, .. } => span.clone(),
        }
    }
}
