mod lexer;
mod parser;

use compiler__diagnostics::Diagnostic;
use compiler__syntax::LibraryFile;

pub fn parse_library_file(source: &str) -> Result<LibraryFile, Vec<Diagnostic>> {
    let mut lexer = lexer::Lexer::new(source);
    let tokens = lexer.lex_all_tokens();
    let mut diagnostics = lexer.into_diagnostics();

    let mut parser = parser::Parser::new(tokens);
    let file = parser.parse_library_file();
    diagnostics.extend(parser.into_diagnostics());

    if diagnostics.is_empty() {
        Ok(file)
    } else {
        Err(diagnostics)
    }
}
