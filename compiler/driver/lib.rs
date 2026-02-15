use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use compiler__diagnostics::Diagnostic;
use compiler__file_role_rules as file_role_rules;
use compiler__parsing::parse_file;
use compiler__resolution::{self as resolution, PackageDiagnostic, PackageFile};
use compiler__source::{FileRole, Span, compare_paths, path_to_key};
use compiler__typecheck as typecheck;
use compiler__workspace::{DiscoveryError, discover_workspace};

pub struct RenderedDiagnostic {
    pub path: String,
    pub source: String,
    pub message: String,
    pub span: Span,
}

pub struct CheckedTarget {
    pub diagnostics: Vec<RenderedDiagnostic>,
}

pub enum CheckFileError {
    ReadSource { path: String, error: io::Error },
    InvalidCheckTarget,
    PackageNotFound,
    WorkspaceDiscoveryFailed(Vec<DiscoveryError>),
}

struct ParsedUnit {
    path: PathBuf,
    source: String,
    parsed: compiler__syntax::ParsedFile,
}

pub fn check_file(path: &str) -> Result<CheckedTarget, CheckFileError> {
    let target_path = PathBuf::from(path);
    let metadata = fs::metadata(&target_path).map_err(|error| CheckFileError::ReadSource {
        path: path.to_string(),
        error,
    })?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(CheckFileError::InvalidCheckTarget);
    }
    let diagnostic_display_base = if metadata.is_dir() {
        target_path.clone()
    } else {
        target_path.parent().unwrap_or(Path::new("")).to_path_buf()
    };

    let package_root = find_package_root(&target_path).ok_or(CheckFileError::PackageNotFound)?;
    let workspace =
        discover_workspace(&package_root).map_err(CheckFileError::WorkspaceDiscoveryFailed)?;
    let package = workspace
        .packages()
        .first()
        .ok_or(CheckFileError::PackageNotFound)?;

    let mut file_entries: Vec<(PathBuf, FileRole)> = Vec::new();
    file_entries.push((package.manifest_path.clone(), FileRole::PackageManifest));
    for source_file in &package.source_files {
        file_entries.push((
            source_file.workspace_relative_path.clone(),
            source_file.role,
        ));
    }
    file_entries.sort_by(|left, right| compare_paths(&left.0, &right.0));

    let mut rendered_diagnostics = Vec::new();
    let mut parsed_units = Vec::new();
    for (relative_path, role) in file_entries {
        let absolute_path = package_root.join(&relative_path);
        let source =
            fs::read_to_string(&absolute_path).map_err(|error| CheckFileError::ReadSource {
                path: display_path(&absolute_path),
                error,
            })?;
        match parse_file(&source, role) {
            Ok(parsed) => parsed_units.push(ParsedUnit {
                path: relative_path,
                source,
                parsed,
            }),
            Err(diagnostics) => {
                for diagnostic in diagnostics {
                    rendered_diagnostics.push(render_diagnostic(
                        &diagnostic_display_base,
                        &relative_path,
                        &source,
                        diagnostic,
                    ));
                }
            }
        }
    }

    for parsed_unit in &parsed_units {
        let mut file_diagnostics = Vec::new();
        file_role_rules::check_file(&parsed_unit.parsed, &mut file_diagnostics);
        typecheck::check_file(&parsed_unit.parsed, &mut file_diagnostics);
        for diagnostic in file_diagnostics {
            rendered_diagnostics.push(render_diagnostic(
                &diagnostic_display_base,
                &parsed_unit.path,
                &parsed_unit.source,
                diagnostic,
            ));
        }
    }

    let mut resolution_diagnostics = Vec::new();
    let package_files: Vec<PackageFile<'_>> = parsed_units
        .iter()
        .map(|unit| PackageFile {
            path: &unit.path,
            parsed: &unit.parsed,
        })
        .collect();
    resolution::check_package(&package_files, &mut resolution_diagnostics);
    for PackageDiagnostic { path, diagnostic } in resolution_diagnostics {
        if let Some(parsed_unit) = parsed_units.iter().find(|unit| unit.path == path) {
            rendered_diagnostics.push(render_diagnostic(
                &diagnostic_display_base,
                &path,
                &parsed_unit.source,
                diagnostic,
            ));
        }
    }

    rendered_diagnostics.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.span.line.cmp(&right.span.line))
            .then(left.span.column.cmp(&right.span.column))
            .then(left.message.cmp(&right.message))
    });

    Ok(CheckedTarget {
        diagnostics: rendered_diagnostics,
    })
}

fn find_package_root(target_path: &Path) -> Option<PathBuf> {
    if target_path.is_dir() {
        return Some(target_path.to_path_buf());
    }
    let mut directory = target_path.parent()?.to_path_buf();
    loop {
        if directory.join("PACKAGE.coppice").is_file() {
            return Some(directory);
        }
        match directory.parent() {
            Some(parent) => {
                directory = parent.to_path_buf();
            }
            None => {
                return None;
            }
        }
    }
}

fn render_diagnostic(
    display_base: &Path,
    path: &Path,
    source: &str,
    diagnostic: Diagnostic,
) -> RenderedDiagnostic {
    RenderedDiagnostic {
        path: display_path(&display_base.join(path)),
        source: source.to_string(),
        message: diagnostic.message,
        span: diagnostic.span,
    }
}

fn display_path(path: &Path) -> String {
    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };

    if let Ok(relative_path) =
        absolute_path.strip_prefix(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    {
        return path_to_key(relative_path);
    }
    path_to_key(&absolute_path)
}
