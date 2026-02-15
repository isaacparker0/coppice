use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use compiler__binding as binding;
use compiler__diagnostics::Diagnostic;
use compiler__exports as exports;
use compiler__file_role_rules as file_role_rules;
use compiler__package_graph as package_graph;
use compiler__parsing::parse_file;
use compiler__source::{FileRole, Span, compare_paths, path_to_key};
use compiler__symbols::{self as symbols, PackageDiagnostic, PackageFile};
use compiler__syntax::{Declaration, Visibility};
use compiler__typecheck as typecheck;
use compiler__visibility as visibility;
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
    InvalidWorkspaceRoot { path: String, error: io::Error },
    WorkspaceRootNotDirectory { path: String },
    WorkspaceRootMissingManifest { path: String },
    InvalidCheckTarget,
    TargetOutsideWorkspace,
    PackageNotFound,
    WorkspaceDiscoveryFailed(Vec<DiscoveryError>),
}

struct ParsedUnit {
    package_path: String,
    path: PathBuf,
    source: String,
    parsed: compiler__syntax::ParsedFile,
}

pub fn check_file(path: &str) -> Result<CheckedTarget, CheckFileError> {
    check_file_with_workspace_root(path, None)
}

pub fn check_file_with_workspace_root(
    path: &str,
    workspace_root_override: Option<&str>,
) -> Result<CheckedTarget, CheckFileError> {
    let current_directory =
        std::env::current_dir().map_err(|error| CheckFileError::ReadSource {
            path: ".".to_string(),
            error,
        })?;
    let workspace_root_display =
        workspace_root_override.map_or_else(|| ".".to_string(), std::string::ToString::to_string);
    let workspace_root = if let Some(root) = workspace_root_override {
        let parsed_root = PathBuf::from(root);
        if parsed_root.is_absolute() {
            parsed_root
        } else {
            current_directory.join(parsed_root)
        }
    } else {
        current_directory
    };
    let workspace_root_metadata =
        fs::metadata(&workspace_root).map_err(|error| CheckFileError::InvalidWorkspaceRoot {
            path: workspace_root_display.clone(),
            error,
        })?;
    if !workspace_root_metadata.is_dir() {
        return Err(CheckFileError::WorkspaceRootNotDirectory {
            path: workspace_root_display.clone(),
        });
    }
    if !workspace_root.join("PACKAGE.coppice").is_file() {
        return Err(CheckFileError::WorkspaceRootMissingManifest {
            path: workspace_root_display,
        });
    }

    let target_path = PathBuf::from(path);
    let absolute_target_path = if target_path.is_absolute() {
        target_path.clone()
    } else {
        workspace_root.join(&target_path)
    };
    let metadata =
        fs::metadata(&absolute_target_path).map_err(|error| CheckFileError::ReadSource {
            path: path.to_string(),
            error,
        })?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(CheckFileError::InvalidCheckTarget);
    }
    if !absolute_target_path.starts_with(&workspace_root) {
        return Err(CheckFileError::TargetOutsideWorkspace);
    }

    let diagnostic_display_base = workspace_root.clone();

    if metadata.is_file()
        && find_owning_package_root(&workspace_root, &absolute_target_path).is_none()
    {
        return Err(CheckFileError::PackageNotFound);
    }
    let workspace =
        discover_workspace(&workspace_root).map_err(CheckFileError::WorkspaceDiscoveryFailed)?;

    let mut rendered_diagnostics = Vec::new();
    let mut parsed_units = Vec::new();
    for package in workspace.packages() {
        let mut file_entries: Vec<(PathBuf, FileRole)> = Vec::new();
        file_entries.push((package.manifest_path.clone(), FileRole::PackageManifest));
        for source_file in &package.source_files {
            file_entries.push((
                source_file.workspace_relative_path.clone(),
                source_file.role,
            ));
        }
        file_entries.sort_by(|left, right| compare_paths(&left.0, &right.0));

        for (relative_path, role) in file_entries {
            let absolute_path = workspace_root.join(&relative_path);
            let source =
                fs::read_to_string(&absolute_path).map_err(|error| CheckFileError::ReadSource {
                    path: display_path(&absolute_path),
                    error,
                })?;
            match parse_file(&source, role) {
                Ok(parsed) => parsed_units.push(ParsedUnit {
                    package_path: package.package_path.clone(),
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
    }

    for parsed_unit in &parsed_units {
        let mut file_diagnostics = Vec::new();
        file_role_rules::check_file(&parsed_unit.parsed, &mut file_diagnostics);
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
            package_path: &unit.package_path,
            path: &unit.path,
            parsed: &unit.parsed,
        })
        .collect();
    let symbols_by_package = symbols::collect_symbols(&package_files, &mut resolution_diagnostics);
    let exports_by_package = exports::build_exports(
        &package_files,
        &symbols_by_package,
        &mut resolution_diagnostics,
    );
    let resolved_imports = visibility::resolve_imports(
        &package_files,
        &symbols_by_package,
        &exports_by_package,
        &mut resolution_diagnostics,
    );
    package_graph::check_cycles(&resolved_imports, &mut resolution_diagnostics);
    let bindings_by_file = visibility::resolved_bindings_by_file(&resolved_imports);
    binding::check_bindings(
        &package_files,
        &bindings_by_file,
        &mut resolution_diagnostics,
    );
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

    let imported_bindings_by_file =
        build_typecheck_imported_bindings(&parsed_units, &resolved_imports);

    let has_pre_typecheck_errors = !rendered_diagnostics.is_empty();
    if has_pre_typecheck_errors {
        rendered_diagnostics.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then(left.span.line.cmp(&right.span.line))
                .then(left.span.column.cmp(&right.span.column))
                .then(left.message.cmp(&right.message))
        });
        return Ok(CheckedTarget {
            diagnostics: rendered_diagnostics,
        });
    }

    for parsed_unit in &parsed_units {
        let mut file_diagnostics = Vec::new();
        let imported_bindings = imported_bindings_by_file
            .get(&parsed_unit.path)
            .map_or(&[][..], Vec::as_slice);
        typecheck::check_file(
            &parsed_unit.parsed,
            imported_bindings,
            &mut file_diagnostics,
        );
        for diagnostic in file_diagnostics {
            rendered_diagnostics.push(render_diagnostic(
                &diagnostic_display_base,
                &parsed_unit.path,
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

fn build_typecheck_imported_bindings(
    parsed_units: &[ParsedUnit],
    resolved_imports: &[visibility::ResolvedImport],
) -> BTreeMap<PathBuf, Vec<typecheck::ImportedBinding>> {
    let mut declaration_by_package_and_name: BTreeMap<(String, String), Declaration> =
        BTreeMap::new();
    let mut ordered_units: Vec<&ParsedUnit> = parsed_units.iter().collect();
    ordered_units.sort_by(|left, right| {
        left.package_path
            .cmp(&right.package_path)
            .then(compare_paths(&left.path, &right.path))
    });

    for unit in ordered_units {
        if unit.parsed.role != FileRole::Library {
            continue;
        }
        for declaration in &unit.parsed.declarations {
            let (name, is_public) = match declaration {
                Declaration::Type(type_declaration) => (
                    &type_declaration.name,
                    type_declaration.visibility == Visibility::Public,
                ),
                Declaration::Function(function_declaration) => (
                    &function_declaration.name,
                    function_declaration.visibility == Visibility::Public,
                ),
                Declaration::Constant(constant_declaration) => (
                    &constant_declaration.name,
                    constant_declaration.visibility == Visibility::Public,
                ),
                Declaration::Import(_) | Declaration::Exports(_) => {
                    continue;
                }
            };
            if !is_public {
                continue;
            }

            declaration_by_package_and_name
                .entry((unit.package_path.clone(), name.clone()))
                .or_insert_with(|| declaration.clone());
        }
    }

    let mut imported_by_file: BTreeMap<PathBuf, Vec<typecheck::ImportedBinding>> = BTreeMap::new();
    for resolved_import in resolved_imports {
        for binding in &resolved_import.bindings {
            let key = (
                resolved_import.target_package_path.clone(),
                binding.imported_name.clone(),
            );
            let Some(declaration) = declaration_by_package_and_name.get(&key) else {
                continue;
            };

            let symbol = match declaration {
                Declaration::Type(type_declaration) => {
                    typecheck::ImportedSymbol::Type(type_declaration.clone())
                }
                Declaration::Function(function_declaration) => {
                    typecheck::ImportedSymbol::Function(function_declaration.clone())
                }
                Declaration::Constant(constant_declaration) => {
                    typecheck::ImportedSymbol::Constant(constant_declaration.clone())
                }
                Declaration::Import(_) | Declaration::Exports(_) => {
                    continue;
                }
            };

            imported_by_file
                .entry(resolved_import.source_path.clone())
                .or_default()
                .push(typecheck::ImportedBinding {
                    local_name: binding.local_name.clone(),
                    span: binding.span.clone(),
                    symbol,
                });
        }
    }

    imported_by_file
}

fn find_owning_package_root(workspace_root: &Path, target_path: &Path) -> Option<PathBuf> {
    let mut directory = target_path.parent()?.to_path_buf();
    loop {
        if directory.join("PACKAGE.coppice").is_file() {
            return Some(directory);
        }
        if directory == workspace_root {
            return None;
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
