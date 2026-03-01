use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use compiler__diagnostics::{FileScopedDiagnostic, PhaseDiagnostic};
use compiler__file_role_rules as file_role_rules;
use compiler__fix_edits::{TextEdit, apply_text_edits, merge_text_edits};
use compiler__package_symbols::{
    PackageSymbolFileInput, ResolvedImportBindingSummary, ResolvedImportSummary,
    build_typed_public_symbol_table,
};
use compiler__packages::PackageId;
use compiler__parsing::parse_file;
use compiler__phase_results::{PhaseOutput, PhaseStatus};
use compiler__reports::{
    CompilerFailure, CompilerFailureDetail, CompilerFailureKind, DiagnosticPhase,
    RenderedDiagnostic,
};
use compiler__resolution as resolution;
use compiler__safe_autofix::SafeAutofix;
use compiler__semantic_lowering::lower_parsed_file;
use compiler__semantic_program::SemanticFile;
use compiler__source::{FileRole, compare_paths, path_to_key};
use compiler__source_formatting::formatting_text_edits;
use compiler__syntax_rules as syntax_rules;
use compiler__type_analysis as type_analysis;
use compiler__type_annotated_program::TypeResolvedDeclarations;
use compiler__visibility::ResolvedImport;
use compiler__workspace::{Workspace, discover_workspace};

const WORKSPACE_MARKER_FILENAME: &str = "COPPICE_WORKSPACE";

pub struct AnalyzedTargetSummary {
    pub diagnostics: Vec<RenderedDiagnostic>,
    pub source_by_path: BTreeMap<String, String>,
    pub safe_autofix_edit_count_by_workspace_relative_path: BTreeMap<String, usize>,
}

pub struct AnalyzedTarget {
    pub diagnostics: Vec<RenderedDiagnostic>,
    pub all_diagnostics_by_file: BTreeMap<PathBuf, Vec<RenderedDiagnostic>>,
    pub source_by_path: BTreeMap<String, String>,
    pub source_by_workspace_relative_path_in_scope: BTreeMap<String, String>,
    pub safe_autofix_edit_count_by_workspace_relative_path: BTreeMap<String, usize>,
    pub canonical_source_override_by_workspace_relative_path: BTreeMap<String, String>,
    pub workspace_root: PathBuf,
    pub workspace: Workspace,
    pub absolute_target_path: PathBuf,
    pub target_is_file: bool,
    pub package_path_by_file: BTreeMap<PathBuf, String>,
    pub file_role_by_path: BTreeMap<PathBuf, FileRole>,
    pub resolved_imports: Vec<ResolvedImport>,
    pub resolved_declarations_by_path: BTreeMap<PathBuf, TypeResolvedDeclarations>,
}

