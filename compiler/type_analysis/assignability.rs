use std::collections::HashSet;

use compiler__semantic_types::Type;

use super::TypeChecker;

impl TypeChecker<'_> {
    pub(super) fn is_assignable(value_type: &Type, expected_type: &Type) -> bool {
        match expected_type {
            Type::Unknown => true,
            Type::Union(members) => match value_type {
                Type::Unknown => true,
                Type::Union(value_members) => value_members
                    .iter()
                    .all(|value_member| members.contains(value_member)),
                _ => members.contains(value_type),
            },
            _ => match value_type {
                Type::Unknown => true,
                Type::Union(_) => false,
                _ => value_type == expected_type,
            },
        }
    }

    pub(super) fn are_comparable_for_equality(left_type: &Type, right_type: &Type) -> bool {
        Self::is_assignable(left_type, right_type) || Self::is_assignable(right_type, left_type)
    }

    pub(super) fn normalize_union(types: Vec<Type>) -> Type {
        let mut flat = Vec::new();
        let mut seen = HashSet::new();
        for value_type in types {
            if let Type::Union(inner) = value_type {
                for inner_type in inner {
                    let key = inner_type.display();
                    if seen.insert(key) {
                        flat.push(inner_type);
                    }
                }
            } else {
                let key = value_type.display();
                if seen.insert(key) {
                    flat.push(value_type);
                }
            }
        }
        if flat.len() == 1 {
            flat.remove(0)
        } else {
            Type::Union(flat)
        }
    }
}
