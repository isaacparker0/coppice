use std::collections::HashSet;

use compiler__semantic_program::{
    SemanticConstantDeclaration, SemanticFunctionDeclaration, SemanticTypeDeclaration,
    SemanticTypeDeclarationKind,
};
use compiler__semantic_types::{
    GenericTypeParameter, ImportedTypeShape, NominalTypeId, NominalTypeRef,
};

use super::{
    FunctionInfo, ImportedTypeDeclaration, InterfaceMethodSignature, MethodInfo, MethodKey,
    TypeChecker, TypeInfo, TypeKind, TypedFunctionSignature,
};

struct ImportedTypeBinding {
    local_name: String,
    type_declaration: ImportedTypeDeclaration,
}

struct ImportedFunctionBinding {
    local_name: String,
    signature: TypedFunctionSignature,
}

impl TypeChecker<'_> {
    pub(super) fn collect_imported_type_declarations(&mut self) {
        let imported_type_bindings: Vec<ImportedTypeBinding> = self
            .imported_bindings
            .iter()
            .filter_map(|(local_name, binding)| match &binding.symbol {
                super::ImportedSymbol::Type(type_declaration) => Some(ImportedTypeBinding {
                    local_name: local_name.clone(),
                    type_declaration: type_declaration.clone(),
                }),
                super::ImportedSymbol::Function(_) | super::ImportedSymbol::Constant(_) => None,
            })
            .collect();

        for imported_binding in &imported_type_bindings {
            if self.types.contains_key(&imported_binding.local_name) {
                continue;
            }
            let kind = match &imported_binding.type_declaration.kind {
                ImportedTypeShape::Struct { .. } => TypeKind::Struct { fields: Vec::new() },
                ImportedTypeShape::Interface { .. } => TypeKind::Interface {
                    methods: Vec::new(),
                },
                ImportedTypeShape::Union { .. } => TypeKind::Union {
                    variants: Vec::new(),
                },
            };
            self.types.insert(
                imported_binding.local_name.clone(),
                TypeInfo {
                    nominal_type_id: imported_binding.type_declaration.nominal_type_id.clone(),
                    type_parameters: imported_binding.type_declaration.type_parameters.clone(),
                    implemented_interfaces: imported_binding
                        .type_declaration
                        .implemented_interfaces
                        .clone(),
                    kind,
                },
            );
        }

        for imported_binding in imported_type_bindings {
            match &imported_binding.type_declaration.kind {
                ImportedTypeShape::Struct { fields, .. } => {
                    let mut resolved_fields = Vec::new();
                    let mut seen = HashSet::new();
                    for (field_name, field_type) in fields {
                        if !seen.insert(field_name.clone()) {
                            continue;
                        }
                        resolved_fields.push((field_name.clone(), field_type.clone()));
                    }
                    if let Some(info) = self.types.get_mut(&imported_binding.local_name) {
                        info.kind = TypeKind::Struct {
                            fields: resolved_fields,
                        };
                    }
                }
                ImportedTypeShape::Interface { methods } => {
                    let mut resolved_methods = Vec::new();
                    let mut seen = HashSet::new();
                    for method in methods {
                        if !seen.insert(method.name.clone()) {
                            continue;
                        }
                        resolved_methods.push(InterfaceMethodSignature {
                            name: method.name.clone(),
                            self_mutable: method.self_mutable,
                            parameter_types: method.parameter_types.clone(),
                            return_type: method.return_type.clone(),
                        });
                    }
                    if let Some(info) = self.types.get_mut(&imported_binding.local_name) {
                        info.kind = TypeKind::Interface {
                            methods: resolved_methods,
                        };
                    }
                }
                ImportedTypeShape::Union { variants } => {
                    let mut resolved_variants = Vec::new();
                    let mut seen = HashSet::new();
                    for variant in variants {
                        let key = variant.display();
                        if !seen.insert(key) {
                            continue;
                        }
                        resolved_variants.push(variant.clone());
                    }
                    if let Some(info) = self.types.get_mut(&imported_binding.local_name) {
                        info.kind = TypeKind::Union {
                            variants: resolved_variants,
                        };
                    }
                }
            }
        }
    }

    pub(super) fn collect_imported_function_signatures(&mut self) {
        let imported_function_bindings: Vec<ImportedFunctionBinding> = self
            .imported_bindings
            .iter()
            .filter_map(|(local_name, binding)| match &binding.symbol {
                super::ImportedSymbol::Function(signature) => Some(ImportedFunctionBinding {
                    local_name: local_name.clone(),
                    signature: signature.clone(),
                }),
                super::ImportedSymbol::Type(_) | super::ImportedSymbol::Constant(_) => None,
            })
            .collect();

        for imported_binding in imported_function_bindings {
            self.imported_functions.insert(
                imported_binding.local_name,
                FunctionInfo {
                    type_parameters: imported_binding.signature.type_parameters,
                    parameter_types: imported_binding.signature.parameter_types,
                    return_type: imported_binding.signature.return_type,
                },
            );
        }
    }

    pub(super) fn collect_imported_method_signatures(&mut self) {
        let imported_type_bindings: Vec<ImportedTypeBinding> = self
            .imported_bindings
            .iter()
            .filter_map(|(local_name, binding)| match &binding.symbol {
                super::ImportedSymbol::Type(type_declaration) => Some(ImportedTypeBinding {
                    local_name: local_name.clone(),
                    type_declaration: type_declaration.clone(),
                }),
                super::ImportedSymbol::Function(_) | super::ImportedSymbol::Constant(_) => None,
            })
            .collect();

        for imported_binding in imported_type_bindings {
            let methods = match &imported_binding.type_declaration.kind {
                ImportedTypeShape::Struct { methods, .. }
                | ImportedTypeShape::Interface { methods } => methods,
                ImportedTypeShape::Union { .. } => continue,
            };

            for method in methods {
                let method_key = MethodKey {
                    receiver_type_id: imported_binding.type_declaration.nominal_type_id.clone(),
                    method_name: method.name.clone(),
                };
                if self.methods.contains_key(&method_key) {
                    continue;
                }

                self.methods.insert(
                    method_key,
                    MethodInfo {
                        self_mutable: method.self_mutable,
                        parameter_types: method.parameter_types.clone(),
                        return_type: method.return_type.clone(),
                    },
                );
            }
        }
    }

    pub(super) fn collect_type_declarations(&mut self, types: &[SemanticTypeDeclaration]) {
        for type_declaration in types {
            self.check_type_name(&type_declaration.name, &type_declaration.span);
            if self.types.contains_key(&type_declaration.name) {
                self.error(
                    format!("duplicate type '{}'", type_declaration.name),
                    type_declaration.span.clone(),
                );
                continue;
            }
            let kind = match &type_declaration.kind {
                SemanticTypeDeclarationKind::Struct { .. } => {
                    TypeKind::Struct { fields: Vec::new() }
                }
                SemanticTypeDeclarationKind::Interface { .. } => TypeKind::Interface {
                    methods: Vec::new(),
                },
                SemanticTypeDeclarationKind::Enum { .. }
                | SemanticTypeDeclarationKind::Union { .. } => TypeKind::Union {
                    variants: Vec::new(),
                },
            };
            self.types.insert(
                type_declaration.name.clone(),
                TypeInfo {
                    nominal_type_id: NominalTypeId {
                        package_id: self.package_id,
                        symbol_name: type_declaration.name.clone(),
                    },
                    type_parameters: type_declaration
                        .type_parameters
                        .iter()
                        .map(|parameter| GenericTypeParameter {
                            name: parameter.name.clone(),
                            constraint: None,
                        })
                        .collect(),
                    implemented_interfaces: Vec::new(),
                    kind,
                },
            );
        }

        for type_declaration in types {
            let names_and_spans = type_declaration
                .type_parameters
                .iter()
                .map(|parameter| (parameter.name.clone(), parameter.span.clone()))
                .collect::<Vec<_>>();
            self.push_type_parameters(&names_and_spans);
            let resolved_type_parameters = type_declaration
                .type_parameters
                .iter()
                .map(|parameter| GenericTypeParameter {
                    name: parameter.name.clone(),
                    constraint: parameter
                        .constraint
                        .as_ref()
                        .map(|constraint| self.resolve_type_name(constraint)),
                })
                .collect::<Vec<_>>();
            let resolved_implemented_interfaces = type_declaration
                .implemented_interfaces
                .iter()
                .map(|implemented_interface| self.resolve_type_name(implemented_interface))
                .collect::<Vec<_>>();
            if let Some(info) = self.types.get_mut(&type_declaration.name) {
                info.type_parameters = resolved_type_parameters;
                info.implemented_interfaces = resolved_implemented_interfaces;
            }

            match &type_declaration.kind {
                SemanticTypeDeclarationKind::Struct { fields, .. } => {
                    let mut resolved_fields = Vec::new();
                    let mut seen = HashSet::new();
                    for field in fields {
                        if !seen.insert(field.name.clone()) {
                            self.error(
                                format!(
                                    "duplicate field '{}' in '{}'",
                                    field.name, type_declaration.name
                                ),
                                field.span.clone(),
                            );
                            continue;
                        }
                        let field_type = self.resolve_type_name(&field.type_name);
                        resolved_fields.push((field.name.clone(), field_type));
                    }
                    if let Some(info) = self.types.get_mut(&type_declaration.name) {
                        info.kind = TypeKind::Struct {
                            fields: resolved_fields,
                        };
                    }
                }
                SemanticTypeDeclarationKind::Interface { methods } => {
                    let mut resolved_methods = Vec::new();
                    let mut seen = HashSet::new();
                    for method in methods {
                        self.check_function_name(&method.name, &method.name_span);
                        if !seen.insert(method.name.clone()) {
                            self.error(
                                format!(
                                    "duplicate method '{}.{}'",
                                    type_declaration.name, method.name
                                ),
                                method.name_span.clone(),
                            );
                            continue;
                        }
                        let return_type = self.resolve_type_name(&method.return_type);
                        let parameter_types = method
                            .parameters
                            .iter()
                            .map(|parameter| self.resolve_type_name(&parameter.type_name))
                            .collect::<Vec<_>>();
                        resolved_methods.push(InterfaceMethodSignature {
                            name: method.name.clone(),
                            self_mutable: method.self_mutable,
                            parameter_types,
                            return_type,
                        });
                    }
                    if let Some(info) = self.types.get_mut(&type_declaration.name) {
                        info.kind = TypeKind::Interface {
                            methods: resolved_methods,
                        };
                    }
                }
                SemanticTypeDeclarationKind::Enum { variants } => {
                    if !type_declaration.type_parameters.is_empty() {
                        self.error(
                            format!(
                                "enum type '{}' cannot declare type parameters",
                                type_declaration.name
                            ),
                            type_declaration.span.clone(),
                        );
                    }
                    let mut resolved_variants = Vec::new();
                    let mut seen = HashSet::new();
                    for variant in variants {
                        if !seen.insert(variant.name.clone()) {
                            self.error(
                                format!("duplicate enum variant '{}'", variant.name),
                                variant.span.clone(),
                            );
                            continue;
                        }
                        resolved_variants.push(super::Type::Named(NominalTypeRef {
                            id: NominalTypeId {
                                package_id: self.package_id,
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
                        }));
                    }
                    if let Some(info) = self.types.get_mut(&type_declaration.name) {
                        info.kind = TypeKind::Union {
                            variants: resolved_variants,
                        };
                    }
                }
                SemanticTypeDeclarationKind::Union { variants } => {
                    let mut resolved_variants = Vec::new();
                    let mut seen = HashSet::new();
                    for variant in variants {
                        if variant.names.len() != 1 {
                            self.error("union variants must be single types", variant.span.clone());
                            continue;
                        }
                        let variant_type = self.resolve_type_name(variant);
                        let key = variant_type.display();
                        if !seen.insert(key.clone()) {
                            self.error(
                                format!("duplicate union variant '{key}'"),
                                variant.span.clone(),
                            );
                            continue;
                        }
                        resolved_variants.push(variant_type);
                    }
                    if let Some(info) = self.types.get_mut(&type_declaration.name) {
                        info.kind = TypeKind::Union {
                            variants: resolved_variants,
                        };
                    }
                }
            }
            self.pop_type_parameters();
        }
    }

    pub(super) fn collect_function_signatures(
        &mut self,
        functions: &[SemanticFunctionDeclaration],
    ) {
        for function in functions {
            self.check_function_name(&function.name, &function.name_span);
            if self.functions.contains_key(&function.name) {
                self.error(
                    format!("duplicate function '{}'", function.name),
                    function.name_span.clone(),
                );
                continue;
            }

            let names_and_spans = function
                .type_parameters
                .iter()
                .map(|parameter| (parameter.name.clone(), parameter.span.clone()))
                .collect::<Vec<_>>();
            self.push_type_parameters(&names_and_spans);
            let return_type = self.resolve_type_name(&function.return_type);

            let mut parameter_types = Vec::new();
            for parameter in &function.parameters {
                let value_type = self.resolve_type_name(&parameter.type_name);
                parameter_types.push(value_type);
            }
            let resolved_type_parameters = function
                .type_parameters
                .iter()
                .map(|parameter| GenericTypeParameter {
                    name: parameter.name.clone(),
                    constraint: parameter
                        .constraint
                        .as_ref()
                        .map(|constraint| self.resolve_type_name(constraint)),
                })
                .collect::<Vec<_>>();
            self.pop_type_parameters();

            self.functions.insert(
                function.name.clone(),
                FunctionInfo {
                    type_parameters: resolved_type_parameters,
                    parameter_types,
                    return_type,
                },
            );
        }
    }

    pub(super) fn collect_method_signatures(&mut self, types: &[SemanticTypeDeclaration]) {
        for type_declaration in types {
            match &type_declaration.kind {
                SemanticTypeDeclarationKind::Struct { methods, .. } => {
                    let names_and_spans = type_declaration
                        .type_parameters
                        .iter()
                        .map(|parameter| (parameter.name.clone(), parameter.span.clone()))
                        .collect::<Vec<_>>();
                    self.push_type_parameters(&names_and_spans);

                    for method in methods {
                        self.check_function_name(&method.name, &method.name_span);
                        let method_key = MethodKey {
                            receiver_type_id: NominalTypeId {
                                package_id: self.package_id,
                                symbol_name: type_declaration.name.clone(),
                            },
                            method_name: method.name.clone(),
                        };
                        if self.methods.contains_key(&method_key) {
                            self.error(
                                format!(
                                    "duplicate method '{}.{}'",
                                    type_declaration.name, method.name
                                ),
                                method.name_span.clone(),
                            );
                            continue;
                        }

                        let return_type = self.resolve_type_name(&method.return_type);
                        let mut parameter_types = Vec::new();
                        for parameter in &method.parameters {
                            let value_type = self.resolve_type_name(&parameter.type_name);
                            parameter_types.push(value_type);
                        }

                        self.methods.insert(
                            method_key,
                            MethodInfo {
                                self_mutable: method.self_mutable,
                                parameter_types,
                                return_type,
                            },
                        );
                    }
                    self.pop_type_parameters();
                }
                SemanticTypeDeclarationKind::Interface { methods } => {
                    let names_and_spans = type_declaration
                        .type_parameters
                        .iter()
                        .map(|parameter| (parameter.name.clone(), parameter.span.clone()))
                        .collect::<Vec<_>>();
                    self.push_type_parameters(&names_and_spans);

                    for method in methods {
                        self.check_function_name(&method.name, &method.name_span);
                        let method_key = MethodKey {
                            receiver_type_id: NominalTypeId {
                                package_id: self.package_id,
                                symbol_name: type_declaration.name.clone(),
                            },
                            method_name: method.name.clone(),
                        };
                        if self.methods.contains_key(&method_key) {
                            self.error(
                                format!(
                                    "duplicate method '{}.{}'",
                                    type_declaration.name, method.name
                                ),
                                method.name_span.clone(),
                            );
                            continue;
                        }

                        let return_type = self.resolve_type_name(&method.return_type);
                        let mut parameter_types = Vec::new();
                        for parameter in &method.parameters {
                            let value_type = self.resolve_type_name(&parameter.type_name);
                            parameter_types.push(value_type);
                        }

                        self.methods.insert(
                            method_key,
                            MethodInfo {
                                self_mutable: method.self_mutable,
                                parameter_types,
                                return_type,
                            },
                        );
                    }
                    self.pop_type_parameters();
                }
                SemanticTypeDeclarationKind::Enum { .. }
                | SemanticTypeDeclarationKind::Union { .. } => {}
            }
        }
    }

    pub(super) fn check_constant_declarations(
        &mut self,
        constants: &[SemanticConstantDeclaration],
    ) {
        for constant in constants {
            self.check_constant_name(&constant.name, &constant.span);
            let value_type = self.check_expression(&constant.expression);
            let declared_type = self.resolve_type_name(&constant.type_name);
            if self.constants.contains_key(&constant.name) {
                self.error(
                    format!("duplicate constant '{name}'", name = constant.name),
                    constant.span.clone(),
                );
                continue;
            }
            if declared_type != super::Type::Unknown
                && value_type != super::Type::Unknown
                && !self.is_assignable(&value_type, &declared_type)
            {
                self.error(
                    format!(
                        "type mismatch: expected {}, got {}",
                        declared_type.display(),
                        value_type.display()
                    ),
                    constant.span.clone(),
                );
            }
            self.constants.insert(
                constant.name.clone(),
                super::ConstantInfo {
                    value_type: if declared_type == super::Type::Unknown {
                        value_type
                    } else {
                        declared_type
                    },
                },
            );
        }
    }

    pub(super) fn check_type_interface_conformance(&mut self, types: &[SemanticTypeDeclaration]) {
        for type_declaration in types {
            if !matches!(
                type_declaration.kind,
                SemanticTypeDeclarationKind::Struct { .. }
            ) && !type_declaration.implemented_interfaces.is_empty()
            {
                self.error(
                    format!(
                        "only struct types can declare implements clauses; '{}' is not a struct",
                        type_declaration.name
                    ),
                    type_declaration.implemented_interfaces[0].span.clone(),
                );
                continue;
            }
            if !matches!(
                type_declaration.kind,
                SemanticTypeDeclarationKind::Struct { .. }
            ) {
                continue;
            }
            self.check_struct_interface_conformance(type_declaration);
        }
    }

    fn check_struct_interface_conformance(&mut self, type_declaration: &SemanticTypeDeclaration) {
        let struct_type_info = self.types.get(&type_declaration.name).cloned();
        let Some(struct_type_info) = struct_type_info else {
            return;
        };
        let struct_type_id = struct_type_info.nominal_type_id;
        let mut seen_interface_names = HashSet::new();
        for implemented_interface in &type_declaration.implemented_interfaces {
            let implemented_interface_type = self.resolve_type_name(implemented_interface);
            if implemented_interface_type == super::Type::Unknown {
                continue;
            }
            let interface_name = implemented_interface_type.display();
            if !seen_interface_names.insert(interface_name.clone()) {
                self.error(
                    format!("duplicate implements entry '{interface_name}'"),
                    implemented_interface.span.clone(),
                );
                continue;
            }

            let Some(interface_type_id) =
                Self::nominal_type_id_for_type(&implemented_interface_type)
            else {
                self.error(
                    format!("implemented type '{interface_name}' must be an interface declaration"),
                    implemented_interface.span.clone(),
                );
                continue;
            };
            let Some(interface_type_info) = self.type_info_by_nominal_type_id(&interface_type_id)
            else {
                continue;
            };
            let TypeKind::Interface { methods } = &interface_type_info.kind else {
                self.error(
                    format!("implemented type '{interface_name}' must be an interface declaration"),
                    implemented_interface.span.clone(),
                );
                continue;
            };
            let methods = methods.clone();

            for interface_method in methods {
                let method_key = MethodKey {
                    receiver_type_id: struct_type_id.clone(),
                    method_name: interface_method.name.clone(),
                };
                let Some(struct_method) = self.methods.get(&method_key) else {
                    self.error(
                        format!(
                            "type '{}' does not implement interface '{}': missing method '{}'",
                            type_declaration.name, interface_name, interface_method.name
                        ),
                        implemented_interface.span.clone(),
                    );
                    continue;
                };
                if struct_method.self_mutable != interface_method.self_mutable
                    || struct_method.parameter_types != interface_method.parameter_types
                    || struct_method.return_type != interface_method.return_type
                {
                    self.error(
                        format!(
                            "type '{}' method '{}' does not match interface '{}'",
                            type_declaration.name, interface_method.name, interface_name
                        ),
                        implemented_interface.span.clone(),
                    );
                }
            }
        }
    }
}
