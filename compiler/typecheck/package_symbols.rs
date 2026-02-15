use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use compiler__packages::PackageId;
use compiler__source::{FileRole, Span, compare_paths};
use compiler__syntax::{
    ConstantDeclaration, Declaration, Expression, FunctionDeclaration, MatchPattern,
    ParameterDeclaration, ParsedFile, TypeDeclaration, TypeName, TypeNameAtom, Visibility,
};

use crate::{ImportedBinding, ImportedSymbol, Type, TypedSymbol, analyze_package_unit};

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

#[derive(Clone)]
struct PublicConstantDefinition {
    path: PathBuf,
    symbol_name: String,
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
        build_imported_bindings_by_file(
            resolved_imports,
            &self.symbol_id_by_lookup_key,
            &self.typed_symbol_by_id,
        )
    }
}

#[must_use]
pub fn build_typed_public_symbol_table(
    package_units: &[PackageUnit<'_>],
    resolved_imports: &[ResolvedImportSummary],
) -> TypedPublicSymbolTable {
    let (
        symbol_id_by_lookup_key,
        public_symbol_definition_by_id,
        public_constant_definition_by_symbol_id,
    ) = collect_public_symbol_index(package_units);

    let typed_symbol_by_id = resolve_public_symbol_types(
        package_units,
        resolved_imports,
        &symbol_id_by_lookup_key,
        &public_symbol_definition_by_id,
        &public_constant_definition_by_symbol_id,
    );

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
    BTreeMap<PublicSymbolId, PublicConstantDefinition>,
) {
    let mut symbol_id_by_lookup_key = BTreeMap::new();
    let mut public_symbol_definition_by_id = BTreeMap::new();
    let mut public_constant_definition_by_symbol_id = BTreeMap::new();

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

            if matches!(declaration, Declaration::Constant(_)) {
                public_constant_definition_by_symbol_id.insert(
                    symbol_id,
                    PublicConstantDefinition {
                        path: unit.path.to_path_buf(),
                        symbol_name: name.clone(),
                    },
                );
            }
        }
    }

    (
        symbol_id_by_lookup_key,
        public_symbol_definition_by_id,
        public_constant_definition_by_symbol_id,
    )
}

