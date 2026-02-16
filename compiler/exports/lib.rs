use std::collections::{BTreeMap, BTreeSet};

use compiler__diagnostics::PhaseDiagnostic;
use compiler__source::FileRole;
use compiler__symbols::{PackageDiagnostic, PackageFile, SymbolsByPackage};
use compiler__syntax::Declaration;

pub type ExportsByPackage = BTreeMap<String, BTreeSet<String>>;

pub fn build_exports(
    files: &[PackageFile<'_>],
    symbols_by_package: &SymbolsByPackage,
    diagnostics: &mut Vec<PackageDiagnostic>,
) -> ExportsByPackage {
    let mut ordered_files: Vec<&PackageFile<'_>> = files.iter().collect();
    ordered_files.sort_by_key(|file| {
        (
            file.package_path.to_string(),
            file.path.to_string_lossy().replace('\\', "/"),
        )
    });

    let mut exports_by_package: ExportsByPackage = BTreeMap::new();

    for file in &ordered_files {
        if file.parsed.role != FileRole::PackageManifest {
            continue;
        }

        let exported_symbols = exports_by_package
            .entry(file.package_path.to_string())
            .or_default();

        for declaration in file.parsed.top_level_declarations() {
            let Declaration::Exports(exports) = declaration else {
                continue;
            };

            let package_symbols = symbols_by_package.get(file.package_path);
            for member in &exports.members {
                let name = member.name.clone();
                if !exported_symbols.insert(name.clone()) {
                    diagnostics.push(PackageDiagnostic {
                        path: file.path.to_path_buf(),
                        diagnostic: PhaseDiagnostic::new(
                            format!("duplicate exported symbol '{name}'"),
                            member.span.clone(),
                        ),
                    });
                    continue;
                }
                let Some(package_symbols) = package_symbols else {
                    diagnostics.push(PackageDiagnostic {
                        path: file.path.to_path_buf(),
                        diagnostic: PhaseDiagnostic::new(
                            format!("exported symbol '{name}' is not declared in this package"),
                            member.span.clone(),
                        ),
                    });
                    continue;
                };
                if !package_symbols.declared.contains(&name) {
                    diagnostics.push(PackageDiagnostic {
                        path: file.path.to_path_buf(),
                        diagnostic: PhaseDiagnostic::new(
                            format!("exported symbol '{name}' is not declared in this package"),
                            member.span.clone(),
                        ),
                    });
                    continue;
                }
                if !package_symbols.package_visible.contains(&name) {
                    diagnostics.push(PackageDiagnostic {
                        path: file.path.to_path_buf(),
                        diagnostic: PhaseDiagnostic::new(
                            format!("exported symbol '{name}' must be declared public"),
                            member.span.clone(),
                        ),
                    });
                }
            }
        }
    }

    exports_by_package
}
