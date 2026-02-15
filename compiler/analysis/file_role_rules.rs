use compiler__diagnostics::Diagnostic;
use compiler__syntax::ParsedFile;

/// Run file-role policy checks.
///
/// Every check that requires knowledge of file role belongs here.
/// `type_checker` is reserved for role-agnostic type semantics only.
pub fn check_file_role_rules(_file: &ParsedFile, _diagnostics: &mut Vec<Diagnostic>) {
    // TODO: Implement role-specific rules:
    // package manifest can only have exports / non-manifest cannot have exports
    // binary/test cannot have public
    // binary must have main with required signature
    // library/test cannot have main
}
