use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use compiler__autofix_policy::{
    AutofixPolicyMode, AutofixPolicyOutcome, evaluate_autofix_policy,
    summarize_pending_safe_autofixes,
};
use compiler__check_pipeline::{
    analyze_target_with_workspace_root, analyze_target_with_workspace_root_and_overrides,
};
use compiler__cranelift_backend::{BuildArtifactIdentity, build_program, run_program};
use compiler__executable_lowering::lower_resolved_declarations_build_unit;
use compiler__phase_results::PhaseStatus;
use compiler__reports::{
    CompilerFailure, CompilerFailureDetail, CompilerFailureKind, RenderedDiagnostic,
};
use compiler__source::{FileRole, path_to_key};
use compiler__visibility::ResolvedImport;

pub struct BuildTargetResult {
    pub autofix_policy_outcome: Option<AutofixPolicyOutcome>,
    pub executable_path: Option<String>,
    pub success_message: Option<String>,
    pub safe_autofix_edit_count_by_workspace_relative_path: BTreeMap<String, usize>,
    pub analysis_result: Option<BuildAnalysisResult>,
    pub build: Result<(), CompilerFailure>,
}

pub struct BuildAnalysisResult {
    pub diagnostics: Vec<RenderedDiagnostic>,
    pub source_by_path: BTreeMap<String, String>,
}

