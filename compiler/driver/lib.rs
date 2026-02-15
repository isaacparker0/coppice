use std::fs;
use std::io;
use std::path::Path;

use compiler__diagnostics::Diagnostic;
use compiler__file_role_rules as file_role_rules;
use compiler__parsing::parse_file;
use compiler__resolution as resolution;
use compiler__source::FileRole;
use compiler__typecheck as typecheck;

pub struct CheckedFile {
    pub source: String,
    pub diagnostics: Vec<Diagnostic>,
}

pub enum CheckFileError {
    ReadSource(io::Error),
    InvalidSourceFileExtension,
}

pub fn check_file(path: &str) -> Result<CheckedFile, CheckFileError> {
    let source = fs::read_to_string(path).map_err(CheckFileError::ReadSource)?;
    let Some(role) = FileRole::from_path(Path::new(path)) else {
        return Err(CheckFileError::InvalidSourceFileExtension);
    };

    let diagnostics = match parse_file(&source, role) {
        Ok(file) => {
            let mut diagnostics = Vec::new();
            file_role_rules::check_file(&file, &mut diagnostics);
            resolution::check_file(&file, &mut diagnostics);
            typecheck::check_file(&file, &mut diagnostics);
            diagnostics
        }
        Err(diagnostics) => diagnostics,
    };

    Ok(CheckedFile {
        source,
        diagnostics,
    })
}
