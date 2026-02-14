mod lexer;
mod parser;

pub use compiler__syntax::*;

pub fn parse_file(source: &str) -> Result<File, Vec<Diagnostic>> {
    let mut lexer = lexer::Lexer::new(source);
    let tokens = lexer.lex_all_tokens();
    let mut diagnostics = lexer.into_diagnostics();

    let mut parser = parser::Parser::new(tokens);
    let file = parser.parse_file();
    diagnostics.extend(parser.into_diagnostics());

    if diagnostics.is_empty() {
        Ok(file)
    } else {
        Err(diagnostics)
    }
}