fn resolve_public_symbol_types(
    package_units: &[PackageUnit<'_>],
    resolved_imports: &[ResolvedImportSummary],
    symbol_id_by_lookup_key: &BTreeMap<PublicSymbolLookupKey, PublicSymbolId>,
    public_symbol_definition_by_id: &BTreeMap<PublicSymbolId, PublicSymbolDefinition>,
    public_constant_definition_by_symbol_id: &BTreeMap<PublicSymbolId, PublicConstantDefinition>,
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

    if public_constant_definition_by_symbol_id.is_empty() {
        return typed_symbol_by_id;
    }

    let package_unit_by_path: BTreeMap<PathBuf, &PackageUnit<'_>> = package_units
        .iter()
        .map(|unit| (unit.path.to_path_buf(), unit))
        .collect();

    let imported_constant_symbol_id_by_file = imported_constant_symbol_id_by_file(
        resolved_imports,
        symbol_id_by_lookup_key,
        public_symbol_definition_by_id,
    );
    let local_public_constant_symbol_id_by_file =
        local_public_constant_symbol_id_by_file(public_constant_definition_by_symbol_id);

    let dependency_symbol_ids_by_constant_symbol_id = dependency_symbol_ids_by_constant_symbol_id(
        public_constant_definition_by_symbol_id,
        &package_unit_by_path,
        &local_public_constant_symbol_id_by_file,
        &imported_constant_symbol_id_by_file,
    );

    let constant_symbol_ids: Vec<PublicSymbolId> = public_constant_definition_by_symbol_id
        .keys()
        .copied()
        .collect();
    let components_in_dependency_order = components_in_dependency_order(
        &constant_symbol_ids,
        &dependency_symbol_ids_by_constant_symbol_id,
    );

    for component_constant_symbol_ids in components_in_dependency_order {
        resolve_constant_component(
            &component_constant_symbol_ids,
            public_constant_definition_by_symbol_id,
            &dependency_symbol_ids_by_constant_symbol_id,
            &package_unit_by_path,
            resolved_imports,
            symbol_id_by_lookup_key,
            &mut typed_symbol_by_id,
        );
    }

    typed_symbol_by_id
}

fn imported_constant_symbol_id_by_file(
    resolved_imports: &[ResolvedImportSummary],
    symbol_id_by_lookup_key: &BTreeMap<PublicSymbolLookupKey, PublicSymbolId>,
    public_symbol_definition_by_id: &BTreeMap<PublicSymbolId, PublicSymbolDefinition>,
) -> BTreeMap<PathBuf, HashMap<String, PublicSymbolId>> {
    let mut imported_constant_symbol_id_by_file = BTreeMap::new();
    for resolved_import in resolved_imports {
        for binding in &resolved_import.bindings {
            let lookup_key = PublicSymbolLookupKey {
                package_id: resolved_import.target_package_id,
                symbol_name: binding.imported_name.clone(),
            };
            let Some(symbol_id) = symbol_id_by_lookup_key.get(&lookup_key) else {
                continue;
            };
            if !matches!(
                public_symbol_definition_by_id.get(symbol_id),
                Some(PublicSymbolDefinition::Constant)
            ) {
                continue;
            }
            imported_constant_symbol_id_by_file
                .entry(resolved_import.source_path.clone())
                .or_insert_with(HashMap::new)
                .insert(binding.local_name.clone(), *symbol_id);
        }
    }
    imported_constant_symbol_id_by_file
}

fn local_public_constant_symbol_id_by_file(
    public_constant_definition_by_symbol_id: &BTreeMap<PublicSymbolId, PublicConstantDefinition>,
) -> BTreeMap<PathBuf, HashMap<String, PublicSymbolId>> {
    let mut local_public_constant_symbol_id_by_file = BTreeMap::new();
    for (symbol_id, definition) in public_constant_definition_by_symbol_id {
        local_public_constant_symbol_id_by_file
            .entry(definition.path.clone())
            .or_insert_with(HashMap::new)
            .insert(definition.symbol_name.clone(), *symbol_id);
    }
    local_public_constant_symbol_id_by_file
}

fn dependency_symbol_ids_by_constant_symbol_id(
    public_constant_definition_by_symbol_id: &BTreeMap<PublicSymbolId, PublicConstantDefinition>,
    package_unit_by_path: &BTreeMap<PathBuf, &PackageUnit<'_>>,
    local_public_constant_symbol_id_by_file: &BTreeMap<PathBuf, HashMap<String, PublicSymbolId>>,
    imported_constant_symbol_id_by_file: &BTreeMap<PathBuf, HashMap<String, PublicSymbolId>>,
) -> BTreeMap<PublicSymbolId, Vec<PublicSymbolId>> {
    let mut dependency_symbol_ids_by_constant_symbol_id = BTreeMap::new();
    for (symbol_id, definition) in public_constant_definition_by_symbol_id {
        let Some(package_unit) = package_unit_by_path.get(&definition.path) else {
            dependency_symbol_ids_by_constant_symbol_id.insert(*symbol_id, Vec::new());
            continue;
        };
        let Some(constant_declaration) =
            find_public_constant_declaration(package_unit.parsed, &definition.symbol_name)
        else {
            dependency_symbol_ids_by_constant_symbol_id.insert(*symbol_id, Vec::new());
            continue;
        };

        let local_constant_symbol_id_by_name = local_public_constant_symbol_id_by_file
            .get(&definition.path)
            .cloned()
            .unwrap_or_default();
        let imported_constant_symbol_id_by_name = imported_constant_symbol_id_by_file
            .get(&definition.path)
            .cloned()
            .unwrap_or_default();

        let mut dependency_symbol_ids = collect_constant_dependency_symbol_ids(
            &constant_declaration.expression,
            &local_constant_symbol_id_by_name,
            &imported_constant_symbol_id_by_name,
        );
        dependency_symbol_ids.remove(symbol_id);

        dependency_symbol_ids_by_constant_symbol_id
            .insert(*symbol_id, dependency_symbol_ids.into_iter().collect());
    }
    dependency_symbol_ids_by_constant_symbol_id
}

fn find_public_constant_declaration<'a>(
    parsed: &'a ParsedFile,
    constant_name: &str,
) -> Option<&'a ConstantDeclaration> {
    for declaration in &parsed.declarations {
        let Declaration::Constant(constant_declaration) = declaration else {
            continue;
        };
        if constant_declaration.visibility == Visibility::Public
            && constant_declaration.name == constant_name
        {
            return Some(constant_declaration);
        }
    }
    None
}

