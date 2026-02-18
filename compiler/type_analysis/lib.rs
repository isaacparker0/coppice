use std::collections::HashMap;

use compiler__diagnostics::PhaseDiagnostic;
use compiler__packages::PackageId;
use compiler__phase_results::{PhaseOutput, PhaseStatus};
use compiler__semantic_program::{
    SemanticBinaryOperator, SemanticConstantDeclaration, SemanticDeclaration, SemanticExpression,
    SemanticFile, SemanticFunctionDeclaration, SemanticStatement, SemanticSymbolKind,
    SemanticTypeDeclaration, SemanticTypeName, SemanticUnaryOperator,
};
use compiler__semantic_types::{
    FileTypecheckSummary, GenericTypeParameter, ImportedBinding, ImportedSymbol,
    ImportedTypeDeclaration, NominalTypeId, NominalTypeRef, Type, TypedFunctionSignature,
    TypedSymbol, type_from_builtin_name,
};
use compiler__source::Span;
use compiler__type_annotated_program::{
    TypeAnnotatedBinaryOperator, TypeAnnotatedExpression, TypeAnnotatedFile,
    TypeAnnotatedFunctionDeclaration, TypeAnnotatedFunctionSignature,
    TypeAnnotatedParameterDeclaration, TypeAnnotatedStatement, TypeAnnotatedStructDeclaration,
    TypeAnnotatedStructFieldDeclaration, TypeAnnotatedStructLiteralField, TypeAnnotatedSymbolKind,
    TypeAnnotatedTypeName, TypeAnnotatedTypeNameSegment, TypeAnnotatedUnaryOperator,
};

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
    package_unit: &SemanticFile,
    imported_bindings: &[ImportedBinding],
) -> PhaseOutput<TypeAnnotatedFile> {
    let mut diagnostics = Vec::new();
    let summary = analyze_package_unit(
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
        value: TypeAnnotatedFile {
            function_signature_by_name: function_signature_by_name_from_summary(&summary),
            struct_declarations: build_struct_declaration_annotations(package_unit),
            function_declarations: build_function_declaration_annotations(package_unit),
        },
        diagnostics,
        status,
    }
}

fn function_signature_by_name_from_summary(
    summary: &FileTypecheckSummary,
) -> HashMap<String, TypeAnnotatedFunctionSignature> {
    let mut function_signature_by_name = HashMap::new();
    for (name, typed_symbol) in &summary.typed_symbol_by_name {
        let TypedSymbol::Function(function_signature) = typed_symbol else {
            continue;
        };
        function_signature_by_name.insert(
            name.clone(),
            TypeAnnotatedFunctionSignature {
                type_parameter_count: function_signature.type_parameters.len(),
                parameter_count: function_signature.parameter_types.len(),
                returns_nil: function_signature.return_type == Type::Nil,
            },
        );
    }
    function_signature_by_name
}

fn build_function_declaration_annotations(
    package_unit: &SemanticFile,
) -> Vec<TypeAnnotatedFunctionDeclaration> {
    package_unit
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            SemanticDeclaration::Function(function_declaration) => Some(function_declaration),
            _ => None,
        })
        .map(|function_declaration| TypeAnnotatedFunctionDeclaration {
            name: function_declaration.name.clone(),
            parameters: function_declaration
                .parameters
                .iter()
                .map(|parameter| TypeAnnotatedParameterDeclaration {
                    name: parameter.name.clone(),
                    type_name: type_annotated_type_name_from_semantic_type_name(
                        &parameter.type_name,
                    ),
                    span: parameter.span.clone(),
                })
                .collect(),
            return_type: type_annotated_type_name_from_semantic_type_name(
                &function_declaration.return_type,
            ),
            span: function_declaration.span.clone(),
            statements: function_declaration
                .body
                .statements
                .iter()
                .map(type_annotated_statement_from_semantic_statement)
                .collect(),
        })
        .collect()
}

