use std::collections::HashMap;

use compiler__diagnostics::Diagnostic;
use compiler__packages::PackageId;
use compiler__phase_results::{PhaseOutput, PhaseStatus};
use compiler__semantic_program::{
    ConstantDeclaration, Declaration, Expression, FunctionDeclaration,
    PackageUnit as SemanticPackageUnit, Statement, TypeDeclaration, TypeName,
};
use compiler__semantic_types::{
    FileTypecheckSummary, ImportedBinding, ImportedSymbol, ImportedTypeDeclaration, NominalTypeId,
    NominalTypeRef, Type, TypedFunctionSignature, TypedSymbol, type_from_builtin_name,
};
use compiler__source::Span;

mod assignability;
mod declarations;
mod expressions;
mod naming_rules;
mod statements;
mod type_narrowing;
mod unused_bindings;

#[must_use]
pub fn check_package_unit(
    package_id: PackageId,
    package_unit: &SemanticPackageUnit,
    imported_bindings: &[ImportedBinding],
) -> PhaseOutput<()> {
    let mut diagnostics = Vec::new();
    analyze_package_unit(
        package_id,
        package_unit,
        imported_bindings,
        &mut diagnostics,
    );
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

pub fn analyze_package_unit(
    package_id: PackageId,
    package_unit: &SemanticPackageUnit,
    imported_bindings: &[ImportedBinding],
    diagnostics: &mut Vec<Diagnostic>,
) -> FileTypecheckSummary {
    check_package_unit_declarations(package_id, package_unit, imported_bindings, diagnostics)
}

fn check_package_unit_declarations(
    package_id: PackageId,
    package_unit: &SemanticPackageUnit,
    imported_bindings: &[ImportedBinding],
    diagnostics: &mut Vec<Diagnostic>,
) -> FileTypecheckSummary {
    let mut type_declarations = Vec::new();
    let mut constant_declarations = Vec::new();
    let mut function_declarations = Vec::new();
    for declaration in &package_unit.declarations {
        match declaration {
            Declaration::Type(type_declaration) => type_declarations.push(type_declaration.clone()),
            Declaration::Constant(constant_declaration) => {
                constant_declarations.push(constant_declaration.clone());
            }
            Declaration::Function(function_declaration) => {
                function_declarations.push(function_declaration.clone());
            }
        }
    }

    check_declarations(
        package_id,
        diagnostics,
        &type_declarations,
        &constant_declarations,
        &function_declarations,
        imported_bindings,
    )
}

fn check_declarations(
    package_id: PackageId,
    diagnostics: &mut Vec<Diagnostic>,
    type_declarations: &[TypeDeclaration],
    constant_declarations: &[ConstantDeclaration],
    function_declarations: &[FunctionDeclaration],
    imported_bindings: &[ImportedBinding],
) -> FileTypecheckSummary {
    let mut type_checker = TypeChecker::new(package_id, imported_bindings, diagnostics);
    type_checker.collect_imported_type_declarations();
    type_checker.collect_type_declarations(type_declarations);
    type_checker.collect_imported_function_signatures();
    type_checker.collect_function_signatures(function_declarations);
    type_checker.collect_imported_method_signatures();
    type_checker.collect_method_signatures(type_declarations);
    type_checker.check_constant_declarations(constant_declarations);
    for function in function_declarations {
        type_checker.check_function(function);
    }
    type_checker.check_methods(type_declarations);
    type_checker.check_unused_imports();
    type_checker.build_summary(
        type_declarations,
        function_declarations,
        constant_declarations,
    )
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

struct ImportedBindingInfo {
    symbol: ImportedSymbol,
    span: Span,
    used: bool,
}

struct TypeInfo {
    nominal_type_id: NominalTypeId,
    type_parameters: Vec<String>,
    kind: TypeKind,
}

#[derive(Clone)]
enum TypeKind {
    Struct { fields: Vec<(String, Type)> },
    Union { variants: Vec<Type> },
}

#[derive(Clone)]
struct FunctionInfo {
    type_parameters: Vec<String>,
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
    receiver_type_id: NominalTypeId,
    method_name: String,
}

struct TypeChecker<'a> {
    package_id: PackageId,
    constants: HashMap<String, ConstantInfo>,
    types: HashMap<String, TypeInfo>,
    functions: HashMap<String, FunctionInfo>,
    imported_functions: HashMap<String, FunctionInfo>,
    imported_bindings: HashMap<String, ImportedBindingInfo>,
    methods: HashMap<MethodKey, MethodInfo>,
    scopes: Vec<HashMap<String, VariableInfo>>,
    type_parameter_scopes: Vec<HashMap<String, Span>>,
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
    fn new(
        package_id: PackageId,
        imported_bindings: &[ImportedBinding],
        diagnostics: &'a mut Vec<Diagnostic>,
    ) -> Self {
        let mut imported_binding_map = HashMap::new();
        for imported in imported_bindings {
            imported_binding_map.insert(
                imported.local_name.clone(),
                ImportedBindingInfo {
                    symbol: imported.symbol.clone(),
                    span: imported.span.clone(),
                    used: false,
                },
            );
        }
        Self {
            package_id,
            constants: HashMap::new(),
            types: HashMap::new(),
            functions: HashMap::new(),
            imported_functions: HashMap::new(),
            imported_bindings: imported_binding_map,
            methods: HashMap::new(),
            scopes: Vec::new(),
            type_parameter_scopes: Vec::new(),
            diagnostics,
            current_return_type: Type::Unknown,
            loop_depth: 0,
        }
    }

    fn build_summary(
        &self,
        type_declarations: &[TypeDeclaration],
        function_declarations: &[FunctionDeclaration],
        constant_declarations: &[ConstantDeclaration],
    ) -> FileTypecheckSummary {
        let mut typed_symbol_by_name = HashMap::new();

        for type_declaration in type_declarations {
            typed_symbol_by_name.insert(type_declaration.name.clone(), TypedSymbol::Type);
        }
        for function_declaration in function_declarations {
            if let Some(info) = self.functions.get(&function_declaration.name) {
                typed_symbol_by_name.insert(
                    function_declaration.name.clone(),
                    TypedSymbol::Function(TypedFunctionSignature {
                        type_parameters: info.type_parameters.clone(),
                        parameter_types: info.parameter_types.clone(),
                        return_type: info.return_type.clone(),
                    }),
                );
            }
        }
        for constant_declaration in constant_declarations {
            if let Some(info) = self.constants.get(&constant_declaration.name) {
                typed_symbol_by_name.insert(
                    constant_declaration.name.clone(),
                    TypedSymbol::Constant(info.value_type.clone()),
                );
            }
        }

        FileTypecheckSummary {
            typed_symbol_by_name,
        }
    }

    fn imported_constant_type(&self, name: &str) -> Option<Type> {
        let binding = self.imported_bindings.get(name)?;
        match &binding.symbol {
            ImportedSymbol::Constant(value_type) => Some(value_type.clone()),
            ImportedSymbol::Type(_) | ImportedSymbol::Function(_) => None,
        }
    }

    fn mark_import_used(&mut self, name: &str) {
        if let Some(binding) = self.imported_bindings.get_mut(name) {
            binding.used = true;
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
        if let Some(value_type) = self.imported_constant_type(name) {
            self.mark_import_used(name);
            return value_type;
        }
        if self.imported_bindings.contains_key(name) {
            self.mark_import_used(name);
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

    fn push_type_parameters(&mut self, names_and_spans: &[(String, Span)]) {
        let mut scope = HashMap::new();
        for (name, span) in names_and_spans {
            self.check_type_name(name, span);
            if scope.contains_key(name) {
                self.error(format!("duplicate type parameter '{name}'"), span.clone());
                continue;
            }
            scope.insert(name.clone(), span.clone());
        }
        self.type_parameter_scopes.push(scope);
    }

    fn pop_type_parameters(&mut self) {
        self.type_parameter_scopes.pop();
    }

    fn resolve_type_parameter(&self, name: &str) -> Option<Type> {
        for scope in self.type_parameter_scopes.iter().rev() {
            if scope.contains_key(name) {
                return Some(Type::TypeParameter(name.to_string()));
            }
        }
        None
    }

    fn instantiate_type(value_type: &Type, substitutions: &HashMap<String, Type>) -> Type {
        match value_type {
            Type::TypeParameter(name) => substitutions
                .get(name)
                .cloned()
                .unwrap_or_else(|| value_type.clone()),
            Type::Union(inner) => {
                let instantiated = inner
                    .iter()
                    .map(|inner_type| Self::instantiate_type(inner_type, substitutions))
                    .collect();
                Self::normalize_union(instantiated)
            }
            Type::Applied { base, arguments } => Type::Applied {
                base: base.clone(),
                arguments: arguments
                    .iter()
                    .map(|argument| Self::instantiate_type(argument, substitutions))
                    .collect(),
            },
            _ => value_type.clone(),
        }
    }

    fn resolve_type_name(&mut self, type_name: &TypeName) -> Type {
        let mut resolved = Vec::new();
        let mut has_unknown = false;
        for atom in &type_name.names {
            let name = atom.name.as_str();
            if let Some(type_parameter) = self.resolve_type_parameter(name) {
                if !atom.type_arguments.is_empty() {
                    self.error(
                        format!("type parameter '{name}' does not take type arguments"),
                        atom.span.clone(),
                    );
                    has_unknown = true;
                    continue;
                }
                resolved.push(type_parameter);
                continue;
            }
            if let Some(builtin) = type_from_builtin_name(name) {
                if !atom.type_arguments.is_empty() {
                    self.error(
                        format!("built-in type '{name}' does not take type arguments"),
                        atom.span.clone(),
                    );
                    has_unknown = true;
                    continue;
                }
                resolved.push(builtin);
                continue;
            }
            if let Some(info) = self.types.get(name) {
                let kind = info.kind.clone();
                let nominal_type_id = info.nominal_type_id.clone();
                let declared_type_parameters = info.type_parameters.clone();
                let type_parameter_count = declared_type_parameters.len();
                if matches!(
                    self.imported_bindings.get(name),
                    Some(ImportedBindingInfo {
                        symbol: ImportedSymbol::Type(_),
                        ..
                    })
                ) {
                    self.mark_import_used(name);
                }
                let resolved_type_arguments = atom
                    .type_arguments
                    .iter()
                    .map(|argument| self.resolve_type_name(argument))
                    .collect::<Vec<_>>();
                if atom.type_arguments.len() != type_parameter_count {
                    if type_parameter_count == 0 {
                        self.error(
                            format!("type '{name}' does not take type arguments"),
                            atom.span.clone(),
                        );
                    } else {
                        self.error(
                            format!(
                                "type '{name}' expects {type_parameter_count} type arguments, got {}",
                                atom.type_arguments.len()
                            ),
                            atom.span.clone(),
                        );
                    }
                    has_unknown = true;
                    continue;
                }
                let nominal = NominalTypeRef {
                    id: nominal_type_id,
                    display_name: name.to_string(),
                };
                match kind {
                    TypeKind::Struct { .. } => {
                        if type_parameter_count == 0 {
                            resolved.push(Type::Named(nominal));
                        } else {
                            resolved.push(Type::Applied {
                                base: nominal,
                                arguments: resolved_type_arguments,
                            });
                        }
                    }
                    TypeKind::Union { variants } => {
                        if type_parameter_count == 0 {
                            resolved.push(Self::normalize_union(variants));
                        } else {
                            let substitutions: HashMap<String, Type> = declared_type_parameters
                                .iter()
                                .cloned()
                                .zip(resolved_type_arguments.iter().cloned())
                                .collect();
                            let instantiated_variants = variants
                                .iter()
                                .map(|variant| Self::instantiate_type(variant, &substitutions))
                                .collect();
                            resolved.push(Self::normalize_union(instantiated_variants));
                        }
                    }
                }
                continue;
            }
            if let Some((enum_name, variant_name)) = name.split_once('.')
                && let Some(variant_type) = self.resolve_enum_variant_type(enum_name, variant_name)
            {
                if !atom.type_arguments.is_empty() {
                    self.error(
                        format!("enum variant '{name}' does not take type arguments"),
                        atom.span.clone(),
                    );
                    has_unknown = true;
                    continue;
                }
                resolved.push(variant_type);
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

    pub(crate) fn resolve_enum_variant_type(
        &mut self,
        enum_name: &str,
        variant_name: &str,
    ) -> Option<Type> {
        let info = self.types.get(enum_name)?;
        let TypeKind::Union { variants } = &info.kind else {
            return None;
        };
        let variants = variants.clone();
        let variant_display = format!("{enum_name}.{variant_name}");
        if matches!(
            self.imported_bindings.get(enum_name),
            Some(ImportedBindingInfo {
                symbol: ImportedSymbol::Type(_),
                ..
            })
        ) {
            self.mark_import_used(enum_name);
        }
        variants
            .into_iter()
            .find(|variant| variant.display() == variant_display)
    }

    fn check_unused_imports(&mut self) {
        let mut unused = Vec::new();
        for (name, binding) in &self.imported_bindings {
            if !binding.used {
                unused.push((name.clone(), binding.span.clone()));
            }
        }
        for (name, span) in unused {
            self.error(format!("unused import '{name}'"), span);
        }
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