fn collect_constant_dependency_symbol_ids(
    expression: &Expression,
    local_constant_symbol_id_by_name: &HashMap<String, PublicSymbolId>,
    imported_constant_symbol_id_by_name: &HashMap<String, PublicSymbolId>,
) -> BTreeSet<PublicSymbolId> {
    let mut dependency_symbol_ids = BTreeSet::new();
    let bound_names = BTreeSet::new();
    collect_constant_dependency_symbol_ids_in_expression(
        expression,
        local_constant_symbol_id_by_name,
        imported_constant_symbol_id_by_name,
        &bound_names,
        &mut dependency_symbol_ids,
    );
    dependency_symbol_ids
}

fn collect_constant_dependency_symbol_ids_in_expression(
    expression: &Expression,
    local_constant_symbol_id_by_name: &HashMap<String, PublicSymbolId>,
    imported_constant_symbol_id_by_name: &HashMap<String, PublicSymbolId>,
    bound_names: &BTreeSet<String>,
    dependency_symbol_ids: &mut BTreeSet<PublicSymbolId>,
) {
    match expression {
        Expression::Identifier { name, .. } => {
            if bound_names.contains(name) {
                return;
            }
            if let Some(symbol_id) = local_constant_symbol_id_by_name.get(name) {
                dependency_symbol_ids.insert(*symbol_id);
                return;
            }
            if let Some(symbol_id) = imported_constant_symbol_id_by_name.get(name) {
                dependency_symbol_ids.insert(*symbol_id);
            }
        }
        Expression::StructLiteral { fields, .. } => {
            for field in fields {
                collect_constant_dependency_symbol_ids_in_expression(
                    &field.value,
                    local_constant_symbol_id_by_name,
                    imported_constant_symbol_id_by_name,
                    bound_names,
                    dependency_symbol_ids,
                );
            }
        }
        Expression::FieldAccess { target, .. } => {
            collect_constant_dependency_symbol_ids_in_expression(
                target,
                local_constant_symbol_id_by_name,
                imported_constant_symbol_id_by_name,
                bound_names,
                dependency_symbol_ids,
            );
        }
        Expression::Call {
            callee, arguments, ..
        } => {
            collect_constant_dependency_symbol_ids_in_expression(
                callee,
                local_constant_symbol_id_by_name,
                imported_constant_symbol_id_by_name,
                bound_names,
                dependency_symbol_ids,
            );
            for argument in arguments {
                collect_constant_dependency_symbol_ids_in_expression(
                    argument,
                    local_constant_symbol_id_by_name,
                    imported_constant_symbol_id_by_name,
                    bound_names,
                    dependency_symbol_ids,
                );
            }
        }
        Expression::Unary { expression, .. } => {
            collect_constant_dependency_symbol_ids_in_expression(
                expression,
                local_constant_symbol_id_by_name,
                imported_constant_symbol_id_by_name,
                bound_names,
                dependency_symbol_ids,
            );
        }
        Expression::Binary { left, right, .. } => {
            collect_constant_dependency_symbol_ids_in_expression(
                left,
                local_constant_symbol_id_by_name,
                imported_constant_symbol_id_by_name,
                bound_names,
                dependency_symbol_ids,
            );
            collect_constant_dependency_symbol_ids_in_expression(
                right,
                local_constant_symbol_id_by_name,
                imported_constant_symbol_id_by_name,
                bound_names,
                dependency_symbol_ids,
            );
        }
        Expression::Match { target, arms, .. } => {
            collect_constant_dependency_symbol_ids_in_expression(
                target,
                local_constant_symbol_id_by_name,
                imported_constant_symbol_id_by_name,
                bound_names,
                dependency_symbol_ids,
            );
            for arm in arms {
                let mut arm_bound_names = bound_names.clone();
                if let MatchPattern::Binding { name, .. } = &arm.pattern {
                    arm_bound_names.insert(name.clone());
                }
                collect_constant_dependency_symbol_ids_in_expression(
                    &arm.value,
                    local_constant_symbol_id_by_name,
                    imported_constant_symbol_id_by_name,
                    &arm_bound_names,
                    dependency_symbol_ids,
                );
            }
        }
        Expression::Matches { value, .. } => collect_constant_dependency_symbol_ids_in_expression(
            value,
            local_constant_symbol_id_by_name,
            imported_constant_symbol_id_by_name,
            bound_names,
            dependency_symbol_ids,
        ),
        Expression::IntegerLiteral { .. }
        | Expression::NilLiteral { .. }
        | Expression::BooleanLiteral { .. }
        | Expression::StringLiteral { .. } => {}
    }
}

