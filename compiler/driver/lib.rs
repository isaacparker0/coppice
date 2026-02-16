use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use compiler__diagnostics::{FileScopedDiagnostic, PhaseDiagnostic};
use compiler__file_role_rules as file_role_rules;
use compiler__package_symbols::{
    PackageUnit, ResolvedImportBindingSummary, ResolvedImportSummary,
    build_typed_public_symbol_table,
};
use compiler__packages::PackageId;
use compiler__parsing::parse_file;
use compiler__phase_results::PhaseStatus;
use compiler__reports::{
    CompilerFailure, CompilerFailureDetail, CompilerFailureKind, DiagnosticPhase,
    RenderedDiagnostic,
};
use compiler__resolution as resolution;
use compiler__semantic_lowering::lower_parsed_file;
use compiler__source::{FileRole, compare_paths, path_to_key};
use compiler__syntax_rules as syntax_rules;
use compiler__type_analysis as type_analysis;
use compiler__visibility::ResolvedImport;
use compiler__workspace::{Workspace, discover_workspace};

pub struct CheckedTarget {
    pub diagnostics: Vec<RenderedDiagnostic>,
    pub source_by_path: BTreeMap<String, String>,
}

struct ParsedUnit {
    package_id: PackageId,
    package_path: String,
    path: PathBuf,
    parsed: compiler__syntax::ParsedFile,
    phase_state: FilePhaseState,
}

struct FilePhaseState {
    parsing: PhaseStatus,
    syntax_rules: PhaseStatus,
    file_role_rules: PhaseStatus,
    resolution: PhaseStatus,
    semantic_lowering: PhaseStatus,
}

impl FilePhaseState {
    fn can_run_syntax_checks(&self) -> bool {
        matches!(self.parsing, PhaseStatus::Ok)
    }

    fn can_run_resolution(&self) -> bool {
        self.can_run_syntax_checks()
            && matches!(self.syntax_rules, PhaseStatus::Ok)
            && matches!(self.file_role_rules, PhaseStatus::Ok)
    }

    fn can_run_type_analysis(&self) -> bool {
        self.can_run_semantic_lowering() && matches!(self.semantic_lowering, PhaseStatus::Ok)
    }

    fn can_run_semantic_lowering(&self) -> bool {
        self.can_run_resolution() && matches!(self.resolution, PhaseStatus::Ok)
    }
}

pub fn check_target(path: &str) -> Result<CheckedTarget, CompilerFailure> {
    check_target_with_workspace_root(path, None)
}

