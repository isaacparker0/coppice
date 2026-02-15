use std::collections::HashMap;

use compiler__diagnostics::Diagnostic;
use compiler__source::Span;
use compiler__syntax::{
    ConstantDeclaration, Declaration, Expression, FunctionDeclaration, ParsedFile, Statement,
    TypeDeclaration, TypeName,
};

use crate::types::{Type, type_from_name};

mod assignability;
mod declarations;
mod expressions;
mod naming_rules;
mod statements;
mod type_narrowing;
mod types;
mod unused_bindings;

pub fn check_file(file: &ParsedFile, diagnostics: &mut Vec<Diagnostic>) {
    check_parsed_file(file, diagnostics);
}

fn check_parsed_file(file: &ParsedFile, diagnostics: &mut Vec<Diagnostic>) {
    let mut type_declarations = Vec::new();
    let mut constant_declarations = Vec::new();
    let mut function_declarations = Vec::new();
    for declaration in &file.declarations {
        match declaration {
            Declaration::Type(type_declaration) => type_declarations.push(type_declaration.clone()),
            Declaration::Constant(constant_declaration) => {
                constant_declarations.push(constant_declaration.clone());
            }
            Declaration::Function(function_declaration) => {
                function_declarations.push(function_declaration.clone());
            }
            Declaration::Import(_) | Declaration::Exports(_) => {}
        }
    }
    check_declarations(
        diagnostics,
        &type_declarations,
        &constant_declarations,
        &function_declarations,
    );
}

fn check_declarations(
    diagnostics: &mut Vec<Diagnostic>,
    type_declarations: &[TypeDeclaration],
    constant_declarations: &[ConstantDeclaration],
    function_declarations: &[FunctionDeclaration],
) {
    let mut type_checker = TypeChecker::new(diagnostics);
    type_checker.collect_type_declarations(type_declarations);
    type_checker.collect_function_signatures(function_declarations);
    type_checker.collect_method_signatures(type_declarations);
    type_checker.check_constant_declarations(constant_declarations);
    for function in function_declarations {
        type_checker.check_function(function);
    }
    type_checker.check_methods(type_declarations);
}

struct VariableInfo {
    value_type: Type,
    used: bool,
    mutable: bool,
    span: Span,
}

struct ConstantInfo {
    value_type: Type,
}

struct TypeInfo {
    kind: TypeKind,
}

enum TypeKind {
    Struct { fields: Vec<(String, Type)> },
    Union { variants: Vec<Type> },
}

struct FunctionInfo {
    parameter_types: Vec<Type>,
    return_type: Type,
}

struct MethodInfo {
    self_mutable: bool,
    parameter_types: Vec<Type>,
    return_type: Type,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct MethodKey {
    receiver_type_name: String,
    method_name: String,
}

struct TypeChecker<'a> {
    constants: HashMap<String, ConstantInfo>,
    types: HashMap<String, TypeInfo>,
    functions: HashMap<String, FunctionInfo>,
    methods: HashMap<MethodKey, MethodInfo>,
    scopes: Vec<HashMap<String, VariableInfo>>,
    diagnostics: &'a mut Vec<Diagnostic>,
    current_return_type: Type,
    loop_depth: usize,
}

struct BranchNarrowing {
    name: String,
    when_true: Type,
    when_false: Type,
}

struct StatementOutcome {
    terminates: bool,
    fallthrough_narrowing: Option<FallthroughNarrowing>,
}

struct FallthroughNarrowing {
    variable_name: String,
    narrowed_type: Type,
}

trait ExpressionSpan {
    fn span(&self) -> Span;
}

trait StatementSpan {
    fn span(&self) -> Span;
}

impl<'a> TypeChecker<'a> {
    fn new(diagnostics: &'a mut Vec<Diagnostic>) -> Self {
        Self {
            constants: HashMap::new(),
            types: HashMap::new(),
            functions: HashMap::new(),
            methods: HashMap::new(),
            scopes: Vec::new(),
            diagnostics,
            current_return_type: Type::Unknown,
            loop_depth: 0,
        }
    }

    fn define_variable(&mut self, name: String, value_type: Type, mutable: bool, span: Span) {
        let duplicate = self
            .scopes
            .last()
            .is_some_and(|scope| scope.contains_key(&name));
        if duplicate {
            self.error(format!("duplicate binding '{name}'"), span.clone());
        }
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(
                name,
                VariableInfo {
                    value_type,
                    used: false,
                    mutable,
                    span,
                },
            );
        }
    }

    fn resolve_variable(&mut self, name: &str, span: &Span) -> Type {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(info) = scope.get_mut(name) {
                info.used = true;
                return info.value_type.clone();
            }
        }
        if let Some(info) = self.constants.get(name) {
            return info.value_type.clone();
        }
        self.error(format!("unknown name '{name}'"), span.clone());
        Type::Unknown
    }

    fn lookup_variable_for_assignment(&mut self, name: &str) -> Option<(bool, Type)> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(info) = scope.get_mut(name) {
                info.used = true;
                return Some((info.mutable, info.value_type.clone()));
            }
        }
        None
    }

    fn error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics.push(Diagnostic::new(message, span));
    }

    fn resolve_type_name(&mut self, type_name: &TypeName) -> Type {
        let mut resolved = Vec::new();
        let mut has_unknown = false;
        for atom in &type_name.names {
            let name = atom.name.as_str();
            if let Some(builtin) = type_from_name(name) {
                resolved.push(builtin);
                continue;
            }
            if let Some(info) = self.types.get(name) {
                match &info.kind {
                    TypeKind::Struct { .. } => resolved.push(Type::Named(name.to_string())),
                    TypeKind::Union { variants } => {
                        resolved.push(Self::normalize_union(variants.clone()));
                    }
                }
                continue;
            }
            self.error(format!("unknown type '{name}'"), atom.span.clone());
            has_unknown = true;
        }

        if has_unknown {
            return Type::Unknown;
        }

        if resolved.len() == 1 {
            return resolved.remove(0);
        }
        Self::normalize_union(resolved)
    }
}

impl ExpressionSpan for Expression {
    fn span(&self) -> Span {
        match self {
            Expression::IntegerLiteral { span, .. }
            | Expression::NilLiteral { span, .. }
            | Expression::BooleanLiteral { span, .. }
            | Expression::StringLiteral { span, .. }
            | Expression::Identifier { span, .. }
            | Expression::StructLiteral { span, .. }
            | Expression::FieldAccess { span, .. }
            | Expression::Call { span, .. }
            | Expression::Unary { span, .. }
            | Expression::Binary { span, .. }
            | Expression::Match { span, .. }
            | Expression::Matches { span, .. } => span.clone(),
        }
    }
}

impl StatementSpan for Statement {
    fn span(&self) -> Span {
        match self {
            Statement::Let { span, .. }
            | Statement::Assign { span, .. }
            | Statement::Return { span, .. }
            | Statement::Abort { span, .. }
            | Statement::If { span, .. }
            | Statement::For { span, .. }
            | Statement::Break { span, .. }
            | Statement::Continue { span, .. }
            | Statement::Expression { span, .. } => span.clone(),
        }
    }
}
