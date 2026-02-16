use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use compiler__packages::PackageId;
use compiler__semantic_types::{
    ImportedBinding, ImportedMethodSignature, ImportedSymbol, ImportedTypeDeclaration,
    ImportedTypeShape, NominalTypeId, NominalTypeRef, Type, TypedFunctionSignature,
    type_from_builtin_name,
};
use compiler__source::{FileRole, Span, compare_paths};
use compiler__syntax::{
    Declaration, FunctionDeclaration, ParsedFile, TypeDeclaration, TypeDeclarationKind, TypeName,
    Visibility,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct PublicSymbolId(usize);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PublicSymbolLookupKey {
    package_id: PackageId,
    symbol_name: String,
}

#[derive(Clone)]
enum PublicSymbolDefinition {
    Type(TypeDeclaration),
    Function(FunctionDeclaration),
    Constant,
}

#[derive(Clone)]
enum TypedPublicSymbol {
    Type(TypeDeclaration),
    Function(FunctionDeclaration),
    Constant(Type),
}

pub struct PackageUnit<'a> {
    pub package_id: PackageId,
    pub path: &'a Path,
    pub parsed: &'a ParsedFile,
}

#[derive(Clone)]
pub struct ResolvedImportBindingSummary {
    pub imported_name: String,
    pub local_name: String,
    pub span: Span,
}

#[derive(Clone)]
pub struct ResolvedImportSummary {
    pub source_path: PathBuf,
    pub target_package_id: PackageId,
    pub bindings: Vec<ResolvedImportBindingSummary>,
}

pub struct TypedPublicSymbolTable {
    symbol_id_by_lookup_key: BTreeMap<PublicSymbolLookupKey, PublicSymbolId>,
    typed_symbol_by_id: BTreeMap<PublicSymbolId, TypedPublicSymbol>,
}

impl TypedPublicSymbolTable {
    #[must_use]
    pub fn imported_bindings_by_file(
        &self,
        resolved_imports: &[ResolvedImportSummary],
    ) -> BTreeMap<PathBuf, Vec<ImportedBinding>> {
        self.imported_bindings_by_file_with_constant_types(resolved_imports, &BTreeMap::new())
    }

    #[must_use]
    pub fn imported_bindings_by_file_with_constant_types(
        &self,
        resolved_imports: &[ResolvedImportSummary],
        constant_type_by_symbol: &BTreeMap<(PackageId, String), Type>,
    ) -> BTreeMap<PathBuf, Vec<ImportedBinding>> {
        build_imported_bindings_by_file(
            resolved_imports,
            &self.symbol_id_by_lookup_key,
            &self.typed_symbol_by_id,
            constant_type_by_symbol,
        )
    }
}