fn components_in_dependency_order(
    constant_symbol_ids: &[PublicSymbolId],
    dependency_symbol_ids_by_constant_symbol_id: &BTreeMap<PublicSymbolId, Vec<PublicSymbolId>>,
) -> Vec<Vec<PublicSymbolId>> {
    let components = strongly_connected_components(
        constant_symbol_ids,
        dependency_symbol_ids_by_constant_symbol_id,
    );

    let mut component_index_by_symbol_id = BTreeMap::new();
    for (component_index, component) in components.iter().enumerate() {
        for symbol_id in component {
            component_index_by_symbol_id.insert(*symbol_id, component_index);
        }
    }

    let mut dependent_component_indexes_by_component_index =
        vec![BTreeSet::new(); components.len()];
    let mut component_indegree = vec![0usize; components.len()];
    for (symbol_id, dependency_symbol_ids) in dependency_symbol_ids_by_constant_symbol_id {
        let Some(component_index) = component_index_by_symbol_id.get(symbol_id).copied() else {
            continue;
        };
        for dependency_symbol_id in dependency_symbol_ids {
            let Some(dependency_component_index) = component_index_by_symbol_id
                .get(dependency_symbol_id)
                .copied()
            else {
                continue;
            };
            if dependency_component_index == component_index {
                continue;
            }
            if dependent_component_indexes_by_component_index[dependency_component_index]
                .insert(component_index)
            {
                component_indegree[component_index] += 1;
            }
        }
    }

    let mut pending = BTreeSet::new();
    for (component_index, indegree) in component_indegree.iter().enumerate() {
        if *indegree == 0 {
            pending.insert(component_index);
        }
    }

    let mut ordered_component_indexes = Vec::new();
    while let Some(component_index) = pending.pop_first() {
        ordered_component_indexes.push(component_index);
        for dependent_component_index in
            &dependent_component_indexes_by_component_index[component_index]
        {
            component_indegree[*dependent_component_index] -= 1;
            if component_indegree[*dependent_component_index] == 0 {
                pending.insert(*dependent_component_index);
            }
        }
    }

    ordered_component_indexes
        .into_iter()
        .map(|component_index| components[component_index].clone())
        .collect()
}