#[must_use]
pub fn build_target_with_workspace_root(
    path: &str,
    workspace_root_override: Option<&str>,
    output_directory_override: Option<&str>,
    strict: bool,
) -> BuildTargetResult {
    let mut analyzed_target =
        match analyze_target_with_workspace_root(path, workspace_root_override) {
            Ok(value) => value,
            Err(error) => {
                return BuildTargetResult {
                    autofix_policy_outcome: None,
                    executable_path: None,
                    success_message: None,
                    safe_autofix_edit_count_by_workspace_relative_path: BTreeMap::new(),
                    analysis_result: None,
                    build: Err(error),
                };
            }
        };

    let safe_autofix_edit_count_by_workspace_relative_path = analyzed_target
        .safe_autofix_edits_by_workspace_relative_path
        .iter()
        .map(|(workspace_relative_path, text_edits)| {
            (workspace_relative_path.clone(), text_edits.len())
        })
        .collect::<BTreeMap<_, _>>();
    let autofix_policy_outcome =
        evaluate_safe_autofix_policy(strict, &safe_autofix_edit_count_by_workspace_relative_path);

    if matches!(
        autofix_policy_outcome,
        AutofixPolicyOutcome::FailInStrictMode { .. }
    ) {
        return BuildTargetResult {
            autofix_policy_outcome: Some(autofix_policy_outcome),
            executable_path: None,
            success_message: None,
            safe_autofix_edit_count_by_workspace_relative_path:
                safe_autofix_edit_count_by_workspace_relative_path.clone(),
            analysis_result: None,
            build: Err(build_failed_from_pending_safe_autofixes(
                &safe_autofix_edit_count_by_workspace_relative_path,
            )),
        };
    }

    if !analyzed_target
        .canonical_source_override_by_workspace_relative_path
        .is_empty()
    {
        analyzed_target = match analyze_target_with_workspace_root_and_overrides(
            path,
            workspace_root_override,
            &analyzed_target.canonical_source_override_by_workspace_relative_path,
        ) {
            Ok(value) => value,
            Err(error) => {
                return BuildTargetResult {
                    autofix_policy_outcome: Some(autofix_policy_outcome),
                    executable_path: None,
                    success_message: None,
                    safe_autofix_edit_count_by_workspace_relative_path:
                        safe_autofix_edit_count_by_workspace_relative_path.clone(),
                    analysis_result: None,
                    build: Err(error),
                };
            }
        };
    }
    let binary_entrypoint = if analyzed_target.target_is_file
        && FileRole::from_path(&analyzed_target.absolute_target_path)
            == Some(FileRole::BinaryEntrypoint)
    {
        path_to_relative_workspace_path(
            analyzed_target.workspace.root_directory(),
            &analyzed_target.absolute_target_path,
        )
    } else {
        return BuildTargetResult {
            autofix_policy_outcome: Some(autofix_policy_outcome),
            executable_path: None,
            success_message: Some(
                "analysis succeeded; package/library/test artifact generation is not implemented yet"
                    .to_string(),
            ),
            safe_autofix_edit_count_by_workspace_relative_path:
                safe_autofix_edit_count_by_workspace_relative_path.clone(),
            analysis_result: Some(BuildAnalysisResult {
                diagnostics: analyzed_target.diagnostics,
                source_by_path: analyzed_target.source_by_path,
            }),
            build: Ok(()),
        };
    };
    if !analyzed_target.diagnostics.is_empty() {
        return BuildTargetResult {
            autofix_policy_outcome: Some(autofix_policy_outcome),
            executable_path: None,
            success_message: None,
            safe_autofix_edit_count_by_workspace_relative_path:
                safe_autofix_edit_count_by_workspace_relative_path.clone(),
            analysis_result: None,
            build: Err(build_failed_from_rendered_diagnostics(
                &analyzed_target.diagnostics,
            )),
        };
    }
    let Some(binary_entrypoint_resolved_declarations) = analyzed_target
        .resolved_declarations_by_path
        .get(&binary_entrypoint)
    else {
        return BuildTargetResult {
            autofix_policy_outcome: Some(autofix_policy_outcome),
            executable_path: None,
            success_message: None,
            safe_autofix_edit_count_by_workspace_relative_path:
                safe_autofix_edit_count_by_workspace_relative_path.clone(),
            analysis_result: None,
            build: Err(CompilerFailure {
                kind: CompilerFailureKind::BuildFailed,
                message: "missing resolved declarations for binary entrypoint".to_string(),
                path: Some(path_to_key(&binary_entrypoint)),
                details: Vec::new(),
            }),
        };
    };
    let Some(binary_entrypoint_package_path) =
        analyzed_target.package_path_by_file.get(&binary_entrypoint)
    else {
        return BuildTargetResult {
            autofix_policy_outcome: Some(autofix_policy_outcome),
            executable_path: None,
            success_message: None,
            safe_autofix_edit_count_by_workspace_relative_path:
                safe_autofix_edit_count_by_workspace_relative_path.clone(),
            analysis_result: None,
            build: Err(CompilerFailure {
                kind: CompilerFailureKind::BuildFailed,
                message: "missing package ownership for binary entrypoint".to_string(),
                path: Some(path_to_key(&binary_entrypoint)),
                details: Vec::new(),
            }),
        };
    };
    let reachable_package_paths = package_dependency_closure(
        binary_entrypoint_package_path,
        &analyzed_target.resolved_imports,
    );
    let mut reachable_diagnostics = Vec::new();
    for (file_path, file_diagnostics) in &analyzed_target.all_diagnostics_by_file {
        let Some(package_path) = analyzed_target.package_path_by_file.get(file_path) else {
            continue;
        };
        if !reachable_package_paths.contains(package_path) {
            continue;
        }
        reachable_diagnostics.extend(file_diagnostics.iter().cloned());
    }
    sort_rendered_diagnostics(&mut reachable_diagnostics);
    if !reachable_diagnostics.is_empty() {
        return BuildTargetResult {
            autofix_policy_outcome: Some(autofix_policy_outcome),
            executable_path: None,
            success_message: None,
            safe_autofix_edit_count_by_workspace_relative_path:
                safe_autofix_edit_count_by_workspace_relative_path.clone(),
            analysis_result: None,
            build: Err(build_failed_from_rendered_diagnostics(
                &reachable_diagnostics,
            )),
        };
    }
    let dependency_library_resolved_declarations = analyzed_target
        .resolved_declarations_by_path
        .iter()
        .filter_map(|(file_path, resolved_declarations)| {
            if file_path == &binary_entrypoint {
                return None;
            }
            if analyzed_target.file_role_by_path.get(file_path) != Some(&FileRole::Library) {
                return None;
            }
            let file_package_path = analyzed_target.package_path_by_file.get(file_path)?;
            if !reachable_package_paths.contains(file_package_path) {
                return None;
            }
            Some(resolved_declarations)
        })
        .collect::<Vec<_>>();
    let executable_lowering_result = lower_resolved_declarations_build_unit(
        binary_entrypoint_resolved_declarations,
        &dependency_library_resolved_declarations,
    );
    if !matches!(executable_lowering_result.status, PhaseStatus::Ok) {
        return BuildTargetResult {
            autofix_policy_outcome: Some(autofix_policy_outcome),
            executable_path: None,
            success_message: None,
            safe_autofix_edit_count_by_workspace_relative_path:
                safe_autofix_edit_count_by_workspace_relative_path.clone(),
            analysis_result: None,
            build: Err(CompilerFailure {
                kind: CompilerFailureKind::BuildFailed,
                message: "build mode does not support this program yet".to_string(),
                path: Some(path_to_key(&binary_entrypoint)),
                details: executable_lowering_result
                    .diagnostics
                    .into_iter()
                    .map(|diagnostic| CompilerFailureDetail {
                        message: format!(
                            "{} (line {}, column {})",
                            diagnostic.message, diagnostic.span.line, diagnostic.span.column
                        ),
                        path: Some(path_to_key(&binary_entrypoint)),
                    })
                    .collect(),
            }),
        };
    }

    let build_directory = if let Some(output_directory) = output_directory_override {
        let parsed_output_directory = PathBuf::from(output_directory);
        if parsed_output_directory.is_absolute() {
            parsed_output_directory
        } else {
            analyzed_target.workspace_root.join(parsed_output_directory)
        }
    } else {
        analyzed_target
            .workspace_root
            .join(".coppice")
            .join("build")
    };
    let executable_stem = match executable_stem_for_binary_entrypoint(&binary_entrypoint) {
        Ok(value) => value,
        Err(error) => {
            return BuildTargetResult {
                autofix_policy_outcome: Some(autofix_policy_outcome),
                executable_path: None,
                success_message: None,
                safe_autofix_edit_count_by_workspace_relative_path:
                    safe_autofix_edit_count_by_workspace_relative_path.clone(),
                analysis_result: None,
                build: Err(error),
            };
        }
    };
    let built_program = match build_program(
        &executable_lowering_result.value,
        &build_directory,
        &BuildArtifactIdentity { executable_stem },
    ) {
        Ok(value) => value,
        Err(error) => {
            return BuildTargetResult {
                autofix_policy_outcome: Some(autofix_policy_outcome),
                executable_path: None,
                success_message: None,
                safe_autofix_edit_count_by_workspace_relative_path:
                    safe_autofix_edit_count_by_workspace_relative_path.clone(),
                analysis_result: None,
                build: Err(error),
            };
        }
    };

    BuildTargetResult {
        autofix_policy_outcome: Some(autofix_policy_outcome),
        executable_path: Some(display_path(&built_program.binary_path)),
        success_message: None,
        safe_autofix_edit_count_by_workspace_relative_path,
        analysis_result: None,
        build: Ok(()),
    }
}

