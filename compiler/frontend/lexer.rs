use crate::diagnostics::{Diagnostic, Span};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyword {
    Public,
    Type,
    Function,
    Return,
    Abort,
    Break,
    Continue,
    If,
    For,
    Else,
    Match,
    And,
    Or,
    Not,
    Nil,
    Mut,
    Struct,
    Matches,
}

impl Keyword {
    pub fn as_str(self) -> &'static str {
        match self {
            Keyword::Public => "public",
            Keyword::Type => "type",
            Keyword::Function => "function",
            Keyword::Return => "return",
            Keyword::Abort => "abort",
            Keyword::Break => "break",
            Keyword::Continue => "continue",
            Keyword::If => "if",
            Keyword::For => "for",
            Keyword::Else => "else",
            Keyword::Match => "match",
            Keyword::And => "and",
            Keyword::Or => "or",
            Keyword::Not => "not",
            Keyword::Nil => "nil",
            Keyword::Mut => "mut",
            Keyword::Struct => "struct",
            Keyword::Matches => "matches",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Symbol {
    LeftParenthesis,
    RightParenthesis,
    LeftBrace,
    RightBrace,
    Comma,
    Colon,
    DoubleColon,
    Dot,
    Pipe,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Plus,
    Minus,
    Star,
    Slash,
    Assign,
    Arrow,
    FatArrow,
    EqualEqual,
    BangEqual,
    Equal,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Identifier(String),
    IntegerLiteral(i64),
    StringLiteral(String),
    BooleanLiteral(bool),
    Keyword(Keyword),
    Symbol(Symbol),
    /// Raw `\n` from the source. These are removed during normalization.
    Newline,
    /// Semantic statement terminator inserted during newline normalization.
    StatementTerminator,
    EndOfFile,
    Error(String),
}

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

pub struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    index: usize,
    line: usize,
    column: usize,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            index: 0,
            line: 1,
            column: 1,
            diagnostics: Vec::new(),
        }
    }

    pub fn lex_all_tokens(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        loop {
            let token = self.next_token();
            let is_end_of_file = token.kind == TokenKind::EndOfFile;
            tokens.push(token);
            if is_end_of_file {
                break;
            }
        }
        normalize_newlines_to_statement_terminators(tokens)
    }

    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }

    fn next_token(&mut self) -> Token {
        self.skip_whitespace();
        let start = self.index;
        let (line, column) = (self.line, self.column);

        if self.index >= self.bytes.len() {
            return Token {
                kind: TokenKind::EndOfFile,
                span: Span {
                    start,
                    end: start,
                    line,
                    column,
                },
            };
        }

        let character = self.peek_byte();
        match character {
            b'\n' => {
                self.advance();
                Token {
                    kind: TokenKind::Newline,
                    span: Span {
                        start,
                        end: start + 1,
                        line,
                        column,
                    },
                }
            }
            b'(' => self.single(Symbol::LeftParenthesis, 1, start, line, column),
            b')' => self.single(Symbol::RightParenthesis, 1, start, line, column),
            b'{' => self.single(Symbol::LeftBrace, 1, start, line, column),
            b'}' => self.single(Symbol::RightBrace, 1, start, line, column),
            b',' => self.single(Symbol::Comma, 1, start, line, column),
            b'.' => self.single(Symbol::Dot, 1, start, line, column),
            b'|' => self.single(Symbol::Pipe, 1, start, line, column),
            b'<' => {
                if self.match_bytes(b"<=") {
                    self.single(Symbol::LessEqual, 2, start, line, column)
                } else {
                    self.single(Symbol::Less, 1, start, line, column)
                }
            }
            b'>' => {
                if self.match_bytes(b">=") {
                    self.single(Symbol::GreaterEqual, 2, start, line, column)
                } else {
                    self.single(Symbol::Greater, 1, start, line, column)
                }
            }
            b':' => {
                if self.match_bytes(b"::") {
                    self.single(Symbol::DoubleColon, 2, start, line, column)
                } else if self.match_bytes(b":=") {
                    self.single(Symbol::Assign, 2, start, line, column)
                } else {
                    self.single(Symbol::Colon, 1, start, line, column)
                }
            }
            b'-' => {
                if self.match_bytes(b"->") {
                    self.single(Symbol::Arrow, 2, start, line, column)
                } else {
                    self.single(Symbol::Minus, 1, start, line, column)
                }
            }
            b'+' => self.single(Symbol::Plus, 1, start, line, column),
            b'*' => self.single(Symbol::Star, 1, start, line, column),
            b'/' => self.single(Symbol::Slash, 1, start, line, column),
            b'!' => {
                if self.match_bytes(b"!=") {
                    self.single(Symbol::BangEqual, 2, start, line, column)
                } else {
                    let message = format!("unexpected character '{}'", self.peek_char());
                    self.advance();
                    self.error_token(message, start, line, column)
                }
            }
            b'=' => {
                if self.match_bytes(b"==") {
                    self.single(Symbol::EqualEqual, 2, start, line, column)
                } else if self.match_bytes(b"=>") {
                    self.single(Symbol::FatArrow, 2, start, line, column)
                } else {
                    self.single(Symbol::Equal, 1, start, line, column)
                }
            }
            b'"' => self.lex_string(start, line, column),
            b'0'..=b'9' => self.lex_integer(start, line, column),
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.lex_identifier(start, line, column),
            _ => {
                let message = format!("unexpected character '{}'", self.peek_char());
                self.advance();
                self.error_token(message, start, line, column)
            }
        }
    }

    fn single(
        &mut self,
        symbol: Symbol,
        length: usize,
        start: usize,
        line: usize,
        column: usize,
    ) -> Token {
        self.advance_by(length);
        Token {
            kind: TokenKind::Symbol(symbol),
            span: Span {
                start,
                end: start + length,
                line,
                column,
            },
        }
    }

    fn lex_string(&mut self, start: usize, line: usize, column: usize) -> Token {
        self.advance();
        let content_start = self.index;
        while self.index < self.bytes.len() {
            let byte = self.peek_byte();
            if byte == b'"' {
                let content = &self.source[content_start..self.index];
                self.advance();
                return Token {
                    kind: TokenKind::StringLiteral(content.to_string()),
                    span: Span {
                        start,
                        end: self.index,
                        line,
                        column,
                    },
                };
            }
            if byte == b'\n' {
                break;
            }
            self.advance();
        }

        self.diagnostics.push(Diagnostic::new(
            "unterminated string literal",
            Span {
                start,
                end: self.index,
                line,
                column,
            },
        ));
        Token {
            kind: TokenKind::Error("unterminated string literal".to_string()),
            span: Span {
                start,
                end: self.index,
                line,
                column,
            },
        }
    }

    fn lex_integer(&mut self, start: usize, line: usize, column: usize) -> Token {
        while self.index < self.bytes.len() {
            match self.peek_byte() {
                b'0'..=b'9' => self.advance(),
                _ => break,
            }
        }
        let text = &self.source[start..self.index];
        let value = text.parse::<i64>();
        if let Ok(value) = value {
            Token {
                kind: TokenKind::IntegerLiteral(value),
                span: Span {
                    start,
                    end: self.index,
                    line,
                    column,
                },
            }
        } else {
            self.diagnostics.push(Diagnostic::new(
                "integer literal out of range",
                Span {
                    start,
                    end: self.index,
                    line,
                    column,
                },
            ));
            Token {
                kind: TokenKind::Error("integer literal out of range".to_string()),
                span: Span {
                    start,
                    end: self.index,
                    line,
                    column,
                },
            }
        }
    }

    fn lex_identifier(&mut self, start: usize, line: usize, column: usize) -> Token {
        while self.index < self.bytes.len() {
            match self.peek_byte() {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' => self.advance(),
                _ => break,
            }
        }
        let text = &self.source[start..self.index];
        let kind = match text {
            "public" => TokenKind::Keyword(Keyword::Public),
            "type" => TokenKind::Keyword(Keyword::Type),
            "function" => TokenKind::Keyword(Keyword::Function),
            "return" => TokenKind::Keyword(Keyword::Return),
            "abort" => TokenKind::Keyword(Keyword::Abort),
            "break" => TokenKind::Keyword(Keyword::Break),
            "continue" => TokenKind::Keyword(Keyword::Continue),
            "if" => TokenKind::Keyword(Keyword::If),
            "for" => TokenKind::Keyword(Keyword::For),
            "else" => TokenKind::Keyword(Keyword::Else),
            "match" => TokenKind::Keyword(Keyword::Match),
            "and" => TokenKind::Keyword(Keyword::And),
            "or" => TokenKind::Keyword(Keyword::Or),
            "not" => TokenKind::Keyword(Keyword::Not),
            "nil" => TokenKind::Keyword(Keyword::Nil),
            "mut" => TokenKind::Keyword(Keyword::Mut),
            "struct" => TokenKind::Keyword(Keyword::Struct),
            "matches" => TokenKind::Keyword(Keyword::Matches),
            "true" => TokenKind::BooleanLiteral(true),
            "false" => TokenKind::BooleanLiteral(false),
            _ => TokenKind::Identifier(text.to_string()),
        };
        Token {
            kind,
            span: Span {
                start,
                end: self.index,
                line,
                column,
            },
        }
    }

    fn error_token(
        &mut self,
        message: impl Into<String>,
        start: usize,
        line: usize,
        column: usize,
    ) -> Token {
        let message = message.into();
        self.diagnostics.push(Diagnostic::new(
            message.clone(),
            Span {
                start,
                end: self.index,
                line,
                column,
            },
        ));
        Token {
            kind: TokenKind::Error(message),
            span: Span {
                start,
                end: self.index,
                line,
                column,
            },
        }
    }

    fn skip_whitespace(&mut self) {
        while self.index < self.bytes.len() {
            match self.peek_byte() {
                b' ' | b'\t' | b'\r' => {
                    self.advance();
                }
                b'/' => {
                    if self.match_bytes(b"//") {
                        self.advance_by(2);
                        while self.index < self.bytes.len() && self.peek_byte() != b'\n' {
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
        if self.index < self.bytes.len() {
            if self.bytes[self.index] == b'\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
            self.index += 1;
        }
    }

    fn advance_by(&mut self, n: usize) {
        for _ in 0..n {
            self.advance();
        }
    }

    fn peek_byte(&self) -> u8 {
        self.bytes[self.index]
    }

    fn peek_char(&self) -> char {
        self.bytes[self.index] as char
    }

    fn match_bytes(&self, bytes: &[u8]) -> bool {
        self.bytes.get(self.index..self.index + bytes.len()) == Some(bytes)
    }
}

fn normalize_newlines_to_statement_terminators(tokens: Vec<Token>) -> Vec<Token> {
    let mut output = Vec::with_capacity(tokens.len());
    let mut saw_newline = false;
    let mut parenthesis_depth = 0usize;
    let mut previous_significant_token: Option<Token> = None;

    for token in tokens {
        if matches!(token.kind, TokenKind::Newline) {
            saw_newline = true;
            continue;
        }

        if saw_newline {
            if parenthesis_depth == 0
                && let Some(previous_token) = previous_significant_token.as_ref()
                && is_statement_terminator_trigger(&previous_token.kind)
                && is_statement_start(&token.kind)
            {
                let span = previous_token.span.clone();
                output.push(Token {
                    kind: TokenKind::StatementTerminator,
                    span,
                });
            }
            saw_newline = false;
        }

        update_parenthesis_depth(&token.kind, &mut parenthesis_depth);
        if !matches!(
            token.kind,
            TokenKind::StatementTerminator | TokenKind::EndOfFile
        ) {
            previous_significant_token = Some(token.clone());
        }
        output.push(token);
    }

    output
}

fn update_parenthesis_depth(kind: &TokenKind, parenthesis_depth: &mut usize) {
    match kind {
        TokenKind::Symbol(Symbol::LeftParenthesis) => {
            *parenthesis_depth = parenthesis_depth.saturating_add(1);
        }
        TokenKind::Symbol(Symbol::RightParenthesis) => {
            *parenthesis_depth = parenthesis_depth.saturating_sub(1);
        }
        _ => {}
    }
}

fn is_statement_terminator_trigger(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Identifier(_)
            | TokenKind::IntegerLiteral(_)
            | TokenKind::StringLiteral(_)
            | TokenKind::BooleanLiteral(_)
            | TokenKind::Symbol(Symbol::RightParenthesis | Symbol::RightBrace)
            | TokenKind::Keyword(Keyword::Return | Keyword::Break | Keyword::Continue)
    )
}

fn is_statement_start(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Identifier(_)
            | TokenKind::Keyword(
                Keyword::Return
                    | Keyword::Abort
                    | Keyword::Break
                    | Keyword::Continue
                    | Keyword::If
                    | Keyword::For
                    | Keyword::Mut
                    | Keyword::Match
            )
    )
}
