use std::fs;
use std::process;

use clap::{Parser, Subcommand};

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
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("{path}: error: {error}");
            process::exit(1);
        }
    };

    match compiler__frontend::parse_file(&source) {
        Ok(file) => {
            let diagnostics = compiler__middle::check_file(&file);
            if diagnostics.is_empty() {
                println!("ok");
            } else {
                for diagnostic in diagnostics {
                    print_diagnostic(&path, &source, &diagnostic.message, &diagnostic.span);
                }
                process::exit(1);
            }
        }
        Err(diagnostics) => {
            for diagnostic in diagnostics {
                print_diagnostic(&path, &source, &diagnostic.message, &diagnostic.span);
            }
            process::exit(1);
        }
    }
}

fn print_diagnostic(path: &str, source: &str, message: &str, span: &compiler__frontend::Span) {
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
