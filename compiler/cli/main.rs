use std::{fs, process};

use clap::{Parser, Subcommand};

use compiler__autofix_policy::{
    AutofixPolicyMode, AutofixPolicyOutcome, evaluate_autofix_policy,
    summarize_pending_safe_autofixes,
};
use compiler__check_pipeline::{
    analyze_target_with_workspace_root, check_target_with_workspace_root,
};
use compiler__driver::{build_target_with_workspace_root, run_target_with_workspace_root};
use compiler__lsp::run_lsp_stdio;
use compiler__reports::{
    CompilerCheckJsonOutput, CompilerCheckSafeFix, CompilerFailure, CompilerFailureKind,
    RenderedDiagnostic, ReportFormat,
};

#[derive(Parser)]
#[command(version)]
struct CommandLine {
    #[arg(long, global = true)]
    workspace_root: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Check {
        path: Option<String>,
        #[arg(long, default_value_t = ReportFormat::Text)]
        format: ReportFormat,
        #[arg(long)]
        strict: bool,
    },
    Build {
        path: String,
        #[arg(long)]
        output_dir: Option<String>,
        #[arg(long)]
        strict: bool,
    },
    Fix {
        path: Option<String>,
    },
    Run {
        path: String,
        #[arg(long)]
        output_dir: Option<String>,
        #[arg(long)]
        strict: bool,
    },
    Lsp {
        #[arg(long)]
        stdio: bool,
    },
}

fn main() {
    let command_line = CommandLine::parse();
    let workspace_root = command_line.workspace_root.as_deref();
    match command_line.command {
        Command::Check {
            path,
            format,
            strict,
        } => {
            let path = path.unwrap_or_else(|| ".".to_string());
            run_check(&path, workspace_root, format, strict);
        }
        Command::Build {
            path,
            output_dir,
            strict,
        } => {
            let build_result = build_target_with_workspace_root(
                &path,
                workspace_root,
                output_dir.as_deref(),
                strict,
            );
            if matches!(
                build_result.autofix_policy_outcome,
                Some(AutofixPolicyOutcome::WarnInNonStrictMode { .. })
            ) {
                render_safe_fix_warning();
            }
            match build_result.build {
                Ok(built_target) => println!("{}", built_target.executable_path),
                Err(error) => {
                    render_compiler_failure_text(&path, &error);
                    process::exit(1);
                }
            }
        }
        Command::Fix { path } => {
            let path = path.unwrap_or_else(|| ".".to_string());
            run_fix(&path, workspace_root);
        }
        Command::Run {
            path,
            output_dir,
            strict,
        } => {
            let run_result = run_target_with_workspace_root(
                &path,
                workspace_root,
                output_dir.as_deref(),
                strict,
            );
            if matches!(
                run_result.autofix_policy_outcome,
                Some(AutofixPolicyOutcome::WarnInNonStrictMode { .. })
            ) {
                render_safe_fix_warning();
            }
            match run_result.run {
                Ok(exit_code) => {
                    if exit_code != 0 {
                        process::exit(exit_code);
                    }
                }
                Err(error) => {
                    render_compiler_failure_text(&path, &error);
                    process::exit(1);
                }
            }
        }
        Command::Lsp { stdio } => {
            run_lsp(workspace_root, stdio);
        }
    }
}

fn run_fix(path: &str, workspace_root: Option<&str>) {
    let analyzed_target = match analyze_target_with_workspace_root(path, workspace_root) {
        Ok(value) => value,
        Err(error) => {
            render_compiler_failure_text(path, &error);
            process::exit(1);
        }
    };

    let mut updated_file_count = 0usize;
    for (workspace_relative_path, canonical_source_text) in
        &analyzed_target.canonical_source_override_by_workspace_relative_path
    {
        let absolute_path = analyzed_target.workspace_root.join(workspace_relative_path);
        if let Err(error) = fs::write(&absolute_path, canonical_source_text) {
            let compiler_failure = CompilerFailure {
                kind: CompilerFailureKind::WriteSource,
                message: error.to_string(),
                path: Some(absolute_path.display().to_string()),
                details: Vec::new(),
            };
            render_compiler_failure_text(path, &compiler_failure);
            process::exit(1);
        }
        updated_file_count += 1;
    }

    if updated_file_count == 0 {
        println!("no fixes applied");
    } else {
        println!("applied fixes to {updated_file_count} files");
    }
}

