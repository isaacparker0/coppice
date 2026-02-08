use crate::diagnostics::{Diagnostic, Span};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyword {
    Function,
    Return,
    If,
    Mut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Symbol {
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Plus,
    Minus,
    Star,
    Slash,
    Assign,
    Arrow,
    EqEq,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Ident(String),
    IntLiteral(i64),
    StringLiteral(String),
    BoolLiteral(bool),
    Keyword(Keyword),
    Symbol(Symbol),
    Eof,
    Error(String),
}

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

pub struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    idx: usize,
    line: usize,
    col: usize,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            idx: 0,
            line: 1,
            col: 1,
            diagnostics: Vec::new(),
        }
    }

    pub fn lex_all(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token();
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        tokens
    }

    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }

    fn next_token(&mut self) -> Token {
        self.skip_whitespace();
        let start = self.idx;
        let (line, col) = (self.line, self.col);

        if self.idx >= self.bytes.len() {
            return Token {
                kind: TokenKind::Eof,
                span: Span {
                    start,
                    end: start,
                    line,
                    col,
                },
            };
        }

        let ch = self.peek_byte();
        match ch {
            b'(' => self.single(Symbol::LParen, 1, start, line, col),
            b')' => self.single(Symbol::RParen, 1, start, line, col),
            b'{' => self.single(Symbol::LBrace, 1, start, line, col),
            b'}' => self.single(Symbol::RBrace, 1, start, line, col),
            b',' => self.single(Symbol::Comma, 1, start, line, col),
            b':' => {
                if self.match_bytes(b":=") {
                    self.single(Symbol::Assign, 2, start, line, col)
                } else {
                    self.single(Symbol::Colon, 1, start, line, col)
                }
            }
            b'-' => {
                if self.match_bytes(b"->") {
                    self.single(Symbol::Arrow, 2, start, line, col)
                } else {
                    self.single(Symbol::Minus, 1, start, line, col)
                }
            }
            b'+' => self.single(Symbol::Plus, 1, start, line, col),
            b'*' => self.single(Symbol::Star, 1, start, line, col),
            b'/' => self.single(Symbol::Slash, 1, start, line, col),
            b'=' => {
                if self.match_bytes(b"==") {
                    self.single(Symbol::EqEq, 2, start, line, col)
                } else {
                    self.advance();
                    self.error_token("unexpected '='; use '==' for equality", start, line, col)
                }
            }
            b'"' => self.lex_string(start, line, col),
            b'0'..=b'9' => self.lex_int(start, line, col),
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.lex_ident(start, line, col),
            _ => {
                let msg = format!("unexpected character '{}'", self.peek_char());
                self.advance();
                self.error_token(msg, start, line, col)
            }
        }
    }

    fn single(&mut self, sym: Symbol, len: usize, start: usize, line: usize, col: usize) -> Token {
        self.advance_by(len);
        Token {
            kind: TokenKind::Symbol(sym),
            span: Span {
                start,
                end: start + len,
                line,
                col,
            },
        }
    }

    fn lex_string(&mut self, start: usize, line: usize, col: usize) -> Token {
        self.advance();
        let content_start = self.idx;
        while self.idx < self.bytes.len() {
            let b = self.peek_byte();
            if b == b'"' {
                let content = &self.src[content_start..self.idx];
                self.advance();
                return Token {
                    kind: TokenKind::StringLiteral(content.to_string()),
                    span: Span {
                        start,
                        end: self.idx,
                        line,
                        col,
                    },
                };
            }
            if b == b'\n' {
                break;
            }
            self.advance();
        }

        self.diagnostics.push(Diagnostic::new(
            "unterminated string literal",
            Span {
                start,
                end: self.idx,
                line,
                col,
            },
        ));
        Token {
            kind: TokenKind::Error("unterminated string literal".to_string()),
            span: Span {
                start,
                end: self.idx,
                line,
                col,
            },
        }
    }

    fn lex_int(&mut self, start: usize, line: usize, col: usize) -> Token {
        while self.idx < self.bytes.len() {
            match self.peek_byte() {
                b'0'..=b'9' => self.advance(),
                _ => break,
            }
        }
        let text = &self.src[start..self.idx];
        let value = text.parse::<i64>();
        if let Ok(value) = value {
            Token {
                kind: TokenKind::IntLiteral(value),
                span: Span {
                    start,
                    end: self.idx,
                    line,
                    col,
                },
            }
        } else {
            self.diagnostics.push(Diagnostic::new(
                "integer literal out of range",
                Span {
                    start,
                    end: self.idx,
                    line,
                    col,
                },
            ));
            Token {
                kind: TokenKind::Error("integer literal out of range".to_string()),
                span: Span {
                    start,
                    end: self.idx,
                    line,
                    col,
                },
            }
        }
    }

    fn lex_ident(&mut self, start: usize, line: usize, col: usize) -> Token {
        while self.idx < self.bytes.len() {
            match self.peek_byte() {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' => self.advance(),
                _ => break,
            }
        }
        let text = &self.src[start..self.idx];
        let kind = match text {
            "function" => TokenKind::Keyword(Keyword::Function),
            "return" => TokenKind::Keyword(Keyword::Return),
            "if" => TokenKind::Keyword(Keyword::If),
            "mut" => TokenKind::Keyword(Keyword::Mut),
            "true" => TokenKind::BoolLiteral(true),
            "false" => TokenKind::BoolLiteral(false),
            _ => TokenKind::Ident(text.to_string()),
        };
        Token {
            kind,
            span: Span {
                start,
                end: self.idx,
                line,
                col,
            },
        }
    }

    fn error_token(&mut self, message: impl Into<String>, start: usize, line: usize, col: usize) -> Token {
        let message = message.into();
        self.diagnostics.push(Diagnostic::new(
            message.clone(),
            Span {
                start,
                end: self.idx,
                line,
                col,
            },
        ));
        Token {
            kind: TokenKind::Error(message),
            span: Span {
                start,
                end: self.idx,
                line,
                col,
            },
        }
    }

    fn skip_whitespace(&mut self) {
        while self.idx < self.bytes.len() {
            match self.peek_byte() {
                b' ' | b'\t' | b'\r' | b'\n' => {
                    self.advance();
                }
                b'/' => {
                    if self.match_bytes(b"//") {
                        self.advance_by(2);
                        while self.idx < self.bytes.len() && self.peek_byte() != b'\n' {
                            self.advance();
                        }
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
    }

    fn advance(&mut self) {
        if self.idx < self.bytes.len() {
            if self.bytes[self.idx] == b'\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            self.idx += 1;
        }
    }

    fn advance_by(&mut self, n: usize) {
        for _ in 0..n {
            self.advance();
        }
    }

    fn peek_byte(&self) -> u8 {
        self.bytes[self.idx]
    }

    fn peek_char(&self) -> char {
        self.bytes[self.idx] as char
    }

    fn match_bytes(&self, s: &[u8]) -> bool {
        self.bytes.get(self.idx..self.idx + s.len()) == Some(s)
    }
}
