use compiler__diagnostics::PhaseDiagnostic;
use compiler__phase_results::{PhaseOutput, PhaseStatus};
use compiler__source::{FileRole, Span};
use compiler__syntax::{
    SyntaxDeclaration, SyntaxFunctionDeclaration, SyntaxParsedFile, SyntaxTopLevelVisibility,
    SyntaxTypeName,
};

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
#[must_use]
pub fn check_file(file: &SyntaxParsedFile) -> PhaseOutput<()> {
    let mut diagnostics = Vec::new();
    check_exports_declaration_roles(file, &mut diagnostics);
    check_visible_declaration_roles(file, &mut diagnostics);
    check_main_function_roles(file, &mut diagnostics);

    let status = if diagnostics.is_empty() {
        PhaseStatus::Ok
    } else {
        PhaseStatus::PreventsDownstreamExecution
    };

    PhaseOutput {
        value: (),
        diagnostics,
        status,
    }
}

fn check_exports_declaration_roles(
    file: &SyntaxParsedFile,
    diagnostics: &mut Vec<PhaseDiagnostic>,
) {
    for declaration in file.top_level_declarations() {
        if file.role == FileRole::PackageManifest
            && !matches!(declaration, SyntaxDeclaration::Exports(_))
        {
            if is_main_function_declaration(declaration) {
                // `main` has a dedicated role diagnostic.
                continue;
            }
            diagnostics.push(PhaseDiagnostic::new(
                "PACKAGE.coppice may only contain exports declarations",
                declaration_span(declaration).clone(),
            ));
            continue;
        }

        if file.role != FileRole::PackageManifest
            && matches!(declaration, SyntaxDeclaration::Exports(_))
        {
            diagnostics.push(PhaseDiagnostic::new(
                "exports declarations are only allowed in PACKAGE.coppice",
                declaration_span(declaration).clone(),
            ));
        }
    }
}

fn is_main_function_declaration(declaration: &SyntaxDeclaration) -> bool {
    matches!(
        declaration,
        SyntaxDeclaration::Function(function_declaration) if function_declaration.name == "main"
    )
}

fn check_visible_declaration_roles(
    file: &SyntaxParsedFile,
    diagnostics: &mut Vec<PhaseDiagnostic>,
) {
    if file.role != FileRole::BinaryEntrypoint && file.role != FileRole::Test {
        return;
    }
    let message = match file.role {
        FileRole::BinaryEntrypoint => "visible declarations are not allowed in .bin.coppice files",
        FileRole::Test => "visible declarations are not allowed in .test.coppice files",
        FileRole::Library | FileRole::PackageManifest => {
            unreachable!("visible declaration role checks are only run for binary or test files")
        }
    };

    for declaration in file.top_level_declarations() {
        match declaration {
            SyntaxDeclaration::Type(type_declaration) => {
                if type_declaration.visibility == SyntaxTopLevelVisibility::Visible {
                    diagnostics.push(PhaseDiagnostic::new(message, type_declaration.span.clone()));
                }
            }
            SyntaxDeclaration::Constant(constant_declaration)
                if constant_declaration.visibility == SyntaxTopLevelVisibility::Visible =>
            {
                diagnostics.push(PhaseDiagnostic::new(
                    message,
                    constant_declaration.span.clone(),
                ));
            }
            SyntaxDeclaration::Function(function_declaration)
                if function_declaration.visibility == SyntaxTopLevelVisibility::Visible =>
            {
                diagnostics.push(PhaseDiagnostic::new(
                    message,
                    function_declaration.span.clone(),
                ));
            }
            _ => {}
        }
    }
}

fn check_main_function_roles(file: &SyntaxParsedFile, diagnostics: &mut Vec<PhaseDiagnostic>) {
    let main_functions: Vec<&SyntaxFunctionDeclaration> = file
        .top_level_declarations()
        .filter_map(|declaration| match declaration {
            SyntaxDeclaration::Function(function) if function.name == "main" => Some(function),
            _ => None,
        })
        .collect();

    match file.role {
        FileRole::BinaryEntrypoint => {
            if main_functions.is_empty() {
                diagnostics.push(PhaseDiagnostic::new(
                    ".bin.coppice files must declare exactly one main function",
                    fallback_file_span(file),
                ));
                return;
            }
            if main_functions.len() > 1 {
                for function in main_functions {
                    diagnostics.push(PhaseDiagnostic::new(
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
                diagnostics.push(PhaseDiagnostic::new(
                    "main is only allowed in .bin.coppice files",
                    function.name_span.clone(),
                ));
            }
        }
    }
}

fn check_binary_main_signature(
    main_function_declaration: &SyntaxFunctionDeclaration,
    diagnostics: &mut Vec<PhaseDiagnostic>,
) {
    if !main_function_declaration.parameters.is_empty() {
        diagnostics.push(PhaseDiagnostic::new(
            "main in .bin.coppice must not declare parameters",
            main_function_declaration.name_span.clone(),
        ));
    }
    if !is_nil_type(&main_function_declaration.return_type) {
        diagnostics.push(PhaseDiagnostic::new(
            "main in .bin.coppice must return nil",
            main_function_declaration.return_type.span.clone(),
        ));
    }
}

fn is_nil_type(type_name: &SyntaxTypeName) -> bool {
    type_name.names.len() == 1 && type_name.names[0].name == "nil"
}

fn fallback_file_span(file: &SyntaxParsedFile) -> Span {
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

fn declaration_span(declaration: &SyntaxDeclaration) -> &Span {
    match declaration {
        SyntaxDeclaration::Import(import_declaration) => &import_declaration.span,
        SyntaxDeclaration::Exports(exports_declaration) => &exports_declaration.span,
        SyntaxDeclaration::Type(type_declaration) => &type_declaration.span,
        SyntaxDeclaration::Constant(constant_declaration) => &constant_declaration.span,
        SyntaxDeclaration::Function(function_declaration) => &function_declaration.span,
    }
}
