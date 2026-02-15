mod type_checker;
mod types;

use compiler__diagnostics::Diagnostic;
use compiler__syntax::ParsedFile;

pub fn check_file(file: &ParsedFile, diagnostics: &mut Vec<Diagnostic>) {
    type_checker::check_parsed_file(file, diagnostics);
}