pub struct RunTargetResult {
    pub autofix_policy_outcome: Option<AutofixPolicyOutcome>,
    pub run: Result<i32, CompilerFailure>,
}

#[must_use]
pub fn run_target_with_workspace_root(
    path: &str,
    workspace_root_override: Option<&str>,
    output_directory_override: Option<&str>,
    strict: bool,
) -> RunTargetResult {
    let built_result = build_target_with_workspace_root(
        path,
        workspace_root_override,
        output_directory_override,
        strict,
    );
    let BuildTargetResult {
        autofix_policy_outcome,
        executable_path,
        success_message: _success_message,
        safe_autofix_edit_count_by_workspace_relative_path:
            _safe_autofix_edit_count_by_workspace_relative_path,
        analysis_result: _analysis_result,
        build,
    } = built_result;

    match (autofix_policy_outcome, build) {
        (None, Err(error)) => RunTargetResult {
            autofix_policy_outcome: None,
            run: Err(error),
        },
        (Some(autofix_policy_outcome), Err(error)) => RunTargetResult {
            autofix_policy_outcome: Some(autofix_policy_outcome),
            run: Err(error),
        },
        (Some(autofix_policy_outcome), Ok(())) => {
            let Some(executable_path) = executable_path else {
                return RunTargetResult {
                    autofix_policy_outcome: Some(autofix_policy_outcome),
                    run: Err(CompilerFailure {
                        kind: CompilerFailureKind::RunFailed,
                        message: "build/run target must be a .bin.copp file".to_string(),
                        path: None,
                        details: Vec::new(),
                    }),
                };
            };
            RunTargetResult {
                autofix_policy_outcome: Some(autofix_policy_outcome),
                run: run_program(Path::new(&executable_path)),
            }
        }
        (None, Ok(())) => panic!("autofix policy outcome missing for successful build"),
    }
}

