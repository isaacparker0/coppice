mod lexer;
mod parser;

use compiler__diagnostics::Diagnostic;
use compiler__phase_results::{PhaseOutput, PhaseStatus};
use compiler__source::FileRole;
use compiler__syntax::ParsedFile;

#[must_use]
pub fn parse_file(source: &str, role: FileRole) -> PhaseOutput<ParsedFile> {
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

    let status = if diagnostics.is_empty() {
        PhaseStatus::Ok
    } else {
        PhaseStatus::PreventsDownstreamExecution
    };

    PhaseOutput {
        value: file,
        diagnostics,
        status,
    }
}