struct ParsedUnit {
    package_id: PackageId,
    package_path: String,
    path: PathBuf,
    parsed: compiler__syntax::SyntaxParsedFile,
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

pub fn analyze_target_summary(path: &str) -> Result<AnalyzedTargetSummary, CompilerFailure> {
    analyze_target_summary_with_workspace_root(path, None)
}

pub fn analyze_target_summary_with_workspace_root(
    path: &str,
    workspace_root_override: Option<&str>,
) -> Result<AnalyzedTargetSummary, CompilerFailure> {
    let source_override_by_workspace_relative_path = BTreeMap::new();
    let analyzed_target = analyze_target_with_workspace_root_and_overrides(
        path,
        workspace_root_override,
        &source_override_by_workspace_relative_path,
    )?;
    Ok(AnalyzedTargetSummary {
        diagnostics: analyzed_target.diagnostics,
        source_by_path: analyzed_target.source_by_path,
        safe_autofix_edit_count_by_workspace_relative_path: analyzed_target
            .safe_autofix_edit_count_by_workspace_relative_path,
    })
}

pub fn analyze_target_with_workspace_root(
    path: &str,
    workspace_root_override: Option<&str>,
) -> Result<AnalyzedTarget, CompilerFailure> {
    let source_override_by_workspace_relative_path = BTreeMap::new();
    analyze_target_with_workspace_root_and_overrides(
        path,
        workspace_root_override,
        &source_override_by_workspace_relative_path,
    )
}

pub fn analyze_target_summary_with_workspace_root_and_overrides(
    path: &str,
    workspace_root_override: Option<&str>,
    source_override_by_workspace_relative_path: &BTreeMap<String, String>,
) -> Result<AnalyzedTargetSummary, CompilerFailure> {
    let analyzed_target = analyze_target_with_workspace_root_and_overrides(
        path,
        workspace_root_override,
        source_override_by_workspace_relative_path,
    )?;
    Ok(AnalyzedTargetSummary {
        diagnostics: analyzed_target.diagnostics,
        source_by_path: analyzed_target.source_by_path,
        safe_autofix_edit_count_by_workspace_relative_path: analyzed_target
            .safe_autofix_edit_count_by_workspace_relative_path,
    })
}

pub fn analyze_target_with_workspace_root_and_overrides(
    path: &str,
    workspace_root_override: Option<&str>,
    source_override_by_workspace_relative_path: &BTreeMap<String, String>,
) -> Result<AnalyzedTarget, CompilerFailure> {
    let workspace_root = resolve_workspace_root(path, workspace_root_override)?;
    let current_directory = std::env::current_dir().map_err(|error| CompilerFailure {
        kind: CompilerFailureKind::ReadSource,
        message: error.to_string(),
        path: Some(".".to_string()),
        details: Vec::new(),
    })?;

    let target_path = PathBuf::from(path);
    let absolute_target_path = if target_path.is_absolute() {
        target_path.clone()
    } else if workspace_root_override.is_some() {
        workspace_root.join(&target_path)
    } else {
        current_directory.join(&target_path)
    };
    let metadata = fs::metadata(&absolute_target_path).map_err(|error| CompilerFailure {
        kind: CompilerFailureKind::ReadSource,
        message: error.to_string(),
        path: Some(path.to_string()),
        details: Vec::new(),
    })?;
    let target_is_file = metadata.is_file();
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(CompilerFailure {
            kind: CompilerFailureKind::InvalidAnalysisTarget,
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
            message: "target is not inside a package (missing PACKAGE.copp)".to_string(),
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
    if workspace.packages().is_empty()
        && metadata.is_dir()
        && absolute_target_path == workspace_root
    {
        return Err(CompilerFailure {
            kind: CompilerFailureKind::PackageNotFound,
            message: "workspace contains no packages (missing PACKAGE.copp)".to_string(),
            path: Some(path.to_string()),
            details: Vec::new(),
        });
    }
    let scoped_package_paths = scoped_package_paths_for_target(
        &workspace,
        &workspace_root,
        &absolute_target_path,
        &metadata,
    )?;
    let scope_is_workspace = scoped_package_paths.is_none();

    let mut rendered_diagnostics = Vec::new();
    let mut all_diagnostics_by_file = BTreeMap::<PathBuf, Vec<RenderedDiagnostic>>::new();
    let mut source_by_path = BTreeMap::new();
    let mut source_by_workspace_relative_path_in_scope = BTreeMap::new();
    let mut safe_autofix_edits_by_workspace_relative_path =
        BTreeMap::<String, Vec<TextEdit>>::new();
    let mut parsed_units = Vec::new();
    let mut package_path_by_file = BTreeMap::new();
    let mut file_role_by_path = BTreeMap::new();
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
            package_path_by_file.insert(relative_path.clone(), package.package_path.clone());
            file_role_by_path.insert(relative_path.clone(), role);
            let workspace_relative_key = path_to_key(&relative_path);
            let source = if let Some(override_source) =
                source_override_by_workspace_relative_path.get(&workspace_relative_key)
            {
                override_source.clone()
            } else if let Some(override_source) =
                source_override_by_workspace_relative_path.get(&path_to_key(&absolute_path))
            {
                override_source.clone()
            } else {
                fs::read_to_string(&absolute_path).map_err(|error| CompilerFailure {
                    kind: CompilerFailureKind::ReadSource,
                    message: error.to_string(),
                    path: Some(display_path(&absolute_path)),
                    details: Vec::new(),
                })?
            };
            let rendered_path = display_path(&absolute_path);
            source_by_path.insert(rendered_path.clone(), source.clone());
            if package_in_scope {
                source_by_workspace_relative_path_in_scope
                    .insert(workspace_relative_key.clone(), source.clone());
            }
            let parse_result = parse_file(&source, role);
            for diagnostic in &parse_result.diagnostics {
                let rendered_diagnostic = render_diagnostic(
                    DiagnosticPhase::Parsing,
                    rendered_path.clone(),
                    diagnostic.clone(),
                );
                push_rendered_diagnostic(
                    &mut rendered_diagnostics,
                    &mut all_diagnostics_by_file,
                    &relative_path,
                    rendered_diagnostic,
                    package_in_scope,
                );
            }
            let PhaseOutput {
                value: parsed_file,
                diagnostics: _,
                safe_autofixes: parse_safe_autofixes,
                status: parsing_status,
            } = parse_result;
            parsed_units.push(ParsedUnit {
                package_id: package.id,
                package_path: package.package_path.clone(),
                path: relative_path,
                parsed: parsed_file,
                phase_state: FilePhaseState {
                    parsing: parsing_status,
                    syntax_rules: PhaseStatus::Ok,
                    file_role_rules: PhaseStatus::Ok,
                    resolution: PhaseStatus::Ok,
                    semantic_lowering: PhaseStatus::Ok,
                },
            });
            if package_in_scope {
                append_safe_autofix_edits_for_file(
                    &mut safe_autofix_edits_by_workspace_relative_path,
                    &workspace_relative_key,
                    &parse_safe_autofixes,
                );
            }
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

        let parsed_unit_in_scope = is_parsed_unit_in_scope(
            parsed_unit,
            scope_is_workspace,
            scoped_package_paths.as_ref(),
        );
        for diagnostic in &syntax_rules_result.diagnostics {
            let rendered_diagnostic = render_diagnostic(
                DiagnosticPhase::SyntaxRules,
                display_path(&diagnostic_display_base.join(&parsed_unit.path)),
                diagnostic.clone(),
            );
            push_rendered_diagnostic(
                &mut rendered_diagnostics,
                &mut all_diagnostics_by_file,
                &parsed_unit.path,
                rendered_diagnostic,
                parsed_unit_in_scope,
            );
        }
        for diagnostic in &file_role_rules_result.diagnostics {
            let rendered_diagnostic = render_diagnostic(
                DiagnosticPhase::FileRoleRules,
                display_path(&diagnostic_display_base.join(&parsed_unit.path)),
                diagnostic.clone(),
            );
            push_rendered_diagnostic(
                &mut rendered_diagnostics,
                &mut all_diagnostics_by_file,
                &parsed_unit.path,
                rendered_diagnostic,
                parsed_unit_in_scope,
            );
        }
        if parsed_unit_in_scope {
            append_safe_autofix_edits_for_file(
                &mut safe_autofix_edits_by_workspace_relative_path,
                &path_to_key(&parsed_unit.path),
                &syntax_rules_result.safe_autofixes,
            );
            append_safe_autofix_edits_for_file(
                &mut safe_autofix_edits_by_workspace_relative_path,
                &path_to_key(&parsed_unit.path),
                &file_role_rules_result.safe_autofixes,
            );
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
            let parsed_unit_in_scope = is_parsed_unit_in_scope(
                parsed_unit,
                scope_is_workspace,
                scoped_package_paths.as_ref(),
            );
            let rendered_diagnostic = render_diagnostic(
                DiagnosticPhase::Resolution,
                display_path(&diagnostic_display_base.join(&path)),
                PhaseDiagnostic::new(message, span),
            );
            push_rendered_diagnostic(
                &mut rendered_diagnostics,
                &mut all_diagnostics_by_file,
                &path,
                rendered_diagnostic,
                parsed_unit_in_scope,
            );
        }
    }

    let package_id_by_path = collect_package_ids_by_path(&workspace);
    let mut semantic_file_by_path: BTreeMap<PathBuf, SemanticFile> = BTreeMap::new();
    for parsed_unit in &mut parsed_units {
        if !parsed_unit.phase_state.can_run_semantic_lowering() {
            continue;
        }
        let lowering_result = lower_parsed_file(&parsed_unit.parsed);
        let PhaseOutput {
            value,
            diagnostics,
            safe_autofixes,
            status,
        } = lowering_result;
        parsed_unit.phase_state.semantic_lowering = status;
        let parsed_unit_in_scope = is_parsed_unit_in_scope(
            parsed_unit,
            scope_is_workspace,
            scoped_package_paths.as_ref(),
        );
        for diagnostic in diagnostics {
            let rendered_diagnostic = render_diagnostic(
                DiagnosticPhase::SemanticLowering,
                display_path(&diagnostic_display_base.join(&parsed_unit.path)),
                diagnostic,
            );
            push_rendered_diagnostic(
                &mut rendered_diagnostics,
                &mut all_diagnostics_by_file,
                &parsed_unit.path,
                rendered_diagnostic,
                parsed_unit_in_scope,
            );
        }
        if matches!(parsed_unit.phase_state.semantic_lowering, PhaseStatus::Ok) {
            semantic_file_by_path.insert(parsed_unit.path.clone(), value);
        }
        if parsed_unit_in_scope {
            append_safe_autofix_edits_for_file(
                &mut safe_autofix_edits_by_workspace_relative_path,
                &path_to_key(&parsed_unit.path),
                &safe_autofixes,
            );
        }
    }
    let package_symbol_file_inputs: Vec<PackageSymbolFileInput<'_>> = parsed_units
        .iter()
        .filter_map(|unit| {
            semantic_file_by_path
                .get(&unit.path)
                .map(|semantic_file| PackageSymbolFileInput {
                    package_id: unit.package_id,
                    path: &unit.path,
                    semantic_file,
                })
        })
        .collect();
    let typecheck_resolved_imports =
        build_typecheck_resolved_imports(&resolved_imports, &package_id_by_path);
    let typed_public_symbol_table =
        build_typed_public_symbol_table(&package_symbol_file_inputs, &typecheck_resolved_imports);
    let imported_bindings_by_file =
        typed_public_symbol_table.imported_bindings_by_file(&typecheck_resolved_imports);
    let mut resolved_declarations_by_path = BTreeMap::new();

    for parsed_unit in &parsed_units {
        if !parsed_unit.phase_state.can_run_type_analysis() {
            continue;
        }
        let parsed_unit_in_scope = is_parsed_unit_in_scope(
            parsed_unit,
            scope_is_workspace,
            scoped_package_paths.as_ref(),
        );
        let imported_bindings = imported_bindings_by_file
            .get(&parsed_unit.path)
            .map_or(&[][..], Vec::as_slice);
        let Some(semantic_file) = semantic_file_by_path.get(&parsed_unit.path) else {
            continue;
        };
        let source_path = display_path(&workspace_root.join(&parsed_unit.path));
        let source_text = source_by_path.get(&source_path).map_or("", String::as_str);
        let type_analysis_result = type_analysis::check_package_unit(
            parsed_unit.package_id,
            &parsed_unit.package_path,
            source_text,
            semantic_file,
            imported_bindings,
        );
        if let Ok(resolved_declarations) = type_analysis_result.value {
            resolved_declarations_by_path.insert(parsed_unit.path.clone(), resolved_declarations);
        }
        for diagnostic in &type_analysis_result.diagnostics {
            let rendered_diagnostic = render_diagnostic(
                DiagnosticPhase::TypeAnalysis,
                display_path(&diagnostic_display_base.join(&parsed_unit.path)),
                diagnostic.clone(),
            );
            push_rendered_diagnostic(
                &mut rendered_diagnostics,
                &mut all_diagnostics_by_file,
                &parsed_unit.path,
                rendered_diagnostic,
                parsed_unit_in_scope,
            );
        }
        if parsed_unit_in_scope {
            append_safe_autofix_edits_for_file(
                &mut safe_autofix_edits_by_workspace_relative_path,
                &path_to_key(&parsed_unit.path),
                &type_analysis_result.safe_autofixes,
            );
        }
    }

    sort_rendered_diagnostics(&mut rendered_diagnostics);
    for diagnostics in all_diagnostics_by_file.values_mut() {
        sort_rendered_diagnostics(diagnostics);
    }
    let (
        safe_autofix_edit_count_by_workspace_relative_path,
        canonical_source_override_by_workspace_relative_path,
    ) = compute_safe_autofix_outputs(
        &source_by_workspace_relative_path_in_scope,
        &safe_autofix_edits_by_workspace_relative_path,
    );

    Ok(AnalyzedTarget {
        diagnostics: rendered_diagnostics,
        all_diagnostics_by_file,
        source_by_path,
        source_by_workspace_relative_path_in_scope,
        safe_autofix_edit_count_by_workspace_relative_path,
        canonical_source_override_by_workspace_relative_path,
        workspace_root,
        workspace,
        absolute_target_path,
        target_is_file,
        package_path_by_file,
        file_role_by_path,
        resolved_imports,
        resolved_declarations_by_path,
    })
}

fn compute_safe_autofix_outputs(
    source_by_workspace_relative_path: &BTreeMap<String, String>,
    safe_autofix_edits_by_workspace_relative_path: &BTreeMap<String, Vec<TextEdit>>,
) -> (BTreeMap<String, usize>, BTreeMap<String, String>) {
    let mut safe_autofix_edit_count_by_workspace_relative_path = BTreeMap::new();
    let mut canonical_source_override_by_workspace_relative_path = BTreeMap::new();

    for (workspace_relative_path, source_text) in source_by_workspace_relative_path {
        if !workspace_relative_path.ends_with(".copp") {
            continue;
        }
        let mut canonical_source_text = source_text.clone();
        let mut safe_autofix_edit_count = 0usize;
        if let Some(candidate_phase_safe_autofix_edits) =
            safe_autofix_edits_by_workspace_relative_path.get(workspace_relative_path)
        {
            let merged_phase_safe_autofix_edits =
                merge_text_edits(candidate_phase_safe_autofix_edits);
            safe_autofix_edit_count += merged_phase_safe_autofix_edits.accepted_text_edits.len();
            if !merged_phase_safe_autofix_edits
                .accepted_text_edits
                .is_empty()
                && let Ok(updated_text) = apply_text_edits(
                    &canonical_source_text,
                    &merged_phase_safe_autofix_edits.accepted_text_edits,
                )
            {
                canonical_source_text = updated_text;
            }
        }

        let formatter_text_edits = formatting_text_edits(&canonical_source_text);
        if !formatter_text_edits.is_empty()
            && let Ok(formatted_text) =
                apply_text_edits(&canonical_source_text, &formatter_text_edits)
        {
            safe_autofix_edit_count += formatter_text_edits.len();
            canonical_source_text = formatted_text;
        }

        if canonical_source_text == *source_text {
            continue;
        }

        safe_autofix_edit_count_by_workspace_relative_path.insert(
            workspace_relative_path.clone(),
            safe_autofix_edit_count.max(1),
        );
        canonical_source_override_by_workspace_relative_path
            .insert(workspace_relative_path.clone(), canonical_source_text);
    }

    (
        safe_autofix_edit_count_by_workspace_relative_path,
        canonical_source_override_by_workspace_relative_path,
    )
}

fn append_safe_autofix_edits_for_file(
    safe_autofix_edits_by_workspace_relative_path: &mut BTreeMap<String, Vec<TextEdit>>,
    workspace_relative_path: &str,
    safe_autofixes: &[SafeAutofix],
) {
    let file_safe_autofix_edits = safe_autofix_edits_by_workspace_relative_path
        .entry(workspace_relative_path.to_string())
        .or_default();
    for safe_autofix in safe_autofixes {
        file_safe_autofix_edits.extend(safe_autofix.text_edits.iter().cloned());
    }
}

fn resolve_workspace_root(
    path: &str,
    workspace_root_override: Option<&str>,
) -> Result<PathBuf, CompilerFailure> {
    let current_directory = std::env::current_dir().map_err(|error| CompilerFailure {
        kind: CompilerFailureKind::ReadSource,
        message: error.to_string(),
        path: Some(".".to_string()),
        details: Vec::new(),
    })?;

    if let Some(root_override) = workspace_root_override {
        let workspace_root =
            absolute_path_from_current_directory(&current_directory, root_override);
        ensure_valid_workspace_root_directory(&workspace_root, root_override)?;
        return Ok(workspace_root);
    }

    let absolute_target_path = absolute_path_from_current_directory(&current_directory, path);
    let search_start_path = marker_search_start_path(&absolute_target_path);
    let Some(workspace_root) = find_workspace_root_from_marker(&search_start_path) else {
        return Err(CompilerFailure {
            kind: CompilerFailureKind::WorkspaceRootMissingManifest,
            message: format!(
                "workspace root marker not found (expected {WORKSPACE_MARKER_FILENAME})"
            ),
            path: Some(path.to_string()),
            details: Vec::new(),
        });
    };
    ensure_valid_workspace_root_directory(&workspace_root, &path_to_key(&workspace_root))?;
    Ok(workspace_root)
}

fn absolute_path_from_current_directory(current_directory: &Path, raw_path: &str) -> PathBuf {
    let parsed_path = PathBuf::from(raw_path);
    if parsed_path.is_absolute() {
        parsed_path
    } else {
        current_directory.join(parsed_path)
    }
}

fn ensure_valid_workspace_root_directory(
    workspace_root: &Path,
    workspace_root_display: &str,
) -> Result<(), CompilerFailure> {
    let workspace_root_metadata =
        fs::metadata(workspace_root).map_err(|error| CompilerFailure {
            kind: CompilerFailureKind::InvalidWorkspaceRoot,
            message: format!("invalid workspace root: {error}"),
            path: Some(workspace_root_display.to_string()),
            details: Vec::new(),
        })?;
    if !workspace_root_metadata.is_dir() {
        return Err(CompilerFailure {
            kind: CompilerFailureKind::WorkspaceRootNotDirectory,
            message: "workspace root must be a directory".to_string(),
            path: Some(workspace_root_display.to_string()),
            details: Vec::new(),
        });
    }
    Ok(())
}

fn marker_search_start_path(absolute_target_path: &Path) -> PathBuf {
    if fs::metadata(absolute_target_path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
    {
        return absolute_target_path
            .parent()
            .map_or_else(|| absolute_target_path.to_path_buf(), Path::to_path_buf);
    }
    absolute_target_path.to_path_buf()
}

fn find_workspace_root_from_marker(search_start_path: &Path) -> Option<PathBuf> {
    let mut current_path = search_start_path.to_path_buf();
    loop {
        if current_path.join(WORKSPACE_MARKER_FILENAME).is_file() {
            return Some(current_path);
        }
        let parent = current_path.parent()?.to_path_buf();
        current_path = parent;
    }
}

fn collect_package_ids_by_path(workspace: &Workspace) -> BTreeMap<String, PackageId> {
    let mut package_id_by_path = BTreeMap::new();
    for package in workspace.packages() {
        package_id_by_path.insert(package.package_path.clone(), package.id);
    }
    package_id_by_path
}

fn is_parsed_unit_in_scope(
    parsed_unit: &ParsedUnit,
    scope_is_workspace: bool,
    scoped_package_paths: Option<&BTreeSet<String>>,
) -> bool {
    scope_is_workspace
        || scoped_package_paths.is_some_and(|scoped| scoped.contains(&parsed_unit.package_path))
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
            target_package_path: resolved_import.target_package_path.clone(),
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
            message: "target is not inside a package (missing PACKAGE.copp)".to_string(),
            path: Some(path_to_key(absolute_target_path)),
            details: Vec::new(),
        });
    };

