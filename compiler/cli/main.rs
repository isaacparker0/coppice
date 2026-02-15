use std::process;

use clap::{Parser, Subcommand};

use compiler__driver::{CheckFileError, check_file};
use compiler__source::Span;

#[derive(Parser)]
#[command(version)]
struct CommandLine {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Check { file: String },
}

fn main() {
    let command_line = CommandLine::parse();
    let path = match command_line.command {
        Command::Check { file } => file,
    };

    match check_file(&path) {
        Ok(checked_file) => {
            if checked_file.diagnostics.is_empty() {
                println!("ok");
            } else {
                for diagnostic in checked_file.diagnostics {
                    print_diagnostic(
                        &path,
                        &checked_file.source,
                        &diagnostic.message,
                        &diagnostic.span,
                    );
                }
                process::exit(1);
            }
        }
        Err(CheckFileError::ReadSource(error)) => {
            eprintln!("{path}: error: {error}");
            process::exit(1);
        }
        Err(CheckFileError::InvalidSourceFileExtension) => {
            eprintln!("{path}: error: expected a .coppice source file");
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