fn build_struct_declaration_annotations(
    package_unit: &SemanticFile,
) -> Vec<TypeAnnotatedStructDeclaration> {
    package_unit
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            SemanticDeclaration::Type(type_declaration) => Some(type_declaration),
            _ => None,
        })
        .filter_map(|type_declaration| match &type_declaration.kind {
            compiler__semantic_program::SemanticTypeDeclarationKind::Struct { fields, .. } => {
                Some(TypeAnnotatedStructDeclaration {
                    name: type_declaration.name.clone(),
                    fields: fields
                        .iter()
                        .map(|field| TypeAnnotatedStructFieldDeclaration {
                            name: field.name.clone(),
                            type_name: type_annotated_type_name_from_semantic_type_name(
                                &field.type_name,
                            ),
                            span: field.span.clone(),
                        })
                        .collect(),
                    span: type_declaration.span.clone(),
                })
            }
            compiler__semantic_program::SemanticTypeDeclarationKind::Enum { .. }
            | compiler__semantic_program::SemanticTypeDeclarationKind::Union { .. } => None,
        })
        .collect()
}

fn type_annotated_statement_from_semantic_statement(
    statement: &SemanticStatement,
) -> TypeAnnotatedStatement {
    match statement {
        SemanticStatement::Binding {
            name,
            mutable,
            initializer,
            span,
            ..
        } => TypeAnnotatedStatement::Binding {
            name: name.clone(),
            mutable: *mutable,
            initializer: type_annotated_expression_from_semantic_expression(initializer),
            span: span.clone(),
        },
        SemanticStatement::Assign {
            name, value, span, ..
        } => TypeAnnotatedStatement::Assign {
            name: name.clone(),
            value: type_annotated_expression_from_semantic_expression(value),
            span: span.clone(),
        },
        SemanticStatement::If {
            condition,
            then_block,
            else_block,
            span,
        } => TypeAnnotatedStatement::If {
            condition: type_annotated_expression_from_semantic_expression(condition),
            then_statements: then_block
                .statements
                .iter()
                .map(type_annotated_statement_from_semantic_statement)
                .collect(),
            else_statements: else_block.as_ref().map(|block| {
                block
                    .statements
                    .iter()
                    .map(type_annotated_statement_from_semantic_statement)
                    .collect()
            }),
            span: span.clone(),
        },
        SemanticStatement::For {
            condition,
            body,
            span,
        } => TypeAnnotatedStatement::For {
            condition: condition
                .as_ref()
                .map(type_annotated_expression_from_semantic_expression),
            body_statements: body
                .statements
                .iter()
                .map(type_annotated_statement_from_semantic_statement)
                .collect(),
            span: span.clone(),
        },
        SemanticStatement::Break { span } => TypeAnnotatedStatement::Break { span: span.clone() },
        SemanticStatement::Continue { span } => {
            TypeAnnotatedStatement::Continue { span: span.clone() }
        }
        SemanticStatement::Expression { value, span } => TypeAnnotatedStatement::Expression {
            value: type_annotated_expression_from_semantic_expression(value),
            span: span.clone(),
        },
        SemanticStatement::Return { value, span } => TypeAnnotatedStatement::Return {
            value: type_annotated_expression_from_semantic_expression(value),
            span: span.clone(),
        },
    }
}