fn run_check(path: &str, workspace_root: Option<&str>, report_format: ReportFormat, strict: bool) {
    match check_target_with_workspace_root(path, workspace_root) {
        Ok(checked_target) => {
            let safe_fixes =
                safe_fix_summaries(&checked_target.safe_autofix_edits_by_workspace_relative_path);
            let autofix_policy_outcome = evaluate_autofix_policy_for_target(
                strict,
                &checked_target.safe_autofix_edits_by_workspace_relative_path,
            );
            let strict_policy_failure = matches!(
                autofix_policy_outcome,
                AutofixPolicyOutcome::FailInStrictMode { .. }
            ) && checked_target.diagnostics.is_empty();
            let strict_policy_error = strict_policy_failure.then(|| CompilerFailure {
                kind: CompilerFailureKind::CheckFailed,
                message: "check failed due to pending safe autofixes".to_string(),
                path: Some(path.to_string()),
                details: safe_fixes
                    .iter()
                    .map(|safe_fix| compiler__reports::CompilerFailureDetail {
                        message: format!("{} pending safe autofix edits", safe_fix.edit_count),
                        path: Some(safe_fix.path.clone()),
                    })
                    .collect(),
            });

            match report_format {
                ReportFormat::Text => {
                    if !checked_target.diagnostics.is_empty() {
                        render_diagnostics_text(
                            &checked_target.diagnostics,
                            &checked_target.source_by_path,
                        );
                    } else if let Some(error) = &strict_policy_error {
                        render_compiler_failure_text(path, error);
                    } else {
                        println!("ok");
                    }
                    if matches!(
                        autofix_policy_outcome,
                        AutofixPolicyOutcome::WarnInNonStrictMode { .. }
                    ) {
                        render_safe_fix_warning();
                    }
                    if !checked_target.diagnostics.is_empty() || strict_policy_failure {
                        process::exit(1);
                    }
                }
                ReportFormat::Json => {
                    let output = CompilerCheckJsonOutput {
                        ok: checked_target.diagnostics.is_empty() && !strict_policy_failure,
                        diagnostics: checked_target.diagnostics.clone(),
                        safe_fixes: safe_fixes.clone(),
                        error: strict_policy_error,
                    };
                    println!("{}", serde_json::to_string_pretty(&output).unwrap());
                    if !checked_target.diagnostics.is_empty() || strict_policy_failure {
                        process::exit(1);
                    }
                }
            }
        }
        Err(error) => {
            match report_format {
                ReportFormat::Text => {
                    render_compiler_failure_text(path, &error);
                }
                ReportFormat::Json => {
                    let output = CompilerCheckJsonOutput {
                        ok: false,
                        diagnostics: Vec::new(),
                        safe_fixes: Vec::new(),
                        error: Some(error),
                    };
                    println!("{}", serde_json::to_string_pretty(&output).unwrap());
                }
            }
            process::exit(1);
        }
    }
}

fn safe_fix_summaries(
    safe_autofix_edits_by_workspace_relative_path: &std::collections::BTreeMap<
        String,
        Vec<compiler__fix_edits::TextEdit>,
    >,
) -> Vec<CompilerCheckSafeFix> {
    safe_autofix_edits_by_workspace_relative_path
        .iter()
        .map(|(path, text_edits)| CompilerCheckSafeFix {
            path: path.clone(),
            edit_count: text_edits.len(),
        })
        .collect()
}

fn render_safe_fix_warning() {
    eprintln!("warning: safe autofixes available; will fail in strict mode");
    eprintln!("run 'coppice fix' to apply");
}

fn evaluate_autofix_policy_for_target(
    strict: bool,
    safe_autofix_edits_by_workspace_relative_path: &std::collections::BTreeMap<
        String,
        Vec<compiler__fix_edits::TextEdit>,
    >,
) -> AutofixPolicyOutcome {
    evaluate_autofix_policy(
        if strict {
            AutofixPolicyMode::Strict
        } else {
            AutofixPolicyMode::NonStrict
        },
        summarize_pending_safe_autofixes(
            safe_autofix_edits_by_workspace_relative_path
                .values()
                .map(Vec::len),
        ),
    )
}

fn run_lsp(workspace_root: Option<&str>, stdio: bool) {
    if !stdio {
        eprintln!("lsp transport mode not specified; pass --stdio");
        process::exit(1);
    }
    if let Err(error) = run_lsp_stdio(workspace_root) {
        render_compiler_failure_text(".", &error);
        process::exit(1);
    }
}

fn render_diagnostics_text(
    diagnostics: &[RenderedDiagnostic],
    source_by_path: &std::collections::BTreeMap<String, String>,
) {
    for diagnostic in diagnostics {
        let source = source_by_path
            .get(&diagnostic.path)
            .map_or("", String::as_str);
        let line = diagnostic.span.line;
        let column = diagnostic.span.column;
        let line_text = source.lines().nth(line - 1).unwrap_or("");
        eprintln!(
            "{path}:{line}:{column}: error: {message}",
            path = diagnostic.path,
            message = diagnostic.message
        );
        eprintln!("  {line_text}");
        if !line_text.is_empty() {
            let caret = " ".repeat(column.saturating_sub(1));
            eprintln!("  {caret}^");
        }
    }
}

fn render_compiler_failure_text(path: &str, error: &CompilerFailure) {
    if matches!(error.kind, CompilerFailureKind::WorkspaceDiscoveryFailed)
        && !error.details.is_empty()
    {
        for detail in &error.details {
            let detail_path = detail.path.as_deref().unwrap_or(path);
            eprintln!("{detail_path}: error: {}", detail.message);
        }
        return;
    }
    let error_path = error.path.as_deref().unwrap_or(path);
    eprintln!("{error_path}: error: {}", error.message);
    for detail in &error.details {
        let detail_path = detail.path.as_deref().unwrap_or(error_path);
        eprintln!("{detail_path}: error: {}", detail.message);
    }
}
