use std::collections::{BTreeMap, BTreeSet};

use compiler__diagnostics::Diagnostic;
use compiler__symbols::{PackageDiagnostic, PackageFile, top_level_symbol};
use compiler__visibility::ResolvedImportBinding;

pub fn check_bindings(
    files: &[PackageFile<'_>],
    bindings_by_file: &BTreeMap<std::path::PathBuf, Vec<ResolvedImportBinding>>,
    diagnostics: &mut Vec<PackageDiagnostic>,
) {
    for file in files {
        let mut imported_names = BTreeSet::new();
        if let Some(bindings) = bindings_by_file.get(file.path) {
            for binding in bindings {
                if !imported_names.insert(binding.local_name.clone()) {
                    diagnostics.push(PackageDiagnostic {
                        path: file.path.to_path_buf(),
                        diagnostic: Diagnostic::new(
                            format!(
                                "duplicate imported name '{}'; use an alias",
                                binding.local_name
                            ),
                            binding.span.clone(),
                        ),
                    });
                }
            }
        }

        for declaration in file.parsed.top_level_declarations() {
            let Some(symbol) = top_level_symbol(declaration) else {
                continue;
            };
            if imported_names.contains(&symbol.name) {
                diagnostics.push(PackageDiagnostic {
                    path: file.path.to_path_buf(),
                    diagnostic: Diagnostic::new(
                        format!(
                            "top-level declaration '{}' conflicts with imported name",
                            symbol.name
                        ),
                        symbol.span,
                    ),
                });
            }
        }
    }
}
