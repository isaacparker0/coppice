use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use compiler__binding as binding;
use compiler__diagnostics::FileScopedDiagnostic;
use compiler__exports as exports;
use compiler__package_graph as package_graph;
use compiler__phase_results::{FileScopedPhaseOutput, PhaseStatus};
use compiler__symbols::{self as symbols, PackageFile};
use compiler__syntax::SyntaxParsedFile;
use compiler__visibility::{self as visibility, ResolvedImport};

pub struct ResolutionFile<'a> {
    pub package_path: &'a str,
    pub path: &'a Path,
    pub parsed: &'a SyntaxParsedFile,
}

pub struct ResolutionArtifacts {
    pub resolved_imports: Vec<ResolvedImport>,
}

#[must_use]
pub fn resolve_files(files: &[ResolutionFile<'_>]) -> FileScopedPhaseOutput<ResolutionArtifacts> {
    let package_files: Vec<PackageFile<'_>> = files
        .iter()
        .map(|file| PackageFile {
            package_path: file.package_path,
            path: file.path,
            parsed: file.parsed,
        })
        .collect();

    let mut package_diagnostics = Vec::new();
    let symbols_by_package = symbols::collect_symbols(&package_files, &mut package_diagnostics);
    let exports_by_package = exports::build_exports(
        &package_files,
        &symbols_by_package,
        &mut package_diagnostics,
    );
    let resolved_imports = visibility::resolve_imports(
        &package_files,
        &symbols_by_package,
        &exports_by_package,
        &mut package_diagnostics,
    );
    package_graph::check_cycles(&resolved_imports, &mut package_diagnostics);
    let cycle_package_paths = package_graph::package_paths_in_cycle(&resolved_imports);
    let bindings_by_file = visibility::resolved_bindings_by_file(&resolved_imports);
    binding::check_bindings(&package_files, &bindings_by_file, &mut package_diagnostics);

    let mut status_by_file: BTreeMap<PathBuf, PhaseStatus> = package_files
        .iter()
        .map(|file| (file.path.to_path_buf(), PhaseStatus::Ok))
        .collect();
    for diagnostic in &package_diagnostics {
        status_by_file.insert(
            diagnostic.path.clone(),
            PhaseStatus::PreventsDownstreamExecution,
        );
    }
    for file in &package_files {
        if cycle_package_paths.contains(file.package_path) {
            status_by_file.insert(
                file.path.to_path_buf(),
                PhaseStatus::PreventsDownstreamExecution,
            );
        }
    }

    let diagnostics = package_diagnostics
        .into_iter()
        .map(|diagnostic| FileScopedDiagnostic {
            path: diagnostic.path,
            message: diagnostic.diagnostic.message,
            span: diagnostic.diagnostic.span,
        })
        .collect();
    FileScopedPhaseOutput {
        value: ResolutionArtifacts { resolved_imports },
        diagnostics,
        status_by_file,
    }
}
