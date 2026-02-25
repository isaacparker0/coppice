use std::{fs, process};

use clap::{Parser, Subcommand};

use compiler__check_pipeline::{
    analyze_target_with_workspace_root, check_target_with_workspace_root,
};
use compiler__driver::{build_target_with_workspace_root, run_target_with_workspace_root};
use compiler__lsp::run_lsp_stdio;
use compiler__reports::{
    CompilerCheckJsonOutput, CompilerFailure, CompilerFailureKind, RenderedDiagnostic, ReportFormat,
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
    },
    Build {
        path: String,
        #[arg(long)]
        output_dir: Option<String>,
    },
    Fix {
        path: Option<String>,
    },
    Run {
        path: String,
        #[arg(long)]
        output_dir: Option<String>,
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
        Command::Check { path, format } => {
            let path = path.unwrap_or_else(|| ".".to_string());
            run_check(&path, workspace_root, format);
        }
        Command::Build { path, output_dir } => {
            match build_target_with_workspace_root(&path, workspace_root, output_dir.as_deref()) {
                Ok(built) => println!("{}", built.executable_path),
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
        Command::Run { path, output_dir } => {
            match run_target_with_workspace_root(&path, workspace_root, output_dir.as_deref()) {
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

fn run_check(path: &str, workspace_root: Option<&str>, report_format: ReportFormat) {
    match check_target_with_workspace_root(path, workspace_root) {
        Ok(checked_target) => match report_format {
            ReportFormat::Text => {
                if checked_target.diagnostics.is_empty() {
                    println!("ok");
                } else {
                    render_diagnostics_text(
                        &checked_target.diagnostics,
                        &checked_target.source_by_path,
                    );
                    process::exit(1);
                }
            }
            ReportFormat::Json => {
                let output = CompilerCheckJsonOutput {
                    ok: checked_target.diagnostics.is_empty(),
                    diagnostics: checked_target.diagnostics.clone(),
                    error: None,
                };
                println!("{}", serde_json::to_string_pretty(&output).unwrap());
                if !checked_target.diagnostics.is_empty() {
                    process::exit(1);
                }
            }
        },
        Err(error) => {
            match report_format {
                ReportFormat::Text => {
                    render_compiler_failure_text(path, &error);
                }
                ReportFormat::Json => {
                    let output = CompilerCheckJsonOutput {
                        ok: false,
                        diagnostics: Vec::new(),
                        error: Some(error),
                    };
                    println!("{}", serde_json::to_string_pretty(&output).unwrap());
                }
            }
            process::exit(1);
        }
    }
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
