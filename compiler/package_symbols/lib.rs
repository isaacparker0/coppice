use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use compiler__packages::PackageId;
use compiler__semantic_program::{
    Declaration, FunctionDeclaration, SemanticFile, TypeDeclaration, TypeDeclarationKind, TypeName,
    TypeParameter, Visibility,
};
use compiler__semantic_types::{
    GenericTypeParameter, ImportedBinding, ImportedMethodSignature, ImportedSymbol,
    ImportedTypeDeclaration, ImportedTypeShape, NominalTypeId, NominalTypeRef, Type,
    TypedFunctionSignature, type_from_builtin_name,
};
use compiler__source::{FileRole, Span, compare_paths};

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
    Constant(TypeName),
}

#[derive(Clone)]
enum TypedPublicSymbol {
    Type(TypeDeclaration),
    Function(FunctionDeclaration),
    Constant(Type),
}

pub struct PackageSymbolFileInput<'a> {
    pub package_id: PackageId,
    pub path: &'a Path,
    pub semantic_file: &'a SemanticFile,
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
        build_imported_bindings_by_file(
            resolved_imports,
            &self.symbol_id_by_lookup_key,
            &self.typed_symbol_by_id,
        )
    }
}

#[must_use]
pub fn build_typed_public_symbol_table(
    package_symbol_file_inputs: &[PackageSymbolFileInput<'_>],
    _resolved_imports: &[ResolvedImportSummary],
) -> TypedPublicSymbolTable {
    let (symbol_id_by_lookup_key, public_symbol_definition_by_id) =
        collect_public_symbol_index(package_symbol_file_inputs);

    let typed_symbol_by_id =
        resolve_public_symbol_types(&symbol_id_by_lookup_key, &public_symbol_definition_by_id);

    TypedPublicSymbolTable {
        symbol_id_by_lookup_key,
        typed_symbol_by_id,
    }
}

fn collect_public_symbol_index(
    package_symbol_file_inputs: &[PackageSymbolFileInput<'_>],
) -> (
    BTreeMap<PublicSymbolLookupKey, PublicSymbolId>,
    BTreeMap<PublicSymbolId, PublicSymbolDefinition>,
) {
    let mut symbol_id_by_lookup_key = BTreeMap::new();
    let mut public_symbol_definition_by_id = BTreeMap::new();

    let mut ordered_file_inputs: Vec<&PackageSymbolFileInput<'_>> =
        package_symbol_file_inputs.iter().collect();
    ordered_file_inputs.sort_by(|left, right| {
        left.package_id
            .cmp(&right.package_id)
            .then(compare_paths(left.path, right.path))
    });

    for file_input in ordered_file_inputs {
        if file_input.semantic_file.role != FileRole::Library {
            continue;
        }

        for declaration in &file_input.semantic_file.declarations {
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
                Declaration::Constant(constant_declaration) => {
                    PublicSymbolDefinition::Constant(constant_declaration.type_name.clone())
                }
            };

            let lookup_key = PublicSymbolLookupKey {
                package_id: file_input.package_id,
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
    symbol_id_by_lookup_key: &BTreeMap<PublicSymbolLookupKey, PublicSymbolId>,
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
            PublicSymbolDefinition::Constant(_) => TypedPublicSymbol::Constant(Type::Unknown),
        };
        typed_symbol_by_id.insert(*symbol_id, typed_symbol);
    }

    let nominal_type_id_by_lookup_key =
        nominal_type_id_by_lookup_key(symbol_id_by_lookup_key, &typed_symbol_by_id);
    let lookup_key_by_symbol_id: BTreeMap<PublicSymbolId, PublicSymbolLookupKey> =
        symbol_id_by_lookup_key
            .iter()
            .map(|(lookup_key, symbol_id)| (*symbol_id, lookup_key.clone()))
            .collect();

    for (symbol_id, definition) in public_symbol_definition_by_id {
        let PublicSymbolDefinition::Constant(type_name) = definition else {
            continue;
        };
        let Some(lookup_key) = lookup_key_by_symbol_id.get(symbol_id) else {
            continue;
        };
        let value_type = resolve_type_name_to_semantic_type(
            type_name,
            lookup_key.package_id,
            &nominal_type_id_by_lookup_key,
            &[],
        );
        typed_symbol_by_id.insert(*symbol_id, TypedPublicSymbol::Constant(value_type));
    }

    typed_symbol_by_id
}

fn build_imported_bindings_by_file(
    resolved_imports: &[ResolvedImportSummary],
    symbol_id_by_lookup_key: &BTreeMap<PublicSymbolLookupKey, PublicSymbolId>,
    typed_symbol_by_id: &BTreeMap<PublicSymbolId, TypedPublicSymbol>,
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
                TypedPublicSymbol::Constant(value_type) => {
                    ImportedSymbol::Constant(value_type.clone())
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
                            &type_declaration
                                .type_parameters
                                .iter()
                                .map(|parameter| parameter.name.as_str())
                                .collect::<Vec<_>>(),
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
                                &type_declaration
                                    .type_parameters
                                    .iter()
                                    .map(|parameter| parameter.name.as_str())
                                    .collect::<Vec<_>>(),
                            )
                        })
                        .collect(),
                    return_type: resolve_type_name_to_semantic_type(
                        &method.return_type,
                        target_package_id,
                        nominal_type_id_by_lookup_key,
                        &type_declaration
                            .type_parameters
                            .iter()
                            .map(|parameter| parameter.name.as_str())
                            .collect::<Vec<_>>(),
                    ),
                })
                .collect();
            ImportedTypeShape::Struct {
                fields: typed_fields,
                methods: typed_methods,
            }
        }
        TypeDeclarationKind::Enum { variants } => ImportedTypeShape::Union {
            variants: variants
                .iter()
                .map(|variant| {
                    Type::Named(NominalTypeRef {
                        id: NominalTypeId {
                            package_id: target_package_id,
                            symbol_name: format!(
                                "{enum_name}.{variant_name}",
                                enum_name = type_declaration.name,
                                variant_name = variant.name
                            ),
                        },
                        display_name: format!(
                            "{enum_name}.{variant_name}",
                            enum_name = type_declaration.name,
                            variant_name = variant.name
                        ),
                    })
                })
                .collect(),
        },
        TypeDeclarationKind::Union { variants } => ImportedTypeShape::Union {
            variants: variants
                .iter()
                .map(|variant| {
                    resolve_type_name_to_semantic_type(
                        variant,
                        target_package_id,
                        nominal_type_id_by_lookup_key,
                        &type_declaration
                            .type_parameters
                            .iter()
                            .map(|parameter| parameter.name.as_str())
                            .collect::<Vec<_>>(),
                    )
                })
                .collect(),
        },
    };

    ImportedTypeDeclaration {
        nominal_type_id: declared_nominal_type_id,
        type_parameters: imported_type_parameters(
            &type_declaration.type_parameters,
            target_package_id,
            nominal_type_id_by_lookup_key,
            &type_declaration
                .type_parameters
                .iter()
                .map(|parameter| parameter.name.as_str())
                .collect::<Vec<_>>(),
        ),
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
                &function_declaration
                    .type_parameters
                    .iter()
                    .map(|parameter| parameter.name.as_str())
                    .collect::<Vec<_>>(),
            )
        })
        .collect();

    TypedFunctionSignature {
        type_parameters: imported_type_parameters(
            &function_declaration.type_parameters,
            target_package_id,
            nominal_type_id_by_lookup_key,
            &function_declaration
                .type_parameters
                .iter()
                .map(|parameter| parameter.name.as_str())
                .collect::<Vec<_>>(),
        ),
        parameter_types,
        return_type: resolve_type_name_to_semantic_type(
            &function_declaration.return_type,
            target_package_id,
            nominal_type_id_by_lookup_key,
            &function_declaration
                .type_parameters
                .iter()
                .map(|parameter| parameter.name.as_str())
                .collect::<Vec<_>>(),
        ),
    }
}

