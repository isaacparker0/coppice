use compiler__source::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Keyword {
    // keep-sorted start
    Abort,
    And,
    As,
    Break,
    Continue,
    Else,
    Enum,
    Exports,
    For,
    Function,
    If,
    Import,
    Match,
    Matches,
    Mut,
    Nil,
    Not,
    Or,
    Print,
    Public,
    Return,
    Struct,
    Type,
    Visible,
    // keep-sorted end
}

impl Keyword {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            // keep-sorted start
            Keyword::Abort => "abort",
            Keyword::And => "and",
            Keyword::As => "as",
            Keyword::Break => "break",
            Keyword::Continue => "continue",
            Keyword::Else => "else",
            Keyword::Enum => "enum",
            Keyword::Exports => "exports",
            Keyword::For => "for",
            Keyword::Function => "function",
            Keyword::If => "if",
            Keyword::Import => "import",
            Keyword::Match => "match",
            Keyword::Matches => "matches",
            Keyword::Mut => "mut",
            Keyword::Nil => "nil",
            Keyword::Not => "not",
            Keyword::Or => "or",
            Keyword::Print => "print",
            Keyword::Public => "public",
            Keyword::Return => "return",
            Keyword::Struct => "struct",
            Keyword::Type => "type",
            Keyword::Visible => "visible",
            // keep-sorted end
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Symbol {
    // keep-sorted start
    Arrow,
    Assign,
    BangEqual,
    Colon,
    Comma,
    Dot,
    DoubleColon,
    Equal,
    EqualEqual,
    FatArrow,
    Greater,
    GreaterEqual,
    LeftBrace,
    LeftBracket,
    LeftParenthesis,
    Less,
    LessEqual,
    Minus,
    Pipe,
    Plus,
    RightBrace,
    RightBracket,
    RightParenthesis,
    Slash,
    Star,
    // keep-sorted end
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TokenKind {
    Identifier(String),
    IntegerLiteral(i64),
    StringLiteral(String),
    BooleanLiteral(bool),
    DocComment(String),
    Keyword(Keyword),
    Symbol(Symbol),
    /// Raw `\n` from the source. These are removed during normalization.
    Newline,
    /// Semantic statement terminator inserted during newline normalization.
    StatementTerminator,
    EndOfFile,
    Error,
}

#[derive(Clone, Debug)]
pub(crate) struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

pub(crate) struct LexError {
    pub(crate) message: String,
    pub(crate) span: Span,
}

pub(crate) struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    index: usize,
    line: usize,
    column: usize,
    lex_errors: Vec<LexError>,
}

impl<'a> Lexer<'a> {
    pub(crate) fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            index: 0,
            line: 1,
            column: 1,
            lex_errors: Vec::new(),
        }
    }

    pub(crate) fn lex_all_tokens(&mut self) -> Vec<Token> {
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

    pub(crate) fn into_errors(self) -> Vec<LexError> {
        self.lex_errors
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
            b'[' => self.single(Symbol::LeftBracket, 1, start, line, column),
            b']' => self.single(Symbol::RightBracket, 1, start, line, column),
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
            b'/' => {
                if self.match_bytes(b"///") {
                    self.lex_doc_comment(start, line, column)
                } else {
                    self.single(Symbol::Slash, 1, start, line, column)
                }
            }
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

        self.lex_errors.push(LexError {
            message: "unterminated string literal".to_string(),
            span: Span {
                start,
                end: self.index,
                line,
                column,
            },
        });
        Token {
            kind: TokenKind::Error,
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
            self.lex_errors.push(LexError {
                message: "integer literal out of range".to_string(),
                span: Span {
                    start,
                    end: self.index,
                    line,
                    column,
                },
            });
            Token {
                kind: TokenKind::Error,
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
            "visible" => TokenKind::Keyword(Keyword::Visible),
            "function" => TokenKind::Keyword(Keyword::Function),
            "return" => TokenKind::Keyword(Keyword::Return),
            "abort" => TokenKind::Keyword(Keyword::Abort),
            "break" => TokenKind::Keyword(Keyword::Break),
            "continue" => TokenKind::Keyword(Keyword::Continue),
            "if" => TokenKind::Keyword(Keyword::If),
            "for" => TokenKind::Keyword(Keyword::For),
            "else" => TokenKind::Keyword(Keyword::Else),
            "enum" => TokenKind::Keyword(Keyword::Enum),
            "exports" => TokenKind::Keyword(Keyword::Exports),
            "import" => TokenKind::Keyword(Keyword::Import),
            "as" => TokenKind::Keyword(Keyword::As),
            "match" => TokenKind::Keyword(Keyword::Match),
            "and" => TokenKind::Keyword(Keyword::And),
            "or" => TokenKind::Keyword(Keyword::Or),
            "print" => TokenKind::Keyword(Keyword::Print),
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

    fn lex_doc_comment(&mut self, start: usize, line: usize, column: usize) -> Token {
        self.advance_by(3);
        let content_start = self.index;
        while self.index < self.bytes.len() && self.peek_byte() != b'\n' {
            self.advance();
        }
        let mut text = self.source[content_start..self.index].to_string();
        if text.starts_with(' ') {
            text.remove(0);
        }
        Token {
            kind: TokenKind::DocComment(text),
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
        self.lex_errors.push(LexError {
            message: message.clone(),
            span: Span {
                start,
                end: self.index,
                line,
                column,
            },
        });
        Token {
            kind: TokenKind::Error,
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
                    if self.match_bytes(b"///") {
                        break;
                    }
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
        TokenKind::Symbol(Symbol::LeftParenthesis | Symbol::LeftBracket) => {
            *parenthesis_depth = parenthesis_depth.saturating_add(1);
        }
        TokenKind::Symbol(Symbol::RightParenthesis | Symbol::RightBracket) => {
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
            | TokenKind::Keyword(
                Keyword::Return | Keyword::Break | Keyword::Continue | Keyword::Print
            )
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
                    | Keyword::Print
                    | Keyword::Exports
                    | Keyword::Import
            )
    )
}