pub fn check_target_with_workspace_root(
    path: &str,
    workspace_root_override: Option<&str>,
) -> Result<CheckedTarget, CompilerFailure> {
    let current_directory = std::env::current_dir().map_err(|error| CompilerFailure {
        kind: CompilerFailureKind::ReadSource,
        message: error.to_string(),
        path: Some(".".to_string()),
        details: Vec::new(),
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
        fs::metadata(&workspace_root).map_err(|error| CompilerFailure {
            kind: CompilerFailureKind::InvalidWorkspaceRoot,
            message: format!("invalid workspace root: {error}"),
            path: Some(workspace_root_display.clone()),
            details: Vec::new(),
        })?;
    if !workspace_root_metadata.is_dir() {
        return Err(CompilerFailure {
            kind: CompilerFailureKind::WorkspaceRootNotDirectory,
            message: "workspace root must be a directory".to_string(),
            path: Some(workspace_root_display.clone()),
            details: Vec::new(),
        });
    }
    if !workspace_root.join("PACKAGE.coppice").is_file() {
        return Err(CompilerFailure {
            kind: CompilerFailureKind::WorkspaceRootMissingManifest,
            message: "not a Coppice workspace root (missing PACKAGE.coppice)".to_string(),
            path: Some(workspace_root_display),
            details: Vec::new(),
        });
    }

    let target_path = PathBuf::from(path);
    let absolute_target_path = if target_path.is_absolute() {
        target_path.clone()
    } else {
        workspace_root.join(&target_path)
    };
    let metadata = fs::metadata(&absolute_target_path).map_err(|error| CompilerFailure {
        kind: CompilerFailureKind::ReadSource,
        message: error.to_string(),
        path: Some(path.to_string()),
        details: Vec::new(),
    })?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(CompilerFailure {
            kind: CompilerFailureKind::InvalidCheckTarget,
            message: "expected a file or directory path".to_string(),
            path: Some(path.to_string()),
            details: Vec::new(),
        });
    }
    if !absolute_target_path.starts_with(&workspace_root) {
        return Err(CompilerFailure {
            kind: CompilerFailureKind::TargetOutsideWorkspace,
            message: "target is outside the current workspace root".to_string(),
            path: Some(path.to_string()),
            details: Vec::new(),
        });
    }

    let diagnostic_display_base = workspace_root.clone();

    if metadata.is_file()
        && find_owning_package_root(&workspace_root, &absolute_target_path).is_none()
    {
        return Err(CompilerFailure {
            kind: CompilerFailureKind::PackageNotFound,
            message: "target is not inside a package (missing PACKAGE.coppice)".to_string(),
            path: Some(path.to_string()),
            details: Vec::new(),
        });
    }
    let workspace = discover_workspace(&workspace_root).map_err(|errors| CompilerFailure {
        kind: CompilerFailureKind::WorkspaceDiscoveryFailed,
        message: "workspace discovery failed".to_string(),
        path: Some(path.to_string()),
        details: errors
            .into_iter()
            .map(|error| CompilerFailureDetail {
                message: error.message,
                path: error.path.map(|path| path.display().to_string()),
            })
            .collect(),
    })?;
    let scoped_package_paths = scoped_package_paths_for_target(
        &workspace,
        &workspace_root,
        &absolute_target_path,
        &metadata,
    )?;
    let scope_is_workspace = scoped_package_paths.is_none();

    let mut rendered_diagnostics = Vec::new();
    let mut source_by_path = BTreeMap::new();
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
            let source = fs::read_to_string(&absolute_path).map_err(|error| CompilerFailure {
                kind: CompilerFailureKind::ReadSource,
                message: error.to_string(),
                path: Some(display_path(&absolute_path)),
                details: Vec::new(),
            })?;
            let rendered_path = display_path(&absolute_path);
            source_by_path.insert(rendered_path.clone(), source.clone());
            let parse_result = parse_file(&source, role);
            if package_in_scope {
                for diagnostic in &parse_result.diagnostics {
                    rendered_diagnostics.push(render_diagnostic(
                        DiagnosticPhase::Parsing,
                        rendered_path.clone(),
                        diagnostic.clone(),
                    ));
                }
            }
            parsed_units.push(ParsedUnit {
                package_id: package.id,
                package_path: package.package_path.clone(),
                path: relative_path,
                parsed: parse_result.value,
                phase_state: FilePhaseState {
                    parsing: parse_result.status,
                    syntax_rules: PhaseStatus::Ok,
                    file_role_rules: PhaseStatus::Ok,
                    resolution: PhaseStatus::Ok,
                    semantic_lowering: PhaseStatus::Ok,
                },
            });
        }
    }

    for parsed_unit in &mut parsed_units {
        if !parsed_unit.phase_state.can_run_syntax_checks() {
            continue;
        }
        let syntax_rules_result = syntax_rules::check_file(&parsed_unit.parsed);
        parsed_unit.phase_state.syntax_rules = syntax_rules_result.status;
        let file_role_rules_result = file_role_rules::check_file(&parsed_unit.parsed);
        parsed_unit.phase_state.file_role_rules = file_role_rules_result.status;

        if !scope_is_workspace
            && !scoped_package_paths
                .as_ref()
                .is_some_and(|scoped| scoped.contains(&parsed_unit.package_path))
        {
            continue;
        }
        for diagnostic in syntax_rules_result.diagnostics {
            rendered_diagnostics.push(render_diagnostic(
                DiagnosticPhase::SyntaxRules,
                display_path(&diagnostic_display_base.join(&parsed_unit.path)),
                diagnostic,
            ));
        }
        for diagnostic in file_role_rules_result.diagnostics {
            rendered_diagnostics.push(render_diagnostic(
                DiagnosticPhase::FileRoleRules,
                display_path(&diagnostic_display_base.join(&parsed_unit.path)),
                diagnostic,
            ));
        }
    }

    let resolution_files: Vec<resolution::ResolutionFile<'_>> = parsed_units
        .iter()
        .filter(|unit| unit.phase_state.can_run_resolution())
        .map(|unit| resolution::ResolutionFile {
            package_path: &unit.package_path,
            path: &unit.path,
            parsed: &unit.parsed,
        })
        .collect();
    let resolution_result = resolution::resolve_files(&resolution_files);
    let resolved_imports = resolution_result.value.resolved_imports;
    for (path, status) in &resolution_result.status_by_file {
        if let Some(parsed_unit) = parsed_units.iter_mut().find(|unit| &unit.path == path) {
            parsed_unit.phase_state.resolution = *status;
        }
    }
    for FileScopedDiagnostic {
        path,
        message,
        span,
    } in resolution_result.diagnostics
    {
        if let Some(parsed_unit) = parsed_units.iter().find(|unit| unit.path == path) {
            if !scope_is_workspace
                && !scoped_package_paths
                    .as_ref()
                    .is_some_and(|scoped| scoped.contains(&parsed_unit.package_path))
            {
                continue;
            }
            rendered_diagnostics.push(render_diagnostic(
                DiagnosticPhase::Resolution,
                display_path(&diagnostic_display_base.join(&path)),
                PhaseDiagnostic::new(message, span),
            ));
        }
    }

    let package_id_by_path = collect_package_ids_by_path(&workspace);
    let mut semantic_program_by_file: BTreeMap<PathBuf, compiler__semantic_program::PackageUnit> =
        BTreeMap::new();
    for parsed_unit in &mut parsed_units {
        if !parsed_unit.phase_state.can_run_semantic_lowering() {
            continue;
        }
        let lowering_result = lower_parsed_file(&parsed_unit.parsed);
        let compiler__phase_results::PhaseOutput {
            value,
            diagnostics,
            status,
        } = lowering_result;
        parsed_unit.phase_state.semantic_lowering = status;
        if !scope_is_workspace
            && !scoped_package_paths
                .as_ref()
                .is_some_and(|scoped| scoped.contains(&parsed_unit.package_path))
        {
            continue;
        }
        for diagnostic in diagnostics {
            rendered_diagnostics.push(render_diagnostic(
                DiagnosticPhase::SemanticLowering,
                display_path(&diagnostic_display_base.join(&parsed_unit.path)),
                diagnostic,
            ));
        }
        if matches!(parsed_unit.phase_state.semantic_lowering, PhaseStatus::Ok) {
            semantic_program_by_file.insert(parsed_unit.path.clone(), value);
        }
    }
    let package_units: Vec<PackageUnit<'_>> = parsed_units
        .iter()
        .filter_map(|unit| {
            semantic_program_by_file
                .get(&unit.path)
                .map(|program| PackageUnit {
                    package_id: unit.package_id,
                    path: &unit.path,
                    program,
                })
        })
        .collect();
    let typecheck_resolved_imports =
        build_typecheck_resolved_imports(&resolved_imports, &package_id_by_path);
    let typed_public_symbol_table =
        build_typed_public_symbol_table(&package_units, &typecheck_resolved_imports);
    let imported_bindings_by_file =
        typed_public_symbol_table.imported_bindings_by_file(&typecheck_resolved_imports);

    for parsed_unit in &parsed_units {
        if !scope_is_workspace
            && !scoped_package_paths
                .as_ref()
                .is_some_and(|scoped| scoped.contains(&parsed_unit.package_path))
        {
            continue;
        }
        if !parsed_unit.phase_state.can_run_type_analysis() {
            continue;
        }
        let imported_bindings = imported_bindings_by_file
            .get(&parsed_unit.path)
            .map_or(&[][..], Vec::as_slice);
        let Some(semantic_unit) = semantic_program_by_file.get(&parsed_unit.path) else {
            continue;
        };
        let type_analysis_result = type_analysis::check_package_unit(
            parsed_unit.package_id,
            semantic_unit,
            imported_bindings,
        );
        for diagnostic in type_analysis_result.diagnostics {
            rendered_diagnostics.push(render_diagnostic(
                DiagnosticPhase::TypeAnalysis,
                display_path(&diagnostic_display_base.join(&parsed_unit.path)),
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
            .then(left.phase.cmp(&right.phase))
    });

    Ok(CheckedTarget {
        diagnostics: rendered_diagnostics,
        source_by_path,
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
    resolved_imports: &[ResolvedImport],
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

fn scoped_package_paths_for_target(
    workspace: &Workspace,
    workspace_root: &Path,
    absolute_target_path: &Path,
    target_metadata: &fs::Metadata,
) -> Result<Option<BTreeSet<String>>, CompilerFailure> {
    if absolute_target_path == workspace_root {
        return Ok(None);
    }

    let owning_package_root = if target_metadata.is_file() {
        find_owning_package_root(workspace_root, absolute_target_path)
    } else {
        find_owning_package_root_for_directory(workspace_root, absolute_target_path)
    };
    let Some(owning_package_root) = owning_package_root else {
        return Err(CompilerFailure {
            kind: CompilerFailureKind::PackageNotFound,
            message: "target is not inside a package (missing PACKAGE.coppice)".to_string(),
            path: Some(path_to_key(absolute_target_path)),
            details: Vec::new(),
        });
    };

    let owning_package_path = relative_package_path(workspace_root, &owning_package_root)
        .ok_or_else(|| CompilerFailure {
            kind: CompilerFailureKind::PackageNotFound,
            message: "target is not inside a package (missing PACKAGE.coppice)".to_string(),
            path: Some(path_to_key(absolute_target_path)),
            details: Vec::new(),
        })?;
    if workspace.package_by_path(&owning_package_path).is_none() {
        return Err(CompilerFailure {
            kind: CompilerFailureKind::PackageNotFound,
            message: "target is not inside a package (missing PACKAGE.coppice)".to_string(),
            path: Some(path_to_key(absolute_target_path)),
            details: Vec::new(),
        });
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
    phase: DiagnosticPhase,
    path: String,
    diagnostic: PhaseDiagnostic,
) -> RenderedDiagnostic {
    RenderedDiagnostic {
        phase,
        path,
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
