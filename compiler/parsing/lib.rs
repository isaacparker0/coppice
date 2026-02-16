mod lexer;
mod parser;

use compiler__diagnostics::Diagnostic;
use compiler__source::FileRole;
use compiler__syntax::ParsedFile;

pub fn parse_file(source: &str, role: FileRole) -> Result<ParsedFile, Vec<Diagnostic>> {
    let mut lexer = lexer::Lexer::new(source);
    let tokens = lexer.lex_all_tokens();
    let mut diagnostics: Vec<Diagnostic> = lexer
        .into_errors()
        .into_iter()
        .map(|lex_error| Diagnostic::new(lex_error.message, lex_error.span))
        .collect();

    let mut parser = parser::Parser::new(tokens);
    let file = parser.parse_file_tokens(role);
    diagnostics.extend(parser.into_diagnostics());

    if diagnostics.is_empty() {
        Ok(file)
    } else {
        Err(diagnostics)
    }
}
