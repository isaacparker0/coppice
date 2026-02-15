mod file_role_rules;
mod type_checker;
mod types;

use compiler__diagnostics::Diagnostic;
use compiler__syntax::ParsedFile;

#[must_use]
pub fn check_file(file: &ParsedFile) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    file_role_rules::check_file_role_rules(file, &mut diagnostics);
    type_checker::check_parsed_file(file, &mut diagnostics);
    diagnostics
}
