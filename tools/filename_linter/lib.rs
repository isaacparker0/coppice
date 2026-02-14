#[must_use]
pub fn is_lower_snake_case(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    if value.contains('-') {
        return false;
    }
    value
        .chars()
        .all(|c| c.is_lowercase() || c == '_' || c.is_numeric())
}

#[must_use]
pub fn is_lower_or_upper_snake_case(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    if value.contains('-') {
        return false;
    }
    let is_all_upper = value
        .chars()
        .all(|c| c.is_uppercase() || c == '_' || c.is_numeric());
    let is_all_lower = value
        .chars()
        .all(|c| c.is_lowercase() || c == '_' || c.is_numeric());
    is_all_upper || is_all_lower
}