fn evaluate_safe_autofix_policy(
    strict: bool,
    safe_autofix_edit_count_by_workspace_relative_path: &BTreeMap<String, usize>,
) -> AutofixPolicyOutcome {
    evaluate_autofix_policy(
        if strict {
            AutofixPolicyMode::Strict
        } else {
            AutofixPolicyMode::NonStrict
        },
        summarize_pending_safe_autofixes(
            safe_autofix_edit_count_by_workspace_relative_path
                .values()
                .copied(),
        ),
    )
}

fn package_dependency_closure(
    root_package_path: &str,
    resolved_imports: &[ResolvedImport],
) -> BTreeSet<String> {
    let mut imported_package_paths_by_source_package = BTreeMap::<String, BTreeSet<String>>::new();
    for resolved_import in resolved_imports {
        imported_package_paths_by_source_package
            .entry(resolved_import.source_package_path.clone())
            .or_default()
            .insert(resolved_import.target_package_path.clone());
    }

    let mut visited_package_paths = BTreeSet::new();
    let mut package_paths_to_visit = vec![root_package_path.to_string()];
    while let Some(package_path) = package_paths_to_visit.pop() {
        if !visited_package_paths.insert(package_path.clone()) {
            continue;
        }
        if let Some(imported_package_paths) =
            imported_package_paths_by_source_package.get(&package_path)
        {
            package_paths_to_visit.extend(imported_package_paths.iter().cloned());
        }
    }

    visited_package_paths
}

fn path_to_relative_workspace_path(workspace_root: &Path, absolute_path: &Path) -> PathBuf {
    absolute_path
        .strip_prefix(workspace_root)
        .map_or_else(|_| absolute_path.to_path_buf(), Path::to_path_buf)
}

fn build_failed_from_rendered_diagnostics(diagnostics: &[RenderedDiagnostic]) -> CompilerFailure {
    CompilerFailure {
        kind: CompilerFailureKind::BuildFailed,
        message: "build failed due to diagnostics".to_string(),
        path: None,
        details: diagnostics
            .iter()
            .map(|diagnostic| CompilerFailureDetail {
                message: format!(
                    "{} ({}:{}:{})",
                    diagnostic.message,
                    diagnostic.path,
                    diagnostic.span.line,
                    diagnostic.span.column
                ),
                path: Some(diagnostic.path.clone()),
            })
            .collect(),
    }
}

fn build_failed_from_pending_safe_autofixes(
    safe_autofix_edit_count_by_workspace_relative_path: &BTreeMap<String, usize>,
) -> CompilerFailure {
    CompilerFailure {
        kind: CompilerFailureKind::BuildFailed,
        message: "build failed due to pending safe autofixes".to_string(),
        path: None,
        details: safe_autofix_edit_count_by_workspace_relative_path
            .iter()
            .map(
                |(workspace_relative_path, text_edit_count)| CompilerFailureDetail {
                    message: format!("{text_edit_count} pending safe autofix edits"),
                    path: Some(workspace_relative_path.clone()),
                },
            )
            .collect(),
    }
}

fn executable_stem_for_binary_entrypoint(
    binary_entrypoint: &Path,
) -> Result<String, CompilerFailure> {
    let file_name = binary_entrypoint
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CompilerFailure {
            kind: CompilerFailureKind::BuildFailed,
            message: "binary entrypoint path has invalid file name".to_string(),
            path: Some(path_to_key(binary_entrypoint)),
            details: Vec::new(),
        })?;
    let Some(executable_stem) = file_name.strip_suffix(".bin.copp") else {
        return Err(CompilerFailure {
            kind: CompilerFailureKind::BuildFailed,
            message: "binary entrypoint file must end with .bin.copp".to_string(),
            path: Some(path_to_key(binary_entrypoint)),
            details: Vec::new(),
        });
    };
    if executable_stem.is_empty() {
        return Err(CompilerFailure {
            kind: CompilerFailureKind::BuildFailed,
            message: "binary entrypoint file name must include executable name before .bin.copp"
                .to_string(),
            path: Some(path_to_key(binary_entrypoint)),
            details: Vec::new(),
        });
    }
    Ok(executable_stem.to_string())
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
