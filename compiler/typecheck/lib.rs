use compiler__diagnostics::Diagnostic;
use compiler__packages::PackageId;
use compiler__semantic_types::{FileTypecheckSummary, ImportedBinding};
use compiler__syntax::ParsedFile;

pub use compiler__semantic_types::{
    FileTypecheckSummary as SemanticFileTypecheckSummary, ImportedMethodSignature, ImportedSymbol,
    ImportedTypeDeclaration, ImportedTypeShape, NominalTypeId, NominalTypeRef, Type,
    TypedFunctionSignature, TypedSymbol, type_from_builtin_name,
};

pub fn check_package_unit(
    package_id: PackageId,
    package_unit: &ParsedFile,
    imported_bindings: &[ImportedBinding],
    diagnostics: &mut Vec<Diagnostic>,
) {
    compiler__type_analysis::check_package_unit(
        package_id,
        package_unit,
        imported_bindings,
        diagnostics,
    );
}

pub fn analyze_package_unit(
    package_id: PackageId,
    package_unit: &ParsedFile,
    imported_bindings: &[ImportedBinding],
    diagnostics: &mut Vec<Diagnostic>,
) -> FileTypecheckSummary {
    compiler__type_analysis::analyze_package_unit(
        package_id,
        package_unit,
        imported_bindings,
        diagnostics,
    )
}
