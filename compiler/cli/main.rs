use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 || args[1] != "check" {
        eprintln!("usage: lang0c check <file.lang>");
        process::exit(2);
    }

    let path = &args[2];
    let src = match fs::read_to_string(path) {
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
                    print_diag(path, &src, &d.message, &d.span);
                }
                process::exit(1);
            }
        }
        Err(diags) => {
            for d in diags {
                print_diag(path, &src, &d.message, &d.span);
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
