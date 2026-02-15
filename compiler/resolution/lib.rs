use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use compiler__diagnostics::Diagnostic;
use compiler__source::Span;
use compiler__syntax::{Declaration, ParsedFile, Visibility};

pub struct PackageFile<'a> {
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

pub fn check_package(files: &[PackageFile<'_>], diagnostics: &mut Vec<PackageDiagnostic>) {
    let mut ordered_files: Vec<&PackageFile<'_>> = files.iter().collect();
    ordered_files.sort_by_key(|file| file.path.to_string_lossy().replace('\\', "/"));

    let mut has_any_top_level_symbol = BTreeSet::new();
    let mut package_visible_symbols = BTreeSet::new();

    for file in &ordered_files {
        if file.parsed.role == compiler__source::FileRole::PackageManifest {
            continue;
        }
        for declaration in &file.parsed.declarations {
            let Some(symbol) = top_level_symbol(declaration) else {
                continue;
            };
            has_any_top_level_symbol.insert(symbol.name.clone());
            if symbol.visibility == Visibility::Public
                && !package_visible_symbols.insert(symbol.name.clone())
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

    let mut exported_symbols = BTreeSet::new();
    for file in &ordered_files {
        if file.parsed.role != compiler__source::FileRole::PackageManifest {
            continue;
        }
        for declaration in &file.parsed.declarations {
            let Declaration::Exports(exports) = declaration else {
                continue;
            };
            for member in &exports.members {
                let name = member.name.clone();
                if !exported_symbols.insert(name.clone()) {
                    diagnostics.push(PackageDiagnostic {
                        path: file.path.to_path_buf(),
                        diagnostic: Diagnostic::new(
                            format!("duplicate exported symbol '{name}'"),
                            member.span.clone(),
                        ),
                    });
                    continue;
                }
                if !has_any_top_level_symbol.contains(&name) {
                    diagnostics.push(PackageDiagnostic {
                        path: file.path.to_path_buf(),
                        diagnostic: Diagnostic::new(
                            format!("exported symbol '{name}' is not declared in this package"),
                            member.span.clone(),
                        ),
                    });
                    continue;
                }
                if !package_visible_symbols.contains(&name) {
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
