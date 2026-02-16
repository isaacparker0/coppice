use std::collections::BTreeMap;
use std::path::PathBuf;

use compiler__diagnostics::PhaseDiagnostic;
use compiler__exports::ExportsByPackage;
use compiler__source::Span;
use compiler__symbols::{PackageDiagnostic, PackageFile, SymbolsByPackage};
use compiler__syntax::{Declaration, ImportDeclaration, ImportMember};

pub struct ResolvedImportBinding {
    pub imported_name: String,
    pub local_name: String,
    pub span: Span,
}

pub struct ResolvedImport {
    pub source_package_path: String,
    pub source_path: PathBuf,
    pub import_span: Span,
    pub target_package_path: String,
    pub bindings: Vec<ResolvedImportBinding>,
}

pub fn resolve_imports(
    files: &[PackageFile<'_>],
    symbols_by_package: &SymbolsByPackage,
    exports_by_package: &ExportsByPackage,
    diagnostics: &mut Vec<PackageDiagnostic>,
) -> Vec<ResolvedImport> {
    let mut ordered_files: Vec<&PackageFile<'_>> = files.iter().collect();
    ordered_files.sort_by_key(|file| {
        (
            file.package_path.to_string(),
            file.path.to_string_lossy().replace('\\', "/"),
        )
    });

    let mut resolved_imports = Vec::new();

    for file in &ordered_files {
        for declaration in file.parsed.top_level_declarations() {
            let Declaration::Import(import_declaration) = declaration else {
                continue;
            };
            let resolved = resolve_import_declaration(
                file,
                import_declaration,
                symbols_by_package,
                exports_by_package,
                diagnostics,
            );
            if let Some(resolved) = resolved {
                resolved_imports.push(resolved);
            }
        }
    }

    resolved_imports
}

fn resolve_import_declaration(
    file: &PackageFile<'_>,
    import_declaration: &ImportDeclaration,
    symbols_by_package: &SymbolsByPackage,
    exports_by_package: &ExportsByPackage,
    diagnostics: &mut Vec<PackageDiagnostic>,
) -> Option<ResolvedImport> {
    let (target_package_path, same_package) =
        match resolve_import_package_path(file.package_path, &import_declaration.package_path) {
            Ok(result) => result,
            Err(message) => {
                diagnostics.push(PackageDiagnostic {
                    path: file.path.to_path_buf(),
                    diagnostic: PhaseDiagnostic::new(message, import_declaration.span.clone()),
                });
                return None;
            }
        };

    let Some(target_package_symbols) = symbols_by_package.get(&target_package_path) else {
        diagnostics.push(PackageDiagnostic {
            path: file.path.to_path_buf(),
            diagnostic: PhaseDiagnostic::new(
                format!("unknown package '{}'", import_declaration.package_path),
                import_declaration.span.clone(),
            ),
        });
        return None;
    };

    let exported_symbols = exports_by_package.get(&target_package_path);
    let mut bindings = Vec::new();
    for member in &import_declaration.members {
        let name = &member.name;
        if !target_package_symbols.declared.contains(name) {
            diagnostics.push(PackageDiagnostic {
                path: file.path.to_path_buf(),
                diagnostic: PhaseDiagnostic::new(
                    format!(
                        "imported symbol '{name}' is not declared in package '{}'",
                        import_declaration.package_path
                    ),
                    member.span.clone(),
                ),
            });
            continue;
        }
        if !target_package_symbols.package_visible.contains(name) {
            diagnostics.push(PackageDiagnostic {
                path: file.path.to_path_buf(),
                diagnostic: PhaseDiagnostic::new(
                    format!(
                        "imported symbol '{name}' in package '{}' must be declared public",
                        import_declaration.package_path
                    ),
                    member.span.clone(),
                ),
            });
            continue;
        }
        if !same_package && !is_exported(name, exported_symbols) {
            diagnostics.push(PackageDiagnostic {
                path: file.path.to_path_buf(),
                diagnostic: PhaseDiagnostic::new(
                    format!(
                        "imported symbol '{name}' in package '{}' is not exported",
                        import_declaration.package_path
                    ),
                    member.span.clone(),
                ),
            });
            continue;
        }

        bindings.push(ResolvedImportBinding {
            imported_name: name.clone(),
            local_name: import_local_name(member).to_string(),
            span: member.alias_span.clone().unwrap_or(member.span.clone()),
        });
    }

    Some(ResolvedImport {
        source_package_path: file.package_path.to_string(),
        source_path: file.path.to_path_buf(),
        import_span: import_declaration.span.clone(),
        target_package_path,
        bindings,
    })
}

fn resolve_import_package_path(
    source_package_path: &str,
    import_package_path: &str,
) -> Result<(String, bool), String> {
    if import_package_path == "workspace" {
        return Ok((String::new(), source_package_path.is_empty()));
    }
    if let Some(workspace_path) = import_package_path.strip_prefix("workspace/") {
        return Ok((
            workspace_path.to_string(),
            source_package_path == workspace_path,
        ));
    }
    if import_package_path.starts_with("std/") || import_package_path.starts_with("external/") {
        return Ok((import_package_path.to_string(), false));
    }
    Err("import path must start with import origin 'workspace', 'std/', or 'external/'".to_string())
}

fn import_local_name(member: &ImportMember) -> &str {
    member.alias.as_deref().unwrap_or(&member.name)
}

fn is_exported(name: &str, exported_symbols: Option<&std::collections::BTreeSet<String>>) -> bool {
    exported_symbols.is_some_and(|symbols| symbols.contains(name))
}

#[must_use]
pub fn resolved_bindings_by_file(
    resolved_imports: &[ResolvedImport],
) -> BTreeMap<PathBuf, Vec<ResolvedImportBinding>> {
    let mut bindings_by_file: BTreeMap<PathBuf, Vec<ResolvedImportBinding>> = BTreeMap::new();
    for import in resolved_imports {
        bindings_by_file
            .entry(import.source_path.clone())
            .or_default()
            .extend(import.bindings.iter().map(|binding| ResolvedImportBinding {
                imported_name: binding.imported_name.clone(),
                local_name: binding.local_name.clone(),
                span: binding.span.clone(),
            }));
    }
    bindings_by_file
}
