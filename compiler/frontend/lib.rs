mod ast;
mod diagnostics;
mod lexer;
mod parser;

pub use ast::*;
pub use diagnostics::{Diagnostic, Span};

pub fn parse_file(src: &str) -> Result<File, Vec<Diagnostic>> {
    let mut lex = lexer::Lexer::new(src);
    let tokens = lex.lex_all();
    let mut diags = lex.into_diagnostics();

    let mut parser = parser::Parser::new(tokens);
    let file = parser.parse_file();
    diags.extend(parser.into_diagnostics());

    if diags.is_empty() {
        Ok(file)
    } else {
        Err(diags)
    }
}
