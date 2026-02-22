use std::fs;
use std::path::Path;

use crate::{BuiltInToolContext, FormatMode, FormatterOutcome};

pub(crate) fn normalize_line_endings(context: BuiltInToolContext<'_>) -> FormatterOutcome {
    let mut files_with_invalid_line_endings = Vec::new();

    for relative_path in context.files {
        let full_path = Path::new(context.workspace_directory).join(relative_path);
        let file_bytes = match fs::read(&full_path) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("error: failed to read {relative_path}: {error}");
                return FormatterOutcome::Failure;
            }
        };

        let Ok(file_contents) = String::from_utf8(file_bytes) else {
            continue;
        };

        if !file_contents.contains('\r') {
            continue;
        }

        match context.mode {
            FormatMode::Check => files_with_invalid_line_endings.push(relative_path.clone()),
            FormatMode::Fix => {
                let normalized_file_contents =
                    file_contents.replace("\r\n", "\n").replace('\r', "\n");
                if let Err(error) = fs::write(&full_path, normalized_file_contents) {
                    eprintln!("error: failed to write {relative_path}: {error}");
                    return FormatterOutcome::Failure;
                }
            }
        }
    }

    if files_with_invalid_line_endings.is_empty() {
        return FormatterOutcome::Success;
    }

    eprintln!("Files with non-LF line endings:");
    for relative_path in &files_with_invalid_line_endings {
        eprintln!("  {relative_path}");
    }
    FormatterOutcome::Failure
}