fn strongly_connected_components(
    constant_symbol_ids: &[PublicSymbolId],
    dependency_symbol_ids_by_constant_symbol_id: &BTreeMap<PublicSymbolId, Vec<PublicSymbolId>>,
) -> Vec<Vec<PublicSymbolId>> {
    struct TarjanState {
        next_index: usize,
        index_by_symbol_id: BTreeMap<PublicSymbolId, usize>,
        lowlink_by_symbol_id: BTreeMap<PublicSymbolId, usize>,
        stack: Vec<PublicSymbolId>,
        on_stack: BTreeSet<PublicSymbolId>,
        components: Vec<Vec<PublicSymbolId>>,
    }

    fn visit(
        symbol_id: PublicSymbolId,
        dependency_symbol_ids_by_constant_symbol_id: &BTreeMap<PublicSymbolId, Vec<PublicSymbolId>>,
        state: &mut TarjanState,
    ) {
        let current_index = state.next_index;
        state.next_index += 1;
        state.index_by_symbol_id.insert(symbol_id, current_index);
        state.lowlink_by_symbol_id.insert(symbol_id, current_index);
        state.stack.push(symbol_id);
        state.on_stack.insert(symbol_id);

        let dependency_symbol_ids = dependency_symbol_ids_by_constant_symbol_id
            .get(&symbol_id)
            .cloned()
            .unwrap_or_default();
        for dependency_symbol_id in dependency_symbol_ids {
            if !state.index_by_symbol_id.contains_key(&dependency_symbol_id) {
                visit(
                    dependency_symbol_id,
                    dependency_symbol_ids_by_constant_symbol_id,
                    state,
                );
                let lowlink = state.lowlink_by_symbol_id[&symbol_id]
                    .min(state.lowlink_by_symbol_id[&dependency_symbol_id]);
                state.lowlink_by_symbol_id.insert(symbol_id, lowlink);
            } else if state.on_stack.contains(&dependency_symbol_id) {
                let lowlink = state.lowlink_by_symbol_id[&symbol_id]
                    .min(state.index_by_symbol_id[&dependency_symbol_id]);
                state.lowlink_by_symbol_id.insert(symbol_id, lowlink);
            }
        }

        if state.lowlink_by_symbol_id[&symbol_id] == state.index_by_symbol_id[&symbol_id] {
            let mut component = Vec::new();
            loop {
                let stack_symbol_id = state
                    .stack
                    .pop()
                    .expect("stack must contain current symbol when forming SCC");
                state.on_stack.remove(&stack_symbol_id);
                component.push(stack_symbol_id);
                if stack_symbol_id == symbol_id {
                    break;
                }
            }
            component.sort();
            state.components.push(component);
        }
    }

    let mut state = TarjanState {
        next_index: 0,
        index_by_symbol_id: BTreeMap::new(),
        lowlink_by_symbol_id: BTreeMap::new(),
        stack: Vec::new(),
        on_stack: BTreeSet::new(),
        components: Vec::new(),
    };

    for symbol_id in constant_symbol_ids {
        if !state.index_by_symbol_id.contains_key(symbol_id) {
            visit(
                *symbol_id,
                dependency_symbol_ids_by_constant_symbol_id,
                &mut state,
            );
        }
    }

    state.components
}

