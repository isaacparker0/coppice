use std::process;

use clap::{Parser, Subcommand};
use serde::Serialize;

use compiler__driver::check_target_with_workspace_root;
use compiler__reports::{CompilerFailure, CompilerFailureKind, RenderedDiagnostic, ReportFormat};

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
}

fn main() {
    let command_line = CommandLine::parse();
    let workspace_root = command_line.workspace_root.as_deref();
    let (path, report_format) = match command_line.command {
        Command::Check { path, format } => (path.unwrap_or_else(|| ".".to_string()), format),
    };

    match check_target_with_workspace_root(&path, workspace_root) {
        Ok(checked_target) => match report_format {
            ReportFormat::Text => {
                if checked_target.diagnostics.is_empty() {
                    println!("ok");
                } else {
                    for diagnostic in &checked_target.diagnostics {
                        let source = checked_target
                            .source_by_path
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
                    process::exit(1);
                }
            }
            ReportFormat::Json => {
                let output = JsonOutput {
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
                    if matches!(error.kind, CompilerFailureKind::WorkspaceDiscoveryFailed)
                        && !error.details.is_empty()
                    {
                        for detail in &error.details {
                            let detail_path = detail.path.as_deref().unwrap_or(&path);
                            eprintln!("{detail_path}: error: {}", detail.message);
                        }
                    } else {
                        let error_path = error.path.as_deref().unwrap_or(&path);
                        eprintln!("{error_path}: error: {}", error.message);
                        for detail in &error.details {
                            let detail_path = detail.path.as_deref().unwrap_or(error_path);
                            eprintln!("{detail_path}: error: {}", detail.message);
                        }
                    }
                }
                ReportFormat::Json => {
                    let output = JsonOutput {
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

#[derive(Serialize)]
struct JsonOutput {
    ok: bool,
    diagnostics: Vec<RenderedDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<CompilerFailure>,
}
