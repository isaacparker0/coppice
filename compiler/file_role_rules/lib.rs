use compiler__diagnostics::Diagnostic;
use compiler__source::{FileRole, Span};
use compiler__syntax::{Declaration, FunctionDeclaration, ParsedFile, TypeName, Visibility};

/// Run file-role policy checks.
///
/// Every check that requires knowledge of file role belongs here.
/// `type_checker` is reserved for role-agnostic type semantics only.
///
/// In particular, `main` entrypoint constraints stay in this pass:
/// - placement (`main` only in `.bin.coppice`)
/// - binary contract (exactly one `main`, no parameters, returns `nil`)
///
/// Keeping role-conditional rules in one owner pass avoids brittle cross-pass
/// suppression ("emit in one pass, silence in another") and keeps diagnostic
/// intent deterministic.
pub fn check_file(file: &ParsedFile, diagnostics: &mut Vec<Diagnostic>) {
    check_exports_declaration_roles(file, diagnostics);
    check_public_declaration_roles(file, diagnostics);
    check_main_function_roles(file, diagnostics);
}

fn check_exports_declaration_roles(file: &ParsedFile, diagnostics: &mut Vec<Diagnostic>) {
    for declaration in file.top_level_declarations() {
        if file.role == FileRole::PackageManifest && !matches!(declaration, Declaration::Exports(_))
        {
            if is_main_function_declaration(declaration) {
                // `main` has a dedicated role diagnostic.
                continue;
            }
            diagnostics.push(Diagnostic::new(
                "PACKAGE.coppice may only contain exports declarations",
                declaration_span(declaration).clone(),
            ));
            continue;
        }

        if file.role != FileRole::PackageManifest && matches!(declaration, Declaration::Exports(_))
        {
            diagnostics.push(Diagnostic::new(
                "exports declarations are only allowed in PACKAGE.coppice",
                declaration_span(declaration).clone(),
            ));
        }
    }
}

fn is_main_function_declaration(declaration: &Declaration) -> bool {
    matches!(
        declaration,
        Declaration::Function(function_declaration) if function_declaration.name == "main"
    )
}

fn check_public_declaration_roles(file: &ParsedFile, diagnostics: &mut Vec<Diagnostic>) {
    if file.role != FileRole::BinaryEntrypoint && file.role != FileRole::Test {
        return;
    }
    let message = match file.role {
        FileRole::BinaryEntrypoint => "public declarations are not allowed in .bin.coppice files",
        FileRole::Test => "public declarations are not allowed in .test.coppice files",
        FileRole::Library | FileRole::PackageManifest => {
            unreachable!("public declaration role checks are only run for binary or test files")
        }
    };

    for declaration in file.top_level_declarations() {
        match declaration {
            Declaration::Type(type_declaration) => {
                if type_declaration.visibility == Visibility::Public {
                    diagnostics.push(Diagnostic::new(message, type_declaration.span.clone()));
                }
            }
            Declaration::Constant(constant_declaration)
                if constant_declaration.visibility == Visibility::Public =>
            {
                diagnostics.push(Diagnostic::new(message, constant_declaration.span.clone()));
            }
            Declaration::Function(function_declaration)
                if function_declaration.visibility == Visibility::Public =>
            {
                diagnostics.push(Diagnostic::new(message, function_declaration.span.clone()));
            }
            _ => {}
        }
    }
}

fn check_main_function_roles(file: &ParsedFile, diagnostics: &mut Vec<Diagnostic>) {
    let main_functions: Vec<&FunctionDeclaration> = file
        .top_level_declarations()
        .filter_map(|declaration| match declaration {
            Declaration::Function(function) if function.name == "main" => Some(function),
            _ => None,
        })
        .collect();

    match file.role {
        FileRole::BinaryEntrypoint => {
            if main_functions.is_empty() {
                diagnostics.push(Diagnostic::new(
                    ".bin.coppice files must declare exactly one main function",
                    fallback_file_span(file),
                ));
                return;
            }
            if main_functions.len() > 1 {
                for function in main_functions {
                    diagnostics.push(Diagnostic::new(
                        ".bin.coppice files must declare exactly one main function",
                        function.name_span.clone(),
                    ));
                }
                return;
            }
            check_binary_main_signature(main_functions[0], diagnostics);
        }
        FileRole::Library | FileRole::Test | FileRole::PackageManifest => {
            for function in main_functions {
                diagnostics.push(Diagnostic::new(
                    "main is only allowed in .bin.coppice files",
                    function.name_span.clone(),
                ));
            }
        }
    }
}

fn check_binary_main_signature(
    main_function: &FunctionDeclaration,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !main_function.parameters.is_empty() {
        diagnostics.push(Diagnostic::new(
            "main in .bin.coppice must not declare parameters",
            main_function.name_span.clone(),
        ));
    }
    if !is_nil_type(&main_function.return_type) {
        diagnostics.push(Diagnostic::new(
            "main in .bin.coppice must return nil",
            main_function.return_type.span.clone(),
        ));
    }
}

fn is_nil_type(type_name: &TypeName) -> bool {
    type_name.names.len() == 1 && type_name.names[0].name == "nil"
}

fn fallback_file_span(file: &ParsedFile) -> Span {
    if let Some(declaration) = file.top_level_declarations().next() {
        return declaration_span(declaration).clone();
    }

    Span {
        start: 0,
        end: 0,
        line: 1,
        column: 1,
    }
}

fn declaration_span(declaration: &Declaration) -> &Span {
    match declaration {
        Declaration::Import(import_declaration) => &import_declaration.span,
        Declaration::Exports(exports_declaration) => &exports_declaration.span,
        Declaration::Type(type_declaration) => &type_declaration.span,
        Declaration::Constant(constant_declaration) => &constant_declaration.span,
        Declaration::Function(function_declaration) => &function_declaration.span,
    }
}
