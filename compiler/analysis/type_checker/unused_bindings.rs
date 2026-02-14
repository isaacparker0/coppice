use super::TypeChecker;

impl TypeChecker<'_> {
    pub(super) fn check_unused_in_current_scope(&mut self) {
        if let Some(scope) = self.scopes.last() {
            let mut unused = Vec::new();
            let mut used_with_ignored_prefix = Vec::new();
            for (name, info) in scope {
                if info.used && name.starts_with('_') {
                    used_with_ignored_prefix.push((name.clone(), info.span.clone()));
                    continue;
                }
                if info.used || name.starts_with('_') {
                    continue;
                }
                unused.push((name.clone(), info.span.clone()));
            }
            for (name, span) in used_with_ignored_prefix {
                self.error(
                    format!("bindings prefixed with '_' must be unused: '{name}' is used"),
                    span,
                );
            }
            for (name, span) in unused {
                self.error(format!("unused variable '{name}'"), span);
            }
        }
    }
}
