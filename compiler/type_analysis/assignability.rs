use std::collections::HashSet;

use compiler__semantic_types::Type;

use super::TypeChecker;

impl TypeChecker<'_> {
    pub(super) fn is_assignable(&self, value_type: &Type, expected_type: &Type) -> bool {
        if matches!(value_type, Type::Never) {
            return true;
        }
        match expected_type {
            Type::Unknown => true,
            Type::Never => matches!(value_type, Type::Unknown | Type::Never),
            Type::Union(members) => match value_type {
                Type::Unknown => true,
                Type::Union(value_members) => value_members
                    .iter()
                    .all(|value_member| self.is_assignable(value_member, expected_type)),
                _ => members
                    .iter()
                    .any(|member| self.is_assignable(value_type, member)),
            },
            _ => match value_type {
                Type::Unknown => true,
                Type::Union(value_members) => value_members
                    .iter()
                    .all(|value_member| self.is_assignable(value_member, expected_type)),
                _ => {
                    if value_type == expected_type {
                        return true;
                    }
                    self.type_implements_expected_interface(value_type, expected_type)
                }
            },
        }
    }

    pub(super) fn are_comparable_for_equality(&self, left_type: &Type, right_type: &Type) -> bool {
        self.is_assignable(left_type, right_type) || self.is_assignable(right_type, left_type)
    }

    fn type_implements_expected_interface(&self, value_type: &Type, expected_type: &Type) -> bool {
        let Some(expected_nominal_type_id) = Self::nominal_type_id_for_type(expected_type) else {
            return false;
        };
        let Some(expected_type_info) = self.type_info_by_nominal_type_id(&expected_nominal_type_id)
        else {
            return false;
        };
        if !matches!(expected_type_info.kind, super::TypeKind::Interface { .. }) {
            return false;
        }

        let Some(value_nominal_type_id) = Self::nominal_type_id_for_type(value_type) else {
            return false;
        };
        let Some(value_type_info) = self.type_info_by_nominal_type_id(&value_nominal_type_id)
        else {
            return false;
        };
        value_type_info
            .implemented_interface_entries
            .iter()
            .any(|implemented_interface| implemented_interface.resolved_type == *expected_type)
    }

    pub(super) fn normalize_union(types: Vec<Type>) -> Type {
        let mut flat = Vec::new();
        let mut seen = HashSet::new();
        for value_type in types {
            if let Type::Union(inner) = value_type {
                for inner_type in inner {
                    if matches!(inner_type, Type::Never) {
                        continue;
                    }
                    let key = inner_type.display();
                    if seen.insert(key) {
                        flat.push(inner_type);
                    }
                }
            } else {
                if matches!(value_type, Type::Never) {
                    continue;
                }
                let key = value_type.display();
                if seen.insert(key) {
                    flat.push(value_type);
                }
            }
        }
        if flat.is_empty() {
            return Type::Never;
        }
        if flat.len() == 1 {
            flat.remove(0)
        } else {
            Type::Union(flat)
        }
    }
}