#[must_use]
pub fn build_typed_public_symbol_table(
    package_units: &[PackageUnit<'_>],
    _resolved_imports: &[ResolvedImportSummary],
) -> TypedPublicSymbolTable {
    let (symbol_id_by_lookup_key, public_symbol_definition_by_id) =
        collect_public_symbol_index(package_units);

    let typed_symbol_by_id = resolve_public_symbol_types(&public_symbol_definition_by_id);

    TypedPublicSymbolTable {
        symbol_id_by_lookup_key,
        typed_symbol_by_id,
    }
}

fn collect_public_symbol_index(
    package_units: &[PackageUnit<'_>],
) -> (
    BTreeMap<PublicSymbolLookupKey, PublicSymbolId>,
    BTreeMap<PublicSymbolId, PublicSymbolDefinition>,
) {
    let mut symbol_id_by_lookup_key = BTreeMap::new();
    let mut public_symbol_definition_by_id = BTreeMap::new();

    let mut ordered_units: Vec<&PackageUnit<'_>> = package_units.iter().collect();
    ordered_units.sort_by(|left, right| {
        left.package_id
            .cmp(&right.package_id)
            .then(compare_paths(left.path, right.path))
    });

    for unit in ordered_units {
        if unit.parsed.role != FileRole::Library {
            continue;
        }

        for declaration in &unit.parsed.declarations {
            let (name, is_public) = match declaration {
                Declaration::Type(type_declaration) => (
                    &type_declaration.name,
                    type_declaration.visibility == Visibility::Public,
                ),
                Declaration::Function(function_declaration) => (
                    &function_declaration.name,
                    function_declaration.visibility == Visibility::Public,
                ),
                Declaration::Constant(constant_declaration) => (
                    &constant_declaration.name,
                    constant_declaration.visibility == Visibility::Public,
                ),
                Declaration::Import(_) | Declaration::Exports(_) => continue,
            };
            if !is_public {
                continue;
            }

            let public_symbol_definition = match declaration {
                Declaration::Type(type_declaration) => {
                    PublicSymbolDefinition::Type(type_declaration.clone())
                }
                Declaration::Function(function_declaration) => {
                    PublicSymbolDefinition::Function(function_declaration.clone())
                }
                Declaration::Constant(_) => PublicSymbolDefinition::Constant,
                Declaration::Import(_) | Declaration::Exports(_) => continue,
            };

            let lookup_key = PublicSymbolLookupKey {
                package_id: unit.package_id,
                symbol_name: name.clone(),
            };
            if symbol_id_by_lookup_key.contains_key(&lookup_key) {
                continue;
            }

            let symbol_id = PublicSymbolId(symbol_id_by_lookup_key.len());
            symbol_id_by_lookup_key.insert(lookup_key, symbol_id);
            public_symbol_definition_by_id.insert(symbol_id, public_symbol_definition);
        }
    }

    (symbol_id_by_lookup_key, public_symbol_definition_by_id)
}

fn resolve_public_symbol_types(
    public_symbol_definition_by_id: &BTreeMap<PublicSymbolId, PublicSymbolDefinition>,
) -> BTreeMap<PublicSymbolId, TypedPublicSymbol> {
    let mut typed_symbol_by_id = BTreeMap::new();
    for (symbol_id, definition) in public_symbol_definition_by_id {
        let typed_symbol = match definition {
            PublicSymbolDefinition::Type(type_declaration) => {
                TypedPublicSymbol::Type(type_declaration.clone())
            }
            PublicSymbolDefinition::Function(function_declaration) => {
                TypedPublicSymbol::Function(function_declaration.clone())
            }
            PublicSymbolDefinition::Constant => TypedPublicSymbol::Constant(Type::Unknown),
        };
        typed_symbol_by_id.insert(*symbol_id, typed_symbol);
    }
    typed_symbol_by_id
}

fn build_imported_bindings_by_file(
    resolved_imports: &[ResolvedImportSummary],
    symbol_id_by_lookup_key: &BTreeMap<PublicSymbolLookupKey, PublicSymbolId>,
    typed_symbol_by_id: &BTreeMap<PublicSymbolId, TypedPublicSymbol>,
    constant_type_by_symbol: &BTreeMap<(PackageId, String), Type>,
) -> BTreeMap<PathBuf, Vec<ImportedBinding>> {
    let mut imported_by_file: BTreeMap<PathBuf, Vec<ImportedBinding>> = BTreeMap::new();
    let nominal_type_id_by_lookup_key =
        nominal_type_id_by_lookup_key(symbol_id_by_lookup_key, typed_symbol_by_id);

    for resolved_import in resolved_imports {
        for binding in &resolved_import.bindings {
            let lookup_key = PublicSymbolLookupKey {
                package_id: resolved_import.target_package_id,
                symbol_name: binding.imported_name.clone(),
            };
            let Some(symbol_id) = symbol_id_by_lookup_key.get(&lookup_key) else {
                continue;
            };
            let Some(typed_symbol) = typed_symbol_by_id.get(symbol_id) else {
                continue;
            };

            let symbol = match typed_symbol {
                TypedPublicSymbol::Type(type_declaration) => {
                    ImportedSymbol::Type(imported_type_declaration(
                        type_declaration,
                        resolved_import.target_package_id,
                        &nominal_type_id_by_lookup_key,
                    ))
                }
                TypedPublicSymbol::Function(function_declaration) => {
                    ImportedSymbol::Function(imported_function_signature(
                        function_declaration,
                        resolved_import.target_package_id,
                        &nominal_type_id_by_lookup_key,
                    ))
                }
                TypedPublicSymbol::Constant(fallback_type) => {
                    let value_type = constant_type_by_symbol
                        .get(&(
                            resolved_import.target_package_id,
                            binding.imported_name.clone(),
                        ))
                        .cloned()
                        .unwrap_or_else(|| fallback_type.clone());
                    ImportedSymbol::Constant(value_type)
                }
            };

            imported_by_file
                .entry(resolved_import.source_path.clone())
                .or_default()
                .push(ImportedBinding {
                    local_name: binding.local_name.clone(),
                    span: binding.span.clone(),
                    symbol,
                });
        }
    }

    imported_by_file
}

fn nominal_type_id_by_lookup_key(
    symbol_id_by_lookup_key: &BTreeMap<PublicSymbolLookupKey, PublicSymbolId>,
    typed_symbol_by_id: &BTreeMap<PublicSymbolId, TypedPublicSymbol>,
) -> BTreeMap<PublicSymbolLookupKey, NominalTypeId> {
    let mut nominal_type_id_by_lookup_key = BTreeMap::new();
    for (lookup_key, symbol_id) in symbol_id_by_lookup_key {
        if !matches!(
            typed_symbol_by_id.get(symbol_id),
            Some(TypedPublicSymbol::Type(_))
        ) {
            continue;
        }
        nominal_type_id_by_lookup_key.insert(
            lookup_key.clone(),
            NominalTypeId {
                package_id: lookup_key.package_id,
                symbol_name: lookup_key.symbol_name.clone(),
            },
        );
    }
    nominal_type_id_by_lookup_key
}

fn imported_type_declaration(
    type_declaration: &TypeDeclaration,
    target_package_id: PackageId,
    nominal_type_id_by_lookup_key: &BTreeMap<PublicSymbolLookupKey, NominalTypeId>,
) -> ImportedTypeDeclaration {
    let declared_nominal_type_id = NominalTypeId {
        package_id: target_package_id,
        symbol_name: type_declaration.name.clone(),
    };
    let kind = match &type_declaration.kind {
        TypeDeclarationKind::Struct { fields, methods } => {
            let typed_fields = fields
                .iter()
                .map(|field| {
                    (
                        field.name.clone(),
                        resolve_type_name_to_semantic_type(
                            &field.type_name,
                            target_package_id,
                            nominal_type_id_by_lookup_key,
                        ),
                    )
                })
                .collect();
            let typed_methods = methods
                .iter()
                .map(|method| ImportedMethodSignature {
                    name: method.name.clone(),
                    self_mutable: method.self_mutable,
                    parameter_types: method
                        .parameters
                        .iter()
                        .map(|parameter| {
                            resolve_type_name_to_semantic_type(
                                &parameter.type_name,
                                target_package_id,
                                nominal_type_id_by_lookup_key,
                            )
                        })
                        .collect(),
                    return_type: resolve_type_name_to_semantic_type(
                        &method.return_type,
                        target_package_id,
                        nominal_type_id_by_lookup_key,
                    ),
                })
                .collect();
            ImportedTypeShape::Struct {
                fields: typed_fields,
                methods: typed_methods,
            }
        }
        TypeDeclarationKind::Union { variants } => ImportedTypeShape::Union {
            variants: variants
                .iter()
                .map(|variant| {
                    resolve_type_name_to_semantic_type(
                        variant,
                        target_package_id,
                        nominal_type_id_by_lookup_key,
                    )
                })
                .collect(),
        },
    };

    ImportedTypeDeclaration {
        nominal_type_id: declared_nominal_type_id,
        kind,
    }
}

fn imported_function_signature(
    function_declaration: &FunctionDeclaration,
    target_package_id: PackageId,
    nominal_type_id_by_lookup_key: &BTreeMap<PublicSymbolLookupKey, NominalTypeId>,
) -> TypedFunctionSignature {
    let parameter_types = function_declaration
        .parameters
        .iter()
        .map(|parameter| {
            resolve_type_name_to_semantic_type(
                &parameter.type_name,
                target_package_id,
                nominal_type_id_by_lookup_key,
            )
        })
        .collect();

    TypedFunctionSignature {
        parameter_types,
        return_type: resolve_type_name_to_semantic_type(
            &function_declaration.return_type,
            target_package_id,
            nominal_type_id_by_lookup_key,
        ),
    }
}

fn resolve_type_name_to_semantic_type(
    type_name: &TypeName,
    target_package_id: PackageId,
    nominal_type_id_by_lookup_key: &BTreeMap<PublicSymbolLookupKey, NominalTypeId>,
) -> Type {
    let mut resolved = Vec::new();
    for atom in &type_name.names {
        if let Some(value_type) = type_from_builtin_name(&atom.name) {
            resolved.push(value_type);
            continue;
        }
        let lookup_key = PublicSymbolLookupKey {
            package_id: target_package_id,
            symbol_name: atom.name.clone(),
        };
        let Some(nominal_type_id) = nominal_type_id_by_lookup_key.get(&lookup_key) else {
            return Type::Unknown;
        };
        resolved.push(Type::Named(NominalTypeRef {
            id: nominal_type_id.clone(),
            display_name: atom.name.clone(),
        }));
    }
    if resolved.is_empty() {
        return Type::Unknown;
    }
    if resolved.len() == 1 {
        return resolved.remove(0);
    }

    let mut flattened = Vec::new();
    let mut seen = BTreeSet::new();
    for value_type in resolved {
        if let Type::Union(inner) = value_type {
            for inner_type in inner {
                let key = inner_type.display();
                if seen.insert(key) {
                    flattened.push(inner_type);
                }
            }
        } else {
            let key = value_type.display();
            if seen.insert(key) {
                flattened.push(value_type);
            }
        }
    }
    if flattened.len() == 1 {
        return flattened.remove(0);
    }
    Type::Union(flattened)
}
