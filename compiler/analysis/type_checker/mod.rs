use std::collections::HashMap;

use compiler__diagnostics::Diagnostic;
use compiler__source::Span;
use compiler__syntax::{Expression, File, Statement, TypeName};

use crate::types::{Type, type_from_name};

mod assignability;
mod declarations;
mod expressions;
mod naming_rules;
mod statements;
mod type_narrowing;
mod unused_bindings;

#[must_use]
pub fn check_file(file: &File) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut type_checker = TypeChecker::new(&mut diagnostics);
    type_checker.collect_type_declarations(&file.types);
    type_checker.collect_function_signatures(&file.functions);
    type_checker.collect_method_signatures(&file.types);
    type_checker.check_constant_declarations(&file.constants);
    for function in &file.functions {
        type_checker.check_function(function);
    }
    type_checker.check_methods(&file.types);
    diagnostics
}

pub(super) struct VariableInfo {
    pub(super) value_type: Type,
    pub(super) used: bool,
    pub(super) mutable: bool,
    pub(super) span: Span,
}

pub(super) struct ConstantInfo {
    pub(super) value_type: Type,
}

pub(super) struct TypeInfo {
    pub(super) kind: TypeKind,
}

pub(super) enum TypeKind {
    Struct { fields: Vec<(String, Type)> },
    Union { variants: Vec<Type> },
}

pub(super) struct FunctionInfo {
    pub(super) parameter_types: Vec<Type>,
    pub(super) return_type: Type,
}

pub(super) struct MethodInfo {
    pub(super) self_mutable: bool,
    pub(super) parameter_types: Vec<Type>,
    pub(super) return_type: Type,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct MethodKey {
    pub(super) receiver_type_name: String,
    pub(super) method_name: String,
}

pub(super) struct TypeChecker<'a> {
    pub(super) constants: HashMap<String, ConstantInfo>,
    pub(super) types: HashMap<String, TypeInfo>,
    pub(super) functions: HashMap<String, FunctionInfo>,
    pub(super) methods: HashMap<MethodKey, MethodInfo>,
    pub(super) scopes: Vec<HashMap<String, VariableInfo>>,
    pub(super) diagnostics: &'a mut Vec<Diagnostic>,
    pub(super) current_return_type: Type,
    pub(super) loop_depth: usize,
}

pub(super) struct BranchNarrowing {
    pub(super) name: String,
    pub(super) when_true: Type,
    pub(super) when_false: Type,
}

pub(super) struct StatementOutcome {
    pub(super) terminates: bool,
    pub(super) fallthrough_narrowing: Option<FallthroughNarrowing>,
}

pub(super) struct FallthroughNarrowing {
    pub(super) variable_name: String,
    pub(super) narrowed_type: Type,
}

pub(super) trait ExpressionSpan {
    fn span(&self) -> Span;
}

pub(super) trait StatementSpan {
    fn span(&self) -> Span;
}

impl<'a> TypeChecker<'a> {
    pub(super) fn new(diagnostics: &'a mut Vec<Diagnostic>) -> Self {
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

    pub(super) fn define_variable(
        &mut self,
        name: String,
        value_type: Type,
        mutable: bool,
        span: Span,
    ) {
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

    pub(super) fn resolve_variable(&mut self, name: &str, span: &Span) -> Type {
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

    pub(super) fn lookup_variable_for_assignment(&mut self, name: &str) -> Option<(bool, Type)> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(info) = scope.get_mut(name) {
                info.used = true;
                return Some((info.mutable, info.value_type.clone()));
            }
        }
        None
    }

    pub(super) fn error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics.push(Diagnostic::new(message, span));
    }

    pub(super) fn resolve_type_name(&mut self, type_name: &TypeName) -> Type {
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
