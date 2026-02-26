#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutofixPolicyMode {
    NonStrict,
    Strict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingSafeAutofixSummary {
    pub file_count: usize,
    pub edit_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutofixPolicyOutcome {
    NoPendingSafeAutofixes,
    WarnInNonStrictMode { summary: PendingSafeAutofixSummary },
    FailInStrictMode { summary: PendingSafeAutofixSummary },
}

#[must_use]
pub fn summarize_pending_safe_autofixes<I>(
    safe_autofix_edit_count_by_file: I,
) -> PendingSafeAutofixSummary
where
    I: IntoIterator<Item = usize>,
{
    let mut file_count = 0usize;
    let mut edit_count = 0usize;
    for safe_autofix_edit_count in safe_autofix_edit_count_by_file {
        file_count += 1;
        edit_count += safe_autofix_edit_count;
    }
    PendingSafeAutofixSummary {
        file_count,
        edit_count,
    }
}

#[must_use]
pub fn evaluate_autofix_policy(
    mode: AutofixPolicyMode,
    summary: PendingSafeAutofixSummary,
) -> AutofixPolicyOutcome {
    if summary.file_count == 0 {
        return AutofixPolicyOutcome::NoPendingSafeAutofixes;
    }
    match mode {
        AutofixPolicyMode::NonStrict => AutofixPolicyOutcome::WarnInNonStrictMode { summary },
        AutofixPolicyMode::Strict => AutofixPolicyOutcome::FailInStrictMode { summary },
    }
}