fn imported_type_parameters(
    type_parameters: &[TypeParameter],
    target_package_id: PackageId,
    nominal_type_id_by_lookup_key: &BTreeMap<PublicSymbolLookupKey, NominalTypeId>,
    in_scope_type_parameter_names: &[&str],
) -> Vec<GenericTypeParameter> {
    type_parameters
        .iter()
        .map(|parameter| GenericTypeParameter {
            name: parameter.name.clone(),
            constraint: parameter.constraint.as_ref().map(|constraint| {
                resolve_type_name_to_semantic_type(
                    constraint,
                    target_package_id,
                    nominal_type_id_by_lookup_key,
                    in_scope_type_parameter_names,
                )
            }),
        })
        .collect()
}

fn resolve_type_name_to_semantic_type(
    type_name: &TypeName,
    target_package_id: PackageId,
    nominal_type_id_by_lookup_key: &BTreeMap<PublicSymbolLookupKey, NominalTypeId>,
    type_parameters: &[&str],
) -> Type {
    let mut resolved = Vec::new();
    for atom in &type_name.names {
        if type_parameters.contains(&atom.name.as_str()) {
            if !atom.type_arguments.is_empty() {
                return Type::Unknown;
            }
            resolved.push(Type::TypeParameter(atom.name.clone()));
            continue;
        }
        if let Some(value_type) = type_from_builtin_name(&atom.name) {
            if !atom.type_arguments.is_empty() {
                return Type::Unknown;
            }
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
        if atom.type_arguments.is_empty() {
            resolved.push(Type::Named(NominalTypeRef {
                id: nominal_type_id.clone(),
                display_name: atom.name.clone(),
            }));
            continue;
        }
        let argument_types = atom
            .type_arguments
            .iter()
            .map(|argument| {
                resolve_type_name_to_semantic_type(
                    argument,
                    target_package_id,
                    nominal_type_id_by_lookup_key,
                    type_parameters,
                )
            })
            .collect::<Vec<_>>();
        resolved.push(Type::Applied {
            base: NominalTypeRef {
                id: nominal_type_id.clone(),
                display_name: atom.name.clone(),
            },
            arguments: argument_types,
        });
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
