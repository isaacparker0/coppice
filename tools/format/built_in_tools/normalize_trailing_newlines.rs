use std::fs;
use std::path::Path;

use crate::{BuiltInToolContext, FormatMode, FormatterOutcome};

pub(crate) fn normalize_trailing_newlines(context: BuiltInToolContext<'_>) -> FormatterOutcome {
    let mut files_with_invalid_trailing_newline = Vec::new();

    for relative_path in context.files {
        let full_path = Path::new(context.workspace_directory).join(relative_path);
        let file_bytes = match fs::read(&full_path) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("error: failed to read {relative_path}: {error}");
                return FormatterOutcome::Failure;
            }
        };

        if file_bytes.is_empty() {
            continue;
        }

        let Ok(file_contents) = String::from_utf8(file_bytes) else {
            continue;
        };

        let has_exactly_one_trailing_newline = file_contents.ends_with('\n')
            && !file_contents[..file_contents.len() - 1].ends_with('\n');
        if has_exactly_one_trailing_newline {
            continue;
        }

        match context.mode {
            FormatMode::Check => files_with_invalid_trailing_newline.push(relative_path.clone()),
            FormatMode::Fix => {
                let normalized_file_contents =
                    format!("{}\n", file_contents.trim_end_matches('\n'));
                if let Err(error) = fs::write(&full_path, normalized_file_contents) {
                    eprintln!("error: failed to write {relative_path}: {error}");
                    return FormatterOutcome::Failure;
                }
            }
        }
    }

    if files_with_invalid_trailing_newline.is_empty() {
        return FormatterOutcome::Success;
    }

    eprintln!("Files without exactly one trailing newline:");
    for relative_path in &files_with_invalid_trailing_newline {
        eprintln!("  {relative_path}");
    }
    FormatterOutcome::Failure
}