fn resolve_constant_component(
    component_constant_symbol_ids: &[PublicSymbolId],
    public_constant_definition_by_symbol_id: &BTreeMap<PublicSymbolId, PublicConstantDefinition>,
    dependency_symbol_ids_by_constant_symbol_id: &BTreeMap<PublicSymbolId, Vec<PublicSymbolId>>,
    package_unit_by_path: &BTreeMap<PathBuf, &PackageUnit<'_>>,
    resolved_imports: &[ResolvedImportSummary],
    symbol_id_by_lookup_key: &BTreeMap<PublicSymbolLookupKey, PublicSymbolId>,
    typed_symbol_by_id: &mut BTreeMap<PublicSymbolId, TypedPublicSymbol>,
) {
    let mut constant_symbol_ids_by_path: BTreeMap<PathBuf, Vec<(PublicSymbolId, String)>> =
        BTreeMap::new();
    for symbol_id in component_constant_symbol_ids {
        let Some(definition) = public_constant_definition_by_symbol_id.get(symbol_id) else {
            continue;
        };
        constant_symbol_ids_by_path
            .entry(definition.path.clone())
            .or_default()
            .push((*symbol_id, definition.symbol_name.clone()));
    }

    let mut paths: Vec<PathBuf> = constant_symbol_ids_by_path.keys().cloned().collect();
    paths.sort_by(|left, right| compare_paths(left, right));

    let has_self_dependency = component_constant_symbol_ids.iter().any(|symbol_id| {
        dependency_symbol_ids_by_constant_symbol_id
            .get(symbol_id)
            .is_some_and(|deps| deps.contains(symbol_id))
    });
    let max_iterations = if component_constant_symbol_ids.len() == 1 && !has_self_dependency {
        1
    } else {
        component_constant_symbol_ids.len() + 1
    };

    for _ in 0..max_iterations {
        let imported_bindings_by_file = build_imported_bindings_by_file(
            resolved_imports,
            symbol_id_by_lookup_key,
            typed_symbol_by_id,
        );
        let mut changed = false;

        for path in &paths {
            let Some(package_unit) = package_unit_by_path.get(path) else {
                continue;
            };
            let imported_bindings = imported_bindings_by_file
                .get(path)
                .map_or(&[][..], Vec::as_slice);
            let mut ignored_diagnostics = Vec::new();
            let summary = analyze_package_unit(
                package_unit.parsed,
                imported_bindings,
                &mut ignored_diagnostics,
            );
            let Some(constant_symbols) = constant_symbol_ids_by_path.get(path) else {
                continue;
            };
            for (symbol_id, symbol_name) in constant_symbols {
                let Some(TypedSymbol::Constant(value_type)) =
                    summary.typed_symbol_by_name.get(symbol_name)
                else {
                    continue;
                };
                if matches!(
                    typed_symbol_by_id.get(symbol_id),
                    Some(TypedPublicSymbol::Constant(existing)) if existing == value_type
                ) {
                    continue;
                }
                typed_symbol_by_id
                    .insert(*symbol_id, TypedPublicSymbol::Constant(value_type.clone()));
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }
}

fn build_imported_bindings_by_file(
    resolved_imports: &[ResolvedImportSummary],
    symbol_id_by_lookup_key: &BTreeMap<PublicSymbolLookupKey, PublicSymbolId>,
    typed_symbol_by_id: &BTreeMap<PublicSymbolId, TypedPublicSymbol>,
) -> BTreeMap<PathBuf, Vec<ImportedBinding>> {
    let mut imported_by_file: BTreeMap<PathBuf, Vec<ImportedBinding>> = BTreeMap::new();
    let mut local_type_name_by_declared_type_name_by_source_and_target: BTreeMap<
        (PathBuf, PackageId),
        HashMap<String, String>,
    > = BTreeMap::new();

    for resolved_import in resolved_imports {
        for binding in &resolved_import.bindings {
            let lookup_key = PublicSymbolLookupKey {
                package_id: resolved_import.target_package_id,
                symbol_name: binding.imported_name.clone(),
            };
            let Some(symbol_id) = symbol_id_by_lookup_key.get(&lookup_key) else {
                continue;
            };
            if matches!(
                typed_symbol_by_id.get(symbol_id),
                Some(TypedPublicSymbol::Type(_))
            ) {
                local_type_name_by_declared_type_name_by_source_and_target
                    .entry((
                        resolved_import.source_path.clone(),
                        resolved_import.target_package_id,
                    ))
                    .or_default()
                    .insert(binding.imported_name.clone(), binding.local_name.clone());
            }
        }
    }

    for resolved_import in resolved_imports {
        let local_type_name_by_declared_type_name =
            local_type_name_by_declared_type_name_by_source_and_target
                .get(&(
                    resolved_import.source_path.clone(),
                    resolved_import.target_package_id,
                ))
                .cloned()
                .unwrap_or_default();

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
                    ImportedSymbol::Type(type_declaration.clone())
                }
                TypedPublicSymbol::Function(function_declaration) => {
                    ImportedSymbol::Function(rewrite_function_signature_type_aliases(
                        function_declaration,
                        &local_type_name_by_declared_type_name,
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

fn rewrite_function_signature_type_aliases(
    function_declaration: &FunctionDeclaration,
    local_type_name_by_declared_type_name: &HashMap<String, String>,
) -> FunctionDeclaration {
    let parameters = function_declaration
        .parameters
        .iter()
        .map(|parameter| ParameterDeclaration {
            name: parameter.name.clone(),
            span: parameter.span.clone(),
            type_name: rewrite_type_name_aliases(
                &parameter.type_name,
                local_type_name_by_declared_type_name,
            ),
        })
        .collect();

    FunctionDeclaration {
        name: function_declaration.name.clone(),
        name_span: function_declaration.name_span.clone(),
        visibility: function_declaration.visibility,
        parameters,
        return_type: rewrite_type_name_aliases(
            &function_declaration.return_type,
            local_type_name_by_declared_type_name,
        ),
        body: function_declaration.body.clone(),
        doc: function_declaration.doc.clone(),
        span: function_declaration.span.clone(),
    }
}

fn rewrite_type_name_aliases(
    type_name: &TypeName,
    local_type_name_by_declared_type_name: &HashMap<String, String>,
) -> TypeName {
    let names = type_name
        .names
        .iter()
        .map(|atom| {
            let name = local_type_name_by_declared_type_name
                .get(&atom.name)
                .cloned()
                .unwrap_or_else(|| atom.name.clone());
            TypeNameAtom {
                name,
                span: atom.span.clone(),
            }
        })
        .collect();

    TypeName {
        names,
        span: type_name.span.clone(),
    }
}
