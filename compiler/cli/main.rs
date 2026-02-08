use std::fs;
use std::process;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Check {
        file: String,
    },
}

fn main() {
    let cli = Cli::parse();
    let path = match cli.command {
        Command::Check { file } => file,
    };
    let src = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("{path}: error: {err}");
            process::exit(1);
        }
    };

    match compiler__frontend::parse_file(&src) {
        Ok(file) => {
            let diags = compiler__middle::check_file(&file);
            if diags.is_empty() {
                println!("ok");
            } else {
                for d in diags {
                    print_diag(&path, &src, &d.message, &d.span);
                }
                process::exit(1);
            }
        }
        Err(diags) => {
            for d in diags {
                print_diag(&path, &src, &d.message, &d.span);
            }
            process::exit(1);
        }
    }
}

fn print_diag(path: &str, src: &str, message: &str, span: &compiler__frontend::Span) {
    let line = span.line;
    let col = span.col;
    let line_text = src.lines().nth(line - 1).unwrap_or("");
    eprintln!("{path}:{line}:{col}: error: {message}");
    eprintln!("  {line_text}");
    if !line_text.is_empty() {
        let caret = " ".repeat(col.saturating_sub(1));
        eprintln!("  {caret}^");
    }
}
