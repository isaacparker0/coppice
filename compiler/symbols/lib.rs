use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use compiler__diagnostics::Diagnostic;
use compiler__source::{FileRole, Span};
use compiler__syntax::{Declaration, ParsedFile, Visibility};

pub struct PackageFile<'a> {
    pub package_path: &'a str,
    pub path: &'a Path,
    pub parsed: &'a ParsedFile,
}

pub struct PackageDiagnostic {
    pub path: PathBuf,
    pub diagnostic: Diagnostic,
}

pub struct TopLevelSymbol {
    pub name: String,
    pub visibility: Visibility,
    pub span: Span,
}

#[derive(Default)]
pub struct PackageSymbols {
    pub declared: BTreeSet<String>,
    pub package_visible: BTreeSet<String>,
}

pub type SymbolsByPackage = BTreeMap<String, PackageSymbols>;

pub fn collect_symbols(
    files: &[PackageFile<'_>],
    diagnostics: &mut Vec<PackageDiagnostic>,
) -> SymbolsByPackage {
    let mut ordered_files: Vec<&PackageFile<'_>> = files.iter().collect();
    ordered_files.sort_by_key(|file| {
        (
            file.package_path.to_string(),
            file.path.to_string_lossy().replace('\\', "/"),
        )
    });

    let mut symbols_by_package: SymbolsByPackage = BTreeMap::new();
    for file in &ordered_files {
        if file.parsed.role != FileRole::Library {
            continue;
        }

        let package_symbols = symbols_by_package
            .entry(file.package_path.to_string())
            .or_default();
        for declaration in file.parsed.top_level_declarations() {
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

    symbols_by_package
}

#[must_use]
pub fn top_level_symbol(declaration: &Declaration) -> Option<TopLevelSymbol> {
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
