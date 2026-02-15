use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use compiler__diagnostics::Diagnostic;
use compiler__source::{FileRole, Span};
use compiler__syntax::{Declaration, ImportDeclaration, ParsedFile, Visibility};

pub struct PackageFile<'a> {
    pub package_path: &'a str,
    pub path: &'a Path,
    pub parsed: &'a ParsedFile,
}

pub struct PackageDiagnostic {
    pub path: PathBuf,
    pub diagnostic: Diagnostic,
}

struct TopLevelSymbol {
    name: String,
    visibility: Visibility,
    span: Span,
}

#[derive(Default)]
struct PackageSymbols {
    declared: BTreeSet<String>,
    package_visible: BTreeSet<String>,
    exported: BTreeSet<String>,
}

pub fn check_package(files: &[PackageFile<'_>], diagnostics: &mut Vec<PackageDiagnostic>) {
    let mut ordered_files: Vec<&PackageFile<'_>> = files.iter().collect();
    ordered_files.sort_by_key(|file| {
        (
            file.package_path.to_string(),
            file.path.to_string_lossy().replace('\\', "/"),
        )
    });

    let mut symbols_by_package: BTreeMap<String, PackageSymbols> = BTreeMap::new();

    for file in &ordered_files {
        if file.parsed.role != FileRole::Library {
            continue;
        }

        let package_symbols = symbols_by_package
            .entry(file.package_path.to_string())
            .or_default();
        for declaration in &file.parsed.declarations {
            let Some(symbol) = top_level_symbol(declaration) else {
                continue;
            };
            package_symbols.declared.insert(symbol.name.clone());
            if symbol.visibility == Visibility::Public
                && !package_symbols.package_visible.insert(symbol.name.clone())
            {
                diagnostics.push(PackageDiagnostic {
                    path: file.path.to_path_buf(),
                    diagnostic: Diagnostic::new(
                        format!("duplicate package-visible symbol '{}'", symbol.name),
                        symbol.span,
                    ),
                });
            }
        }
    }

    for file in &ordered_files {
        if file.parsed.role != FileRole::PackageManifest {
            continue;
        }
        let package_symbols = symbols_by_package
            .entry(file.package_path.to_string())
            .or_default();
        for declaration in &file.parsed.declarations {
            let Declaration::Exports(exports) = declaration else {
                continue;
            };
            for member in &exports.members {
                let name = member.name.clone();
                if !package_symbols.exported.insert(name.clone()) {
                    diagnostics.push(PackageDiagnostic {
                        path: file.path.to_path_buf(),
                        diagnostic: Diagnostic::new(
                            format!("duplicate exported symbol '{name}'"),
                            member.span.clone(),
                        ),
                    });
                    continue;
                }
                if !package_symbols.declared.contains(&name) {
                    diagnostics.push(PackageDiagnostic {
                        path: file.path.to_path_buf(),
                        diagnostic: Diagnostic::new(
                            format!("exported symbol '{name}' is not declared in this package"),
                            member.span.clone(),
                        ),
                    });
                    continue;
                }
                if !package_symbols.package_visible.contains(&name) {
                    diagnostics.push(PackageDiagnostic {
                        path: file.path.to_path_buf(),
                        diagnostic: Diagnostic::new(
                            format!("exported symbol '{name}' must be declared public"),
                            member.span.clone(),
                        ),
                    });
                }
            }
        }
    }

    for file in &ordered_files {
        if file.parsed.role == FileRole::PackageManifest {
            continue;
        }
        for declaration in &file.parsed.declarations {
            let Declaration::Import(import_declaration) = declaration else {
                continue;
            };
            check_import_declaration(
                file.package_path,
                file.path,
                import_declaration,
                &symbols_by_package,
                diagnostics,
            );
        }
    }
}

fn top_level_symbol(declaration: &Declaration) -> Option<TopLevelSymbol> {
    match declaration {
        Declaration::Type(type_declaration) => Some(TopLevelSymbol {
            name: type_declaration.name.clone(),
            visibility: type_declaration.visibility,
            span: type_declaration.span.clone(),
        }),
        Declaration::Constant(constant_declaration) => Some(TopLevelSymbol {
            name: constant_declaration.name.clone(),
            visibility: constant_declaration.visibility,
            span: constant_declaration.span.clone(),
        }),
        Declaration::Function(function_declaration) => Some(TopLevelSymbol {
            name: function_declaration.name.clone(),
            visibility: function_declaration.visibility,
            span: function_declaration.span.clone(),
        }),
        Declaration::Import(_) | Declaration::Exports(_) => None,
    }
}

fn check_import_declaration(
    source_package_path: &str,
    source_path: &Path,
    import_declaration: &ImportDeclaration,
    symbols_by_package: &BTreeMap<String, PackageSymbols>,
    diagnostics: &mut Vec<PackageDiagnostic>,
) {
    let (target_package_key, same_package) =
        match resolve_import_package_path(source_package_path, &import_declaration.package_path) {
            Ok(result) => result,
            Err(message) => {
                diagnostics.push(PackageDiagnostic {
                    path: source_path.to_path_buf(),
                    diagnostic: Diagnostic::new(message, import_declaration.span.clone()),
                });
                return;
            }
        };

    let Some(target_package_symbols) = symbols_by_package.get(&target_package_key) else {
        diagnostics.push(PackageDiagnostic {
            path: source_path.to_path_buf(),
            diagnostic: Diagnostic::new(
                format!("unknown package '{}'", import_declaration.package_path),
                import_declaration.span.clone(),
            ),
        });
        return;
    };

    for member in &import_declaration.members {
        let name = &member.name;
        if !target_package_symbols.declared.contains(name) {
            diagnostics.push(PackageDiagnostic {
                path: source_path.to_path_buf(),
                diagnostic: Diagnostic::new(
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
                path: source_path.to_path_buf(),
                diagnostic: Diagnostic::new(
                    format!(
                        "imported symbol '{name}' in package '{}' must be declared public",
                        import_declaration.package_path
                    ),
                    member.span.clone(),
                ),
            });
            continue;
        }
        if !same_package && !target_package_symbols.exported.contains(name) {
            diagnostics.push(PackageDiagnostic {
                path: source_path.to_path_buf(),
                diagnostic: Diagnostic::new(
                    format!(
                        "imported symbol '{name}' in package '{}' is not exported",
                        import_declaration.package_path
                    ),
                    member.span.clone(),
                ),
            });
        }
    }
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
