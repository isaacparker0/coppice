use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use compiler__check_pipeline::analyze_target_with_workspace_root;
use compiler__cranelift_backend::{BuildArtifactIdentity, build_program, run_program};
use compiler__executable_lowering::lower_resolved_declarations_build_unit;
use compiler__phase_results::PhaseStatus;
use compiler__reports::{
    CompilerFailure, CompilerFailureDetail, CompilerFailureKind, RenderedDiagnostic,
};
use compiler__source::{FileRole, path_to_key};
use compiler__visibility::ResolvedImport;
use compiler__workspace::Workspace;

pub struct BuiltTarget {
    pub executable_path: String,
}

pub fn build_target_with_workspace_root(
    path: &str,
    workspace_root_override: Option<&str>,
    output_directory_override: Option<&str>,
) -> Result<BuiltTarget, CompilerFailure> {
    let analyzed_target = analyze_target_with_workspace_root(path, workspace_root_override)?;
    if !analyzed_target.diagnostics.is_empty() {
        return Err(build_failed_from_rendered_diagnostics(
            &analyzed_target.diagnostics,
        ));
    }
    let binary_entrypoint = find_single_binary_entrypoint(
        &analyzed_target.workspace,
        &analyzed_target.absolute_target_path,
        analyzed_target.target_is_file,
    )?;
    let binary_entrypoint_resolved_declarations = analyzed_target
        .resolved_declarations_by_path
        .get(&binary_entrypoint)
        .ok_or_else(|| CompilerFailure {
            kind: CompilerFailureKind::BuildFailed,
            message: "missing resolved declarations for binary entrypoint".to_string(),
            path: Some(path_to_key(&binary_entrypoint)),
            details: Vec::new(),
        })?;
    let binary_entrypoint_package_path = analyzed_target
        .package_path_by_file
        .get(&binary_entrypoint)
        .ok_or_else(|| CompilerFailure {
            kind: CompilerFailureKind::BuildFailed,
            message: "missing package ownership for binary entrypoint".to_string(),
            path: Some(path_to_key(&binary_entrypoint)),
            details: Vec::new(),
        })?;
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
        return Err(build_failed_from_rendered_diagnostics(
            &reachable_diagnostics,
        ));
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
        return Err(CompilerFailure {
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
        });
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
    let executable_stem = executable_stem_for_binary_entrypoint(&binary_entrypoint)?;
    let built_program = build_program(
        &executable_lowering_result.value,
        &build_directory,
        &BuildArtifactIdentity { executable_stem },
    )?;

    Ok(BuiltTarget {
        executable_path: display_path(&built_program.binary_path),
    })
}

pub fn run_target_with_workspace_root(
    path: &str,
    workspace_root_override: Option<&str>,
    output_directory_override: Option<&str>,
) -> Result<i32, CompilerFailure> {
    let built =
        build_target_with_workspace_root(path, workspace_root_override, output_directory_override)?;
    run_program(Path::new(&built.executable_path))
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

fn find_single_binary_entrypoint(
    workspace: &Workspace,
    absolute_target_path: &Path,
    target_is_file: bool,
) -> Result<PathBuf, CompilerFailure> {
    if !target_is_file {
        return Err(CompilerFailure {
            kind: CompilerFailureKind::BuildFailed,
            message: "build/run target must be an explicit .bin.copp file".to_string(),
            path: Some(path_to_key(absolute_target_path)),
            details: Vec::new(),
        });
    }

    let role = FileRole::from_path(absolute_target_path).ok_or_else(|| CompilerFailure {
        kind: CompilerFailureKind::InvalidCheckTarget,
        message: "target file is not a Coppice source file".to_string(),
        path: Some(path_to_key(absolute_target_path)),
        details: Vec::new(),
    })?;
    if role != FileRole::BinaryEntrypoint {
        return Err(CompilerFailure {
            kind: CompilerFailureKind::BuildFailed,
            message: "build/run target must be a .bin.copp file".to_string(),
            path: Some(path_to_key(absolute_target_path)),
            details: Vec::new(),
        });
    }

    Ok(path_to_relative_workspace_path(
        workspace.root_directory(),
        absolute_target_path,
    ))
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
