use std::process;

use clap::{Parser, Subcommand};

use compiler__driver::{CheckFileError, check_target_with_workspace_root};
use compiler__source::Span;

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
    Check { path: Option<String> },
}

fn main() {
    let command_line = CommandLine::parse();
    let workspace_root = command_line.workspace_root.as_deref();
    let path = match command_line.command {
        Command::Check { path } => path.unwrap_or_else(|| ".".to_string()),
    };

    match check_target_with_workspace_root(&path, workspace_root) {
        Ok(checked_target) => {
            if checked_target.diagnostics.is_empty() {
                println!("ok");
            } else {
                for diagnostic in checked_target.diagnostics {
                    print_diagnostic(
                        &diagnostic.path,
                        &diagnostic.source,
                        &diagnostic.message,
                        &diagnostic.span,
                    );
                }
                process::exit(1);
            }
        }
        Err(CheckFileError::ReadSource { path, error }) => {
            eprintln!("{path}: error: {error}");
            process::exit(1);
        }
        Err(CheckFileError::InvalidWorkspaceRoot { path, error }) => {
            eprintln!("{path}: error: invalid workspace root: {error}");
            process::exit(1);
        }
        Err(CheckFileError::WorkspaceRootNotDirectory { path }) => {
            eprintln!("{path}: error: workspace root must be a directory");
            process::exit(1);
        }
        Err(CheckFileError::WorkspaceRootMissingManifest { path }) => {
            eprintln!("{path}: error: not a Coppice workspace root (missing PACKAGE.coppice)");
            process::exit(1);
        }
        Err(CheckFileError::InvalidCheckTarget) => {
            eprintln!("{path}: error: expected a file or directory path");
            process::exit(1);
        }
        Err(CheckFileError::TargetOutsideWorkspace) => {
            eprintln!("{path}: error: target is outside the current workspace root");
            process::exit(1);
        }
        Err(CheckFileError::PackageNotFound) => {
            eprintln!("{path}: error: target is not inside a package (missing PACKAGE.coppice)");
            process::exit(1);
        }
        Err(CheckFileError::WorkspaceDiscoveryFailed(errors)) => {
            for error in errors {
                if let Some(error_path) = error.path {
                    eprintln!("{}: error: {}", error_path.display(), error.message);
                } else {
                    eprintln!("{path}: error: {}", error.message);
                }
            }
            process::exit(1);
        }
    }
}

fn print_diagnostic(path: &str, source: &str, message: &str, span: &Span) {
    let line = span.line;
    let column = span.column;
    let line_text = source.lines().nth(line - 1).unwrap_or("");
    eprintln!("{path}:{line}:{column}: error: {message}");
    eprintln!("  {line_text}");
    if !line_text.is_empty() {
        let caret = " ".repeat(column.saturating_sub(1));
        eprintln!("  {caret}^");
    }
}
