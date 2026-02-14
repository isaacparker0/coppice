use tools__gazelle_rust__rust_parser::parser::parse_source;

#[test]
fn test_simple_import() {
    let result = parse_source("use serde::Serialize;").unwrap();
    assert_eq!(result.imports, vec!["serde"]);
    assert!(result.external_modules.is_empty());
    assert!(!result.has_main);
}

#[test]
fn test_multiple_imports() {
    let code = r"
        use serde::{Serialize, Deserialize};
        use tokio::runtime::Runtime;
    ";
    let result = parse_source(code).unwrap();
    assert_eq!(result.imports, vec!["serde", "tokio"]);
    assert!(result.external_modules.is_empty());
    assert!(!result.has_main);
}

#[test]
fn test_external_modules() {
    let code = r"
        mod foo;
        mod bar;
    ";
    let result = parse_source(code).unwrap();
    assert!(result.imports.is_empty());
    assert_eq!(result.external_modules, vec!["foo", "bar"]);
    assert!(!result.has_main);
}

#[test]
fn test_inline_mod_not_external() {
    let code = r"
        mod foo {
            pub fn bar() {}
        }
    ";
    let result = parse_source(code).unwrap();
    assert!(result.imports.is_empty());
    assert!(result.external_modules.is_empty());
    assert!(!result.has_main);
}

#[test]
fn test_nested_mod_declaration_not_external() {
    let code = r"
        mod outer {
            mod inner;
        }
    ";
    let result = parse_source(code).unwrap();
    assert!(result.imports.is_empty());
    assert!(result.external_modules.is_empty());
    assert!(!result.has_main);
}

#[test]
fn test_has_main() {
    let code = r#"
        fn main() {
            println!("hello");
        }
    "#;
    let result = parse_source(code).unwrap();
    assert!(result.imports.is_empty());
    assert!(result.external_modules.is_empty());
    assert!(result.has_main);
}

#[test]
fn test_main_in_module_not_detected() {
    let code = r"
        mod inner {
            fn main() {}
        }
    ";
    let result = parse_source(code).unwrap();
    assert!(result.imports.is_empty());
    assert!(result.external_modules.is_empty());
    assert!(!result.has_main);
}

#[test]
fn test_derive_import() {
    let code = r"
        #[derive(serde::Serialize)]
        struct Foo {}
    ";
    let result = parse_source(code).unwrap();
    assert_eq!(result.imports, vec!["serde"]);
    assert!(result.external_modules.is_empty());
    assert!(!result.has_main);
}

#[test]
fn test_std_imported() {
    let code = r"
        use std::collections::HashMap;
    ";
    let result = parse_source(code).unwrap();
    assert_eq!(result.imports, vec!["std"]);
    assert!(result.external_modules.is_empty());
    assert!(!result.has_main);
}

#[test]
fn test_crate_super_self_ignored() {
    let code = r"
        use crate::foo::Bar;
        use super::baz::Qux;
        use self::inner::Thing;
    ";
    let result = parse_source(code).unwrap();
    assert!(result.imports.is_empty());
    assert!(result.external_modules.is_empty());
    assert!(!result.has_main);
}

#[test]
fn test_primitive_types_not_imported() {
    let code = r"
        fn foo() {
            let x = u32::from_le_bytes([0, 0, 0, 1]);
            let y = i64::MAX;
            let z = str::from_utf8(&[]).unwrap();
        }
    ";
    let result = parse_source(code).unwrap();
    assert!(result.imports.is_empty());
    assert!(result.external_modules.is_empty());
    assert!(!result.has_main);
}

#[test]
fn test_primitive_in_type_annotation_not_imported() {
    let code = r"
        fn foo(x: u32) -> i64 {
            x as i64
        }
    ";
    let result = parse_source(code).unwrap();
    assert!(result.imports.is_empty());
    assert!(result.external_modules.is_empty());
    assert!(!result.has_main);
}

#[test]
fn test_qualified_function_call_import() {
    let code = r#"
        fn main() {
            let encoded = urlencoding::encode("hello world");
        }
    "#;
    let result = parse_source(code).unwrap();
    assert_eq!(result.imports, vec!["urlencoding"]);
    assert!(result.external_modules.is_empty());
    assert!(result.has_main);
}

#[test]
fn test_qualified_call_in_format_macro() {
    let code = r#"
        fn main() {
            let url = format!(
                "https://example.com?q={}",
                urlencoding::encode("hello world")
            );
        }
    "#;
    let result = parse_source(code).unwrap();
    assert_eq!(result.imports, vec!["urlencoding"]);
    assert!(result.external_modules.is_empty());
    assert!(result.has_main);
}