fn type_annotated_expression_from_semantic_expression(
    expression: &SemanticExpression,
) -> TypeAnnotatedExpression {
    match expression {
        SemanticExpression::IntegerLiteral { value, span } => {
            TypeAnnotatedExpression::IntegerLiteral {
                value: *value,
                span: span.clone(),
            }
        }
        SemanticExpression::BooleanLiteral { value, span } => {
            TypeAnnotatedExpression::BooleanLiteral {
                value: *value,
                span: span.clone(),
            }
        }
        SemanticExpression::NilLiteral { span } => {
            TypeAnnotatedExpression::NilLiteral { span: span.clone() }
        }
        SemanticExpression::StringLiteral { value, span } => {
            TypeAnnotatedExpression::StringLiteral {
                value: value.clone(),
                span: span.clone(),
            }
        }
        SemanticExpression::Symbol { name, kind, span } => TypeAnnotatedExpression::Symbol {
            name: name.clone(),
            kind: match kind {
                SemanticSymbolKind::UserDefined => TypeAnnotatedSymbolKind::UserDefined,
                SemanticSymbolKind::Builtin => TypeAnnotatedSymbolKind::Builtin,
            },
            span: span.clone(),
        },
        SemanticExpression::StructLiteral {
            type_name,
            fields,
            span,
        } => TypeAnnotatedExpression::StructLiteral {
            type_name: type_annotated_type_name_from_semantic_type_name(type_name),
            fields: fields
                .iter()
                .map(|field| TypeAnnotatedStructLiteralField {
                    name: field.name.clone(),
                    value: type_annotated_expression_from_semantic_expression(&field.value),
                    span: field.span.clone(),
                })
                .collect(),
            span: span.clone(),
        },
        SemanticExpression::FieldAccess {
            target,
            field,
            span,
            ..
        } => TypeAnnotatedExpression::FieldAccess {
            target: Box::new(type_annotated_expression_from_semantic_expression(target)),
            field: field.clone(),
            span: span.clone(),
        },
        SemanticExpression::Unary {
            operator,
            expression,
            span,
        } => TypeAnnotatedExpression::Unary {
            operator: match operator {
                SemanticUnaryOperator::Not => TypeAnnotatedUnaryOperator::Not,
                SemanticUnaryOperator::Negate => TypeAnnotatedUnaryOperator::Negate,
            },
            expression: Box::new(type_annotated_expression_from_semantic_expression(
                expression,
            )),
            span: span.clone(),
        },
        SemanticExpression::Binary {
            operator,
            left,
            right,
            span,
        } => match operator {
            SemanticBinaryOperator::Add => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::Add,
                left: Box::new(type_annotated_expression_from_semantic_expression(left)),
                right: Box::new(type_annotated_expression_from_semantic_expression(right)),
                span: span.clone(),
            },
            SemanticBinaryOperator::Subtract => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::Subtract,
                left: Box::new(type_annotated_expression_from_semantic_expression(left)),
                right: Box::new(type_annotated_expression_from_semantic_expression(right)),
                span: span.clone(),
            },
            SemanticBinaryOperator::Multiply => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::Multiply,
                left: Box::new(type_annotated_expression_from_semantic_expression(left)),
                right: Box::new(type_annotated_expression_from_semantic_expression(right)),
                span: span.clone(),
            },
            SemanticBinaryOperator::Divide => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::Divide,
                left: Box::new(type_annotated_expression_from_semantic_expression(left)),
                right: Box::new(type_annotated_expression_from_semantic_expression(right)),
                span: span.clone(),
            },
            SemanticBinaryOperator::EqualEqual => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::EqualEqual,
                left: Box::new(type_annotated_expression_from_semantic_expression(left)),
                right: Box::new(type_annotated_expression_from_semantic_expression(right)),
                span: span.clone(),
            },
            SemanticBinaryOperator::NotEqual => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::NotEqual,
                left: Box::new(type_annotated_expression_from_semantic_expression(left)),
                right: Box::new(type_annotated_expression_from_semantic_expression(right)),
                span: span.clone(),
            },
            SemanticBinaryOperator::LessThan => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::LessThan,
                left: Box::new(type_annotated_expression_from_semantic_expression(left)),
                right: Box::new(type_annotated_expression_from_semantic_expression(right)),
                span: span.clone(),
            },
            SemanticBinaryOperator::LessThanOrEqual => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::LessThanOrEqual,
                left: Box::new(type_annotated_expression_from_semantic_expression(left)),
                right: Box::new(type_annotated_expression_from_semantic_expression(right)),
                span: span.clone(),
            },
            SemanticBinaryOperator::GreaterThan => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::GreaterThan,
                left: Box::new(type_annotated_expression_from_semantic_expression(left)),
                right: Box::new(type_annotated_expression_from_semantic_expression(right)),
                span: span.clone(),
            },
            SemanticBinaryOperator::GreaterThanOrEqual => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::GreaterThanOrEqual,
                left: Box::new(type_annotated_expression_from_semantic_expression(left)),
                right: Box::new(type_annotated_expression_from_semantic_expression(right)),
                span: span.clone(),
            },
            SemanticBinaryOperator::And => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::And,
                left: Box::new(type_annotated_expression_from_semantic_expression(left)),
                right: Box::new(type_annotated_expression_from_semantic_expression(right)),
                span: span.clone(),
            },
            SemanticBinaryOperator::Or => TypeAnnotatedExpression::Binary {
                operator: TypeAnnotatedBinaryOperator::Or,
                left: Box::new(type_annotated_expression_from_semantic_expression(left)),
                right: Box::new(type_annotated_expression_from_semantic_expression(right)),
                span: span.clone(),
            },
        },
        SemanticExpression::Call {
            callee,
            type_arguments,
            arguments,
            span,
        } => TypeAnnotatedExpression::Call {
            callee: Box::new(type_annotated_expression_from_semantic_expression(callee)),
            arguments: arguments
                .iter()
                .map(type_annotated_expression_from_semantic_expression)
                .collect(),
            has_type_arguments: !type_arguments.is_empty(),
            span: span.clone(),
        },
        _ => TypeAnnotatedExpression::Unsupported {
            span: expression.span(),
        },
    }
}

