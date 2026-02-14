use super::TypeChecker;
use compiler__source::Span;

impl TypeChecker<'_> {
    pub(super) fn check_type_name(&mut self, name: &str, span: &Span) {
        if !is_pascal_case(name) {
            self.error("type name must be PascalCase", span.clone());
        }
    }

    pub(super) fn check_function_name(&mut self, name: &str, span: &Span) {
        if !is_camel_case(name) {
            self.error("function name must be camelCase", span.clone());
        }
    }

    pub(super) fn check_constant_name(&mut self, name: &str, span: &Span) {
        if !is_upper_snake_case(name) {
            self.error("constant name must be UPPER_SNAKE_CASE", span.clone());
        }
    }

    pub(super) fn check_variable_name(&mut self, name: &str, span: &Span) {
        if !is_camel_case_with_optional_leading_underscore(name) {
            self.error("variable name must be camelCase", span.clone());
        }
    }

    pub(super) fn check_parameter_name(&mut self, name: &str, span: &Span) {
        if !is_camel_case_with_optional_leading_underscore(name) {
            self.error("parameter name must be camelCase", span.clone());
        }
    }
}

fn is_pascal_case(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_uppercase() {
        return false;
    }
    if !is_alphanumeric_no_underscore(first) {
        return false;
    }
    let mut previous_upper = true;
    for ch in chars {
        if !is_alphanumeric_no_underscore(ch) {
            return false;
        }
        let is_upper = ch.is_ascii_uppercase();
        if previous_upper && is_upper {
            return false;
        }
        previous_upper = is_upper;
    }
    true
}

fn is_camel_case(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    if !is_alphanumeric_no_underscore(first) {
        return false;
    }
    let mut previous_upper = false;
    for ch in chars {
        if !is_alphanumeric_no_underscore(ch) {
            return false;
        }
        let is_upper = ch.is_ascii_uppercase();
        if previous_upper && is_upper {
            return false;
        }
        previous_upper = is_upper;
    }
    true
}

fn is_upper_snake_case(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_uppercase() {
        return false;
    }
    for ch in chars {
        if !(ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_') {
            return false;
        }
    }
    true
}

fn is_camel_case_with_optional_leading_underscore(name: &str) -> bool {
    if name.starts_with("__") {
        return false;
    }
    if let Some(rest) = name.strip_prefix('_') {
        if rest.is_empty() {
            return true;
        }
        return is_camel_case(rest);
    }
    is_camel_case(name)
}

fn is_alphanumeric_no_underscore(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
}