    let owning_package_path = relative_package_path(workspace_root, &owning_package_root)
        .ok_or_else(|| CompilerFailure {
            kind: CompilerFailureKind::PackageNotFound,
            message: "target is not inside a package (missing PACKAGE.copp)".to_string(),
            path: Some(path_to_key(absolute_target_path)),
            details: Vec::new(),
        })?;
    if workspace.package_by_path(&owning_package_path).is_none() {
        return Err(CompilerFailure {
            kind: CompilerFailureKind::PackageNotFound,
            message: "target is not inside a package (missing PACKAGE.copp)".to_string(),
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
        if directory.join("PACKAGE.copp").is_file() {
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
        if directory.join("PACKAGE.copp").is_file() {
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

fn push_rendered_diagnostic(
    in_scope_diagnostics: &mut Vec<RenderedDiagnostic>,
    all_diagnostics_by_file: &mut BTreeMap<PathBuf, Vec<RenderedDiagnostic>>,
    file_path: &Path,
    rendered_diagnostic: RenderedDiagnostic,
    include_in_scope_output: bool,
) {
    if include_in_scope_output {
        in_scope_diagnostics.push(rendered_diagnostic.clone());
    }
    all_diagnostics_by_file
        .entry(file_path.to_path_buf())
        .or_default()
        .push(rendered_diagnostic);
}

fn sort_rendered_diagnostics(diagnostics: &mut [RenderedDiagnostic]) {
    diagnostics.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.span.line.cmp(&right.span.line))
            .then(left.span.column.cmp(&right.span.column))
            .then(left.message.cmp(&right.message))
            .then(left.phase.cmp(&right.phase))
    });
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