fn type_annotated_type_name_from_semantic_type_name(
    type_name: &SemanticTypeName,
) -> TypeAnnotatedTypeName {
    TypeAnnotatedTypeName {
        names: type_name
            .names
            .iter()
            .map(|name_segment| TypeAnnotatedTypeNameSegment {
                name: name_segment.name.clone(),
                has_type_arguments: !name_segment.type_arguments.is_empty(),
                span: name_segment.span.clone(),
            })
            .collect(),
        span: type_name.span.clone(),
    }
}

pub fn analyze_package_unit(
    package_id: PackageId,
    package_unit: &SemanticFile,
    imported_bindings: &[ImportedBinding],
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> FileTypecheckSummary {
    check_package_unit_declarations(package_id, package_unit, imported_bindings, diagnostics)
}

fn check_package_unit_declarations(
    package_id: PackageId,
    package_unit: &SemanticFile,
    imported_bindings: &[ImportedBinding],
    diagnostics: &mut Vec<PhaseDiagnostic>,
) -> FileTypecheckSummary {
    let mut type_declarations = Vec::new();
    let mut constant_declarations = Vec::new();
    let mut function_declarations = Vec::new();
    for declaration in &package_unit.declarations {
        match declaration {
            SemanticDeclaration::Type(type_declaration) => {
                type_declarations.push(type_declaration.clone());
            }
            SemanticDeclaration::Constant(constant_declaration) => {
                constant_declarations.push(constant_declaration.clone());
            }
            SemanticDeclaration::Function(function_declaration) => {
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
    diagnostics: &mut Vec<PhaseDiagnostic>,
    type_declarations: &[SemanticTypeDeclaration],
    constant_declarations: &[SemanticConstantDeclaration],
    function_declarations: &[SemanticFunctionDeclaration],
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
    type_parameters: Vec<GenericTypeParameter>,
    kind: TypeKind,
}

#[derive(Clone)]
enum TypeKind {
    Struct { fields: Vec<(String, Type)> },
    Union { variants: Vec<Type> },
}

#[derive(Clone)]
struct FunctionInfo {
    type_parameters: Vec<GenericTypeParameter>,
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
    diagnostics: &'a mut Vec<PhaseDiagnostic>,
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
        diagnostics: &'a mut Vec<PhaseDiagnostic>,
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
            functions: builtin_functions(),
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
        type_declarations: &[SemanticTypeDeclaration],
        function_declarations: &[SemanticFunctionDeclaration],
        constant_declarations: &[SemanticConstantDeclaration],
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

    fn symbol_expression_is_callable(&self, name: &str, kind: SemanticSymbolKind) -> bool {
        kind == SemanticSymbolKind::Builtin
            || self.functions.contains_key(name)
            || self.imported_functions.contains_key(name)
    }

    fn check_symbol_expression(
        &mut self,
        name: &str,
        kind: SemanticSymbolKind,
        span: &Span,
    ) -> Type {
        if self.symbol_expression_is_callable(name, kind) {
            // TODO: when first-class function types are introduced, return the function
            // value type here instead of requiring an immediate call.
            if kind == SemanticSymbolKind::Builtin {
                self.error(
                    format!("builtin function '{name}' must be called"),
                    span.clone(),
                );
            } else {
                if self.imported_functions.contains_key(name) {
                    self.mark_import_used(name);
                }
                self.error(format!("function '{name}' must be called"), span.clone());
            }
            return Type::Unknown;
        }
        self.resolve_variable(name, span)
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
        self.diagnostics.push(PhaseDiagnostic::new(message, span));
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

    fn check_type_argument_constraints(
        &mut self,
        context_name: &str,
        type_parameters: &[GenericTypeParameter],
        resolved_type_arguments: &[Type],
        span: &Span,
    ) {
        for (type_parameter, type_argument) in type_parameters.iter().zip(resolved_type_arguments) {
            let Some(constraint) = &type_parameter.constraint else {
                continue;
            };
            if *type_argument == Type::Unknown || *constraint == Type::Unknown {
                continue;
            }
            if !Self::is_assignable(type_argument, constraint) {
                self.error(
                    format!(
                        "type argument '{}' does not satisfy constraint '{}' for type parameter '{}' on '{}'",
                        type_argument.display(),
                        constraint.display(),
                        type_parameter.name,
                        context_name
                    ),
                    span.clone(),
                );
            }
        }
    }

    fn resolve_type_name(&mut self, type_name: &SemanticTypeName) -> Type {
        let mut resolved = Vec::new();
        let mut has_unknown = false;
        for segment in &type_name.names {
            let name = segment.name.as_str();
            if let Some(type_parameter) = self.resolve_type_parameter(name) {
                if !segment.type_arguments.is_empty() {
                    self.error(
                        format!("type parameter '{name}' does not take type arguments"),
                        segment.span.clone(),
                    );
                    has_unknown = true;
                    continue;
                }
                resolved.push(type_parameter);
                continue;
            }
            if let Some(builtin) = type_from_builtin_name(name) {
                if !segment.type_arguments.is_empty() {
                    self.error(
                        format!("built-in type '{name}' does not take type arguments"),
                        segment.span.clone(),
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
                let resolved_type_arguments = segment
                    .type_arguments
                    .iter()
                    .map(|argument| self.resolve_type_name(argument))
                    .collect::<Vec<_>>();
                if segment.type_arguments.len() != type_parameter_count {
                    if type_parameter_count == 0 {
                        self.error(
                            format!("type '{name}' does not take type arguments"),
                            segment.span.clone(),
                        );
                    } else {
                        self.error(
                            format!(
                                "type '{name}' expects {type_parameter_count} type arguments, got {}",
                                segment.type_arguments.len()
                            ),
                            segment.span.clone(),
                        );
                    }
                    has_unknown = true;
                    continue;
                }
                self.check_type_argument_constraints(
                    name,
                    &declared_type_parameters,
                    &resolved_type_arguments,
                    &segment.span,
                );
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
                                .map(|parameter| parameter.name.clone())
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
                if !segment.type_arguments.is_empty() {
                    self.error(
                        format!("enum variant '{name}' does not take type arguments"),
                        segment.span.clone(),
                    );
                    has_unknown = true;
                    continue;
                }
                resolved.push(variant_type);
                continue;
            }
            self.error(format!("unknown type '{name}'"), segment.span.clone());
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

fn builtin_functions() -> HashMap<String, FunctionInfo> {
    let mut functions = HashMap::new();
    functions.insert(
        "abort".to_string(),
        FunctionInfo {
            type_parameters: Vec::new(),
            parameter_types: vec![Type::String],
            return_type: Type::Never,
        },
    );
    functions.insert(
        "print".to_string(),
        FunctionInfo {
            type_parameters: Vec::new(),
            parameter_types: vec![Type::String],
            return_type: Type::Nil,
        },
    );
    functions
}

impl ExpressionSpan for SemanticExpression {
    fn span(&self) -> Span {
        match self {
            SemanticExpression::IntegerLiteral { span, .. }
            | SemanticExpression::NilLiteral { span, .. }
            | SemanticExpression::BooleanLiteral { span, .. }
            | SemanticExpression::StringLiteral { span, .. }
            | SemanticExpression::Symbol { span, .. }
            | SemanticExpression::StructLiteral { span, .. }
            | SemanticExpression::FieldAccess { span, .. }
            | SemanticExpression::Call { span, .. }
            | SemanticExpression::Unary { span, .. }
            | SemanticExpression::Binary { span, .. }
            | SemanticExpression::Match { span, .. }
            | SemanticExpression::Matches { span, .. } => span.clone(),
        }
    }
}

impl StatementSpan for SemanticStatement {
    fn span(&self) -> Span {
        match self {
            SemanticStatement::Binding { span, .. }
            | SemanticStatement::Assign { span, .. }
            | SemanticStatement::Return { span, .. }
            | SemanticStatement::If { span, .. }
            | SemanticStatement::For { span, .. }
            | SemanticStatement::Break { span, .. }
            | SemanticStatement::Continue { span, .. }
            | SemanticStatement::Expression { span, .. } => span.clone(),
        }
    }
}
