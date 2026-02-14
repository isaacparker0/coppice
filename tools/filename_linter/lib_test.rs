use tools__filename_linter::{is_lower_or_upper_snake_case, is_lower_snake_case};

mod is_lower_snake_case_tests {
    use super::*;

    #[test]
    fn valid() {
        assert!(is_lower_snake_case("foo"));
        assert!(is_lower_snake_case("foo_bar"));
        assert!(is_lower_snake_case("foo_bar_baz"));
        assert!(is_lower_snake_case("foo123"));
        assert!(is_lower_snake_case("foo_123"));
        assert!(is_lower_snake_case("123"));
    }

    #[test]
    fn valid_edge_cases() {
        assert!(is_lower_snake_case(""));
        assert!(is_lower_snake_case("_"));
        assert!(is_lower_snake_case("__"));
        assert!(is_lower_snake_case("_foo"));
        assert!(is_lower_snake_case("foo_"));
    }

    #[test]
    fn invalid_hyphenated() {
        assert!(!is_lower_snake_case("foo-bar"));
        assert!(!is_lower_snake_case("foo-bar-baz"));
    }

    #[test]
    fn invalid_uppercase() {
        assert!(!is_lower_snake_case("FOO"));
        assert!(!is_lower_snake_case("FOO_BAR"));
        assert!(!is_lower_snake_case("Foo"));
        assert!(!is_lower_snake_case("fooBar"));
    }
}

mod is_lower_or_upper_snake_case_tests {
    use super::*;

    #[test]
    fn valid_lower() {
        assert!(is_lower_or_upper_snake_case("foo"));
        assert!(is_lower_or_upper_snake_case("foo_bar"));
        assert!(is_lower_or_upper_snake_case("foo_bar_baz"));
    }

    #[test]
    fn valid_upper() {
        assert!(is_lower_or_upper_snake_case("FOO"));
        assert!(is_lower_or_upper_snake_case("FOO_BAR"));
        assert!(is_lower_or_upper_snake_case("README"));
        assert!(is_lower_or_upper_snake_case("BUILD"));
    }

    #[test]
    fn valid_with_numbers() {
        assert!(is_lower_or_upper_snake_case("foo_123"));
        assert!(is_lower_or_upper_snake_case("foo123"));
        assert!(is_lower_or_upper_snake_case("FOO_123"));
        assert!(is_lower_or_upper_snake_case("123"));
    }

    #[test]
    fn valid_edge_cases() {
        assert!(is_lower_or_upper_snake_case(""));
        assert!(is_lower_or_upper_snake_case("_"));
        assert!(is_lower_or_upper_snake_case("__"));
    }

    #[test]
    fn invalid_hyphenated() {
        assert!(!is_lower_or_upper_snake_case("foo-bar"));
        assert!(!is_lower_or_upper_snake_case("FOO-BAR"));
    }

    #[test]
    fn invalid_mixed_case() {
        assert!(!is_lower_or_upper_snake_case("Foo"));
        assert!(!is_lower_or_upper_snake_case("fooBar"));
        assert!(!is_lower_or_upper_snake_case("FooBar"));
        assert!(!is_lower_or_upper_snake_case("Foo_bar"));
        assert!(!is_lower_or_upper_snake_case("foo_Bar"));
    }
}
