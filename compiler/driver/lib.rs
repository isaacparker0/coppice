use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use compiler__binding as binding;
use compiler__diagnostics::Diagnostic;
use compiler__exports as exports;
use compiler__file_role_rules as file_role_rules;
use compiler__package_graph as package_graph;
use compiler__package_symbols::{
    PackageUnit, ResolvedImportBindingSummary, ResolvedImportSummary, TypedPublicSymbolTable,
    build_typed_public_symbol_table,
};
use compiler__packages::PackageId;
use compiler__parsing::parse_file;
use compiler__semantic_types::ImportedBinding;
use compiler__source::{FileRole, Span, compare_paths, path_to_key};
use compiler__symbols::{self as symbols, PackageDiagnostic, PackageFile};
use compiler__syntax::{Declaration, Visibility};
use compiler__typecheck::{self as typecheck};
use compiler__visibility as visibility;
use compiler__workspace::{DiscoveryError, Workspace, discover_workspace};

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
    package_id: PackageId,
    package_path: String,
    path: PathBuf,
    source: String,
    parsed: compiler__syntax::ParsedFile,
}

pub fn check_target(path: &str) -> Result<CheckedTarget, CheckFileError> {
    check_target_with_workspace_root(path, None)
}

pub fn check_target_with_workspace_root(
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
    let scoped_package_paths = scoped_package_paths_for_target(
        &workspace,
        &workspace_root,
        &absolute_target_path,
        &metadata,
    )?;
    let scope_is_workspace = scoped_package_paths.is_none();

    let mut rendered_diagnostics = Vec::new();
    let mut parsed_units = Vec::new();
    for package in workspace.packages() {
        let package_in_scope = scope_is_workspace
            || scoped_package_paths
                .as_ref()
                .is_some_and(|scoped| scoped.contains(&package.package_path));
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
                    package_id: package.id,
                    package_path: package.package_path.clone(),
                    path: relative_path,
                    source,
                    parsed,
                }),
                Err(diagnostics) => {
                    if package_in_scope {
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
    }

    for parsed_unit in &parsed_units {
        if !scope_is_workspace
            && !scoped_package_paths
                .as_ref()
                .is_some_and(|scoped| scoped.contains(&parsed_unit.package_path))
        {
            continue;
        }
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
            if !scope_is_workspace
                && !scoped_package_paths
                    .as_ref()
                    .is_some_and(|scoped| scoped.contains(&parsed_unit.package_path))
            {
                continue;
            }
            rendered_diagnostics.push(render_diagnostic(
                &diagnostic_display_base,
                &path,
                &parsed_unit.source,
                diagnostic,
            ));
        }
    }

    let package_id_by_path = collect_package_ids_by_path(&workspace);
    let package_units: Vec<PackageUnit<'_>> = parsed_units
        .iter()
        .map(|unit| PackageUnit {
            package_id: unit.package_id,
            path: &unit.path,
            parsed: &unit.parsed,
        })
        .collect();
    let typecheck_resolved_imports =
        build_typecheck_resolved_imports(&resolved_imports, &package_id_by_path);
    let typed_public_symbol_table =
        build_typed_public_symbol_table(&package_units, &typecheck_resolved_imports);
    let imported_bindings_by_file = infer_imported_bindings_by_file(
        &parsed_units,
        &typecheck_resolved_imports,
        &typed_public_symbol_table,
    );

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
        if !scope_is_workspace
            && !scoped_package_paths
                .as_ref()
                .is_some_and(|scoped| scoped.contains(&parsed_unit.package_path))
        {
            continue;
        }
        let mut file_diagnostics = Vec::new();
        let imported_bindings = imported_bindings_by_file
            .get(&parsed_unit.path)
            .map_or(&[][..], Vec::as_slice);
        typecheck::check_package_unit(
            parsed_unit.package_id,
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

fn collect_package_ids_by_path(workspace: &Workspace) -> BTreeMap<String, PackageId> {
    let mut package_id_by_path = BTreeMap::new();
    for package in workspace.packages() {
        package_id_by_path.insert(package.package_path.clone(), package.id);
    }
    package_id_by_path
}

fn build_typecheck_resolved_imports(
    resolved_imports: &[visibility::ResolvedImport],
    package_id_by_path: &BTreeMap<String, PackageId>,
) -> Vec<ResolvedImportSummary> {
    let mut typecheck_resolved_imports = Vec::new();
    for resolved_import in resolved_imports {
        let Some(target_package_id) = package_id_by_path.get(&resolved_import.target_package_path)
        else {
            continue;
        };
        let bindings = resolved_import
            .bindings
            .iter()
            .map(|binding| ResolvedImportBindingSummary {
                imported_name: binding.imported_name.clone(),
                local_name: binding.local_name.clone(),
                span: binding.span.clone(),
            })
            .collect();
        typecheck_resolved_imports.push(ResolvedImportSummary {
            source_path: resolved_import.source_path.clone(),
            target_package_id: *target_package_id,
            bindings,
        });
    }
    typecheck_resolved_imports
}

fn infer_imported_bindings_by_file(
    parsed_units: &[ParsedUnit],
    resolved_imports: &[ResolvedImportSummary],
    typed_public_symbol_table: &TypedPublicSymbolTable,
) -> BTreeMap<PathBuf, Vec<ImportedBinding>> {
    let public_constant_names_by_file = public_constant_names_by_file(parsed_units);
    if public_constant_names_by_file.is_empty() {
        return typed_public_symbol_table.imported_bindings_by_file(resolved_imports);
    }

    let public_constant_count: usize = public_constant_names_by_file.values().map(Vec::len).sum();
    let max_iterations = public_constant_count + 1;
    let mut constant_type_by_symbol: BTreeMap<(PackageId, String), typecheck::Type> =
        BTreeMap::new();

    for _ in 0..max_iterations {
        let imported_bindings_by_file = typed_public_symbol_table
            .imported_bindings_by_file_with_constant_types(
                resolved_imports,
                &constant_type_by_symbol,
            );
        let mut changed = false;

        for parsed_unit in parsed_units {
            let Some(public_constant_names) = public_constant_names_by_file.get(&parsed_unit.path)
            else {
                continue;
            };
            let imported_bindings = imported_bindings_by_file
                .get(&parsed_unit.path)
                .map_or(&[][..], Vec::as_slice);
            let mut ignored_diagnostics = Vec::new();
            let summary = typecheck::analyze_package_unit(
                parsed_unit.package_id,
                &parsed_unit.parsed,
                imported_bindings,
                &mut ignored_diagnostics,
            );

            for public_constant_name in public_constant_names {
                let Some(typecheck::TypedSymbol::Constant(value_type)) =
                    summary.typed_symbol_by_name.get(public_constant_name)
                else {
                    continue;
                };
                let key = (parsed_unit.package_id, public_constant_name.clone());
                if constant_type_by_symbol.get(&key) == Some(value_type) {
                    continue;
                }
                constant_type_by_symbol.insert(key, value_type.clone());
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }

    typed_public_symbol_table
        .imported_bindings_by_file_with_constant_types(resolved_imports, &constant_type_by_symbol)
}

fn public_constant_names_by_file(parsed_units: &[ParsedUnit]) -> BTreeMap<PathBuf, Vec<String>> {
    let mut names_by_file = BTreeMap::new();
    for parsed_unit in parsed_units {
        if parsed_unit.parsed.role != FileRole::Library {
            continue;
        }
        let mut names = Vec::new();
        for declaration in &parsed_unit.parsed.declarations {
            let Declaration::Constant(constant_declaration) = declaration else {
                continue;
            };
            if constant_declaration.visibility != Visibility::Public {
                continue;
            }
            names.push(constant_declaration.name.clone());
        }
        if !names.is_empty() {
            names_by_file.insert(parsed_unit.path.clone(), names);
        }
    }
    names_by_file
}

fn scoped_package_paths_for_target(
    workspace: &Workspace,
    workspace_root: &Path,
    absolute_target_path: &Path,
    target_metadata: &fs::Metadata,
) -> Result<Option<BTreeSet<String>>, CheckFileError> {
    if absolute_target_path == workspace_root {
        return Ok(None);
    }

    let owning_package_root = if target_metadata.is_file() {
        find_owning_package_root(workspace_root, absolute_target_path)
    } else {
        find_owning_package_root_for_directory(workspace_root, absolute_target_path)
    };
    let Some(owning_package_root) = owning_package_root else {
        return Err(CheckFileError::PackageNotFound);
    };

    let owning_package_path = relative_package_path(workspace_root, &owning_package_root)
        .ok_or(CheckFileError::PackageNotFound)?;
    if workspace.package_by_path(&owning_package_path).is_none() {
        return Err(CheckFileError::PackageNotFound);
    }

    let mut scoped = BTreeSet::new();
    scoped.insert(owning_package_path);
    Ok(Some(scoped))
}

fn find_owning_package_root_for_directory(
    workspace_root: &Path,
    target_directory: &Path,
) -> Option<PathBuf> {
    let mut directory = target_directory.to_path_buf();
    loop {
        if directory.join("PACKAGE.coppice").is_file() {
            return Some(directory);
        }
        if directory == workspace_root {
            return None;
        }
        match directory.parent() {
            Some(parent) => directory = parent.to_path_buf(),
            None => return None,
        }
    }
}

fn relative_package_path(workspace_root: &Path, package_root: &Path) -> Option<String> {
    let relative = package_root.strip_prefix(workspace_root).ok()?;
    let key = path_to_key(relative);
    if key == "." || key.is_empty() {
        return Some(String::new());
    }
    Some(key)
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
