use std::collections::HashSet;

use crate::types::Type;
use compiler__syntax::{
    ConstantDeclaration, FunctionDeclaration, TypeDeclaration, TypeDeclarationKind,
};

use super::{FunctionInfo, MethodInfo, MethodKey, TypeChecker, TypeInfo, TypeKind};

impl TypeChecker<'_> {
    pub(super) fn collect_imported_type_declarations(&mut self) {
        let imported_type_bindings: Vec<(String, TypeDeclaration)> = self
            .imported_bindings
            .iter()
            .filter_map(|(local_name, binding)| match &binding.symbol {
                super::ImportedSymbol::Type(type_declaration) => {
                    Some((local_name.clone(), type_declaration.clone()))
                }
                super::ImportedSymbol::Function(_) | super::ImportedSymbol::Constant(_) => None,
            })
            .collect();

        for (local_name, type_declaration) in &imported_type_bindings {
            if self.types.contains_key(local_name) {
                continue;
            }
            let kind = match &type_declaration.kind {
                TypeDeclarationKind::Struct { .. } => TypeKind::Struct { fields: Vec::new() },
                TypeDeclarationKind::Union { .. } => TypeKind::Union {
                    variants: Vec::new(),
                },
            };
            self.types.insert(local_name.clone(), TypeInfo { kind });
        }

        for (local_name, type_declaration) in imported_type_bindings {
            match &type_declaration.kind {
                TypeDeclarationKind::Struct { fields, .. } => {
                    let mut resolved_fields = Vec::new();
                    let mut seen = HashSet::new();
                    for field in fields {
                        if !seen.insert(field.name.clone()) {
                            continue;
                        }
                        let field_type = self.resolve_type_name(&field.type_name);
                        resolved_fields.push((field.name.clone(), field_type));
                    }
                    if let Some(info) = self.types.get_mut(&local_name) {
                        info.kind = TypeKind::Struct {
                            fields: resolved_fields,
                        };
                    }
                }
                TypeDeclarationKind::Union { variants } => {
                    let mut resolved_variants = Vec::new();
                    let mut seen = HashSet::new();
                    for variant in variants {
                        if variant.names.len() != 1 {
                            continue;
                        }
                        let variant_type = self.resolve_type_name(variant);
                        let key = variant_type.display();
                        if !seen.insert(key) {
                            continue;
                        }
                        resolved_variants.push(variant_type);
                    }
                    if let Some(info) = self.types.get_mut(&local_name) {
                        info.kind = TypeKind::Union {
                            variants: resolved_variants,
                        };
                    }
                }
            }
        }
    }

    pub(super) fn collect_imported_function_signatures(&mut self) {
        let imported_function_bindings: Vec<(String, FunctionDeclaration)> = self
            .imported_bindings
            .iter()
            .filter_map(|(local_name, binding)| match &binding.symbol {
                super::ImportedSymbol::Function(function) => {
                    Some((local_name.clone(), function.clone()))
                }
                super::ImportedSymbol::Type(_) | super::ImportedSymbol::Constant(_) => None,
            })
            .collect();

        for (local_name, function) in imported_function_bindings {
            let return_type = self.resolve_type_name(&function.return_type);
            let mut parameter_types = Vec::new();
            for parameter in &function.parameters {
                parameter_types.push(self.resolve_type_name(&parameter.type_name));
            }
            self.imported_functions.insert(
                local_name,
                FunctionInfo {
                    parameter_types,
                    return_type,
                },
            );
        }

        let imported_constant_names: Vec<String> = self
            .imported_bindings
            .iter()
            .filter_map(|(local_name, binding)| match &binding.symbol {
                super::ImportedSymbol::Constant(_) => Some(local_name.clone()),
                super::ImportedSymbol::Type(_) | super::ImportedSymbol::Function(_) => None,
            })
            .collect();
        for local_name in imported_constant_names {
            self.imported_constants.insert(local_name, Type::Unknown);
        }
    }

    pub(super) fn collect_type_declarations(&mut self, types: &[TypeDeclaration]) {
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
                TypeDeclarationKind::Struct { .. } => TypeKind::Struct { fields: Vec::new() },
                TypeDeclarationKind::Union { .. } => TypeKind::Union {
                    variants: Vec::new(),
                },
            };
            self.types
                .insert(type_declaration.name.clone(), TypeInfo { kind });
        }

        for type_declaration in types {
            match &type_declaration.kind {
                TypeDeclarationKind::Struct { fields, .. } => {
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
                TypeDeclarationKind::Union { variants } => {
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
        }
    }

    pub(super) fn collect_function_signatures(&mut self, functions: &[FunctionDeclaration]) {
        for function in functions {
            self.check_function_name(&function.name, &function.name_span);
            if self.functions.contains_key(&function.name) {
                self.error(
                    format!("duplicate function '{}'", function.name),
                    function.name_span.clone(),
                );
                continue;
            }

            let return_type = self.resolve_type_name(&function.return_type);

            let mut parameter_types = Vec::new();
            for parameter in &function.parameters {
                let value_type = self.resolve_type_name(&parameter.type_name);
                parameter_types.push(value_type);
            }

            self.functions.insert(
                function.name.clone(),
                FunctionInfo {
                    parameter_types,
                    return_type,
                },
            );
        }
    }

    pub(super) fn collect_method_signatures(&mut self, types: &[TypeDeclaration]) {
        for type_declaration in types {
            let TypeDeclarationKind::Struct { methods, .. } = &type_declaration.kind else {
                continue;
            };

            for method in methods {
                self.check_function_name(&method.name, &method.name_span);
                let method_key = MethodKey {
                    receiver_type_name: type_declaration.name.clone(),
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
        }
    }

    pub(super) fn check_constant_declarations(&mut self, constants: &[ConstantDeclaration]) {
        for constant in constants {
            self.check_constant_name(&constant.name, &constant.span);
            let value_type = self.check_expression(&constant.expression);
            if self.constants.contains_key(&constant.name) {
                self.error(
                    format!("duplicate constant '{name}'", name = constant.name),
                    constant.span.clone(),
                );
                continue;
            }
            self.constants
                .insert(constant.name.clone(), super::ConstantInfo { value_type });
        }
    }
}
