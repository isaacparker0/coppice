use compiler__check_session::CheckSession;
use compiler__reports::CompilerFailure;

pub struct LspValidationResult {
    pub diagnostic_count: usize,
}

pub fn run_lsp_validation(
    workspace_root_override: Option<&str>,
) -> Result<LspValidationResult, CompilerFailure> {
    let check_session = CheckSession::new(workspace_root_override.map(ToString::to_string));
    let checked_target = check_session.check_target(".")?;
    Ok(LspValidationResult {
        diagnostic_count: checked_target.diagnostics.len(),
    })
}
