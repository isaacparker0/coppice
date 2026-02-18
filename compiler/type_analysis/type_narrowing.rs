use compiler__semantic_program::{
    SemanticBinaryOperator, SemanticBlock, SemanticExpression, SemanticMatchArm, SemanticSymbolKind,
};

use compiler__semantic_types::Type;

use super::{BranchNarrowing, TypeChecker};

impl TypeChecker<'_> {
    pub(super) fn check_block_with_type_narrowing(
        &mut self,
        block: &SemanticBlock,
        type_narrowing: Option<&BranchNarrowing>,
        use_true_branch: bool,
    ) -> bool {
        let restore = type_narrowing.and_then(|type_narrowing| {
            let narrowed_type = if use_true_branch {
                type_narrowing.when_true.clone()
            } else {
                type_narrowing.when_false.clone()
            };
            self.apply_variable_narrowing(&type_narrowing.name, narrowed_type)
        });

        let result = self.check_block(block);

        if let Some((scope_index, name, original_type)) = restore {
            self.restore_variable_type(scope_index, &name, original_type);
        }

        result
    }

    pub(super) fn apply_variable_narrowing(
        &mut self,
        name: &str,
        narrowed_type: Type,
    ) -> Option<(usize, String, Type)> {
        if narrowed_type == Type::Unknown {
            return None;
        }
        for (scope_index, scope) in self.scopes.iter_mut().enumerate().rev() {
            if let Some(info) = scope.get_mut(name) {
                let original_type = info.value_type.clone();
                info.value_type = narrowed_type;
                return Some((scope_index, name.to_string(), original_type));
            }
        }
        None
    }

    pub(super) fn restore_variable_type(
        &mut self,
        scope_index: usize,
        name: &str,
        original_type: Type,
    ) {
        if let Some(scope) = self.scopes.get_mut(scope_index)
            && let Some(info) = scope.get_mut(name)
        {
            info.value_type = original_type;
        }
    }

    pub(super) fn derive_condition_type_narrowing(
        &mut self,
        condition: &SemanticExpression,
    ) -> Option<BranchNarrowing> {
        if let SemanticExpression::Binary {
            operator,
            left,
            right,
            ..
        } = condition
        {
            if *operator != SemanticBinaryOperator::EqualEqual
                && *operator != SemanticBinaryOperator::NotEqual
            {
                return None;
            }

            let (name, is_nil_test) = if let SemanticExpression::Symbol {
                name,
                kind: SemanticSymbolKind::UserDefined,
                ..
            } = left.as_ref()
            {
                (
                    name,
                    matches!(right.as_ref(), SemanticExpression::NilLiteral { .. }),
                )
            } else if let SemanticExpression::Symbol {
                name,
                kind: SemanticSymbolKind::UserDefined,
                ..
            } = right.as_ref()
            {
                (
                    name,
                    matches!(left.as_ref(), SemanticExpression::NilLiteral { .. }),
                )
            } else {
                return None;
            };

            if !is_nil_test {
                return None;
            }

            let variable_type = self.lookup_variable_type(name)?;
            let non_nil_type = Self::without_type_member(&variable_type, &Type::Nil);

            let (when_true, when_false) = match *operator {
                SemanticBinaryOperator::EqualEqual => (Type::Nil, non_nil_type),
                SemanticBinaryOperator::NotEqual => (non_nil_type, Type::Nil),
                _ => return None,
            };
            return Some(BranchNarrowing {
                name: name.clone(),
                when_true,
                when_false,
            });
        }

        if let SemanticExpression::Matches {
            value,
            type_name,
            span: _,
        } = condition
        {
            let SemanticExpression::Symbol {
                name,
                kind: SemanticSymbolKind::UserDefined,
                ..
            } = value.as_ref()
            else {
                return None;
            };
            let pattern_type = self.resolve_match_pattern_type_name(type_name, &type_name.span);
            if pattern_type == Type::Unknown {
                return None;
            }
            let variable_type = self.lookup_variable_type(name)?;
            if let Type::Union(variants) = &variable_type
                && variants.contains(&pattern_type)
            {
                return Some(BranchNarrowing {
                    name: name.clone(),
                    when_true: pattern_type.clone(),
                    when_false: Self::without_type_member(&variable_type, &pattern_type),
                });
            }
            if variable_type == pattern_type {
                return Some(BranchNarrowing {
                    name: name.clone(),
                    when_true: pattern_type,
                    when_false: Type::Unknown,
                });
            }
        }

        None
    }

    pub(super) fn is_boolean_membership_match(arms: &[SemanticMatchArm]) -> bool {
        let mut true_count = 0usize;
        let mut false_count = 0usize;
        for arm in arms {
            match &arm.value {
                SemanticExpression::BooleanLiteral { value: true, .. } => true_count += 1,
                SemanticExpression::BooleanLiteral { value: false, .. } => false_count += 1,
                _ => return false,
            }
        }
        true_count == 1 && false_count >= 1
    }

    pub(super) fn lookup_variable_type(&self, name: &str) -> Option<Type> {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info.value_type.clone());
            }
        }
        None
    }

    pub(super) fn without_type_member(value_type: &Type, removed_member: &Type) -> Type {
        match value_type {
            Type::Union(members) => {
                let filtered = members
                    .iter()
                    .filter(|member| *member != removed_member)
                    .cloned()
                    .collect::<Vec<_>>();
                if filtered.is_empty() {
                    Type::Unknown
                } else {
                    Self::normalize_union(filtered)
                }
            }
            _ => {
                if value_type == removed_member {
                    Type::Unknown
                } else {
                    value_type.clone()
                }
            }
        }
    }
}
