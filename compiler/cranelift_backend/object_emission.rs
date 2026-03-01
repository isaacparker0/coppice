use std::collections::BTreeMap;
use std::sync::Arc;

use crate::build_failed;
use crate::builtin_conversion::convert_int64_to_string;
use crate::runtime_interface_emission::{
    ExternalRuntimeFunctions, declare_runtime_interface_functions,
};
use compiler__executable_program::{
    ExecutableAssignTarget, ExecutableBinaryOperator, ExecutableCallTarget,
    ExecutableCallableReference, ExecutableConstantDeclaration, ExecutableConstantReference,
    ExecutableEnumVariantReference, ExecutableExpression, ExecutableFunctionDeclaration,
    ExecutableInterfaceDeclaration, ExecutableInterfaceReference, ExecutableMatchArm,
    ExecutableMatchPattern, ExecutableMethodDeclaration, ExecutableNominalTypeReference,
    ExecutableProgram, ExecutableStatement, ExecutableStructDeclaration, ExecutableStructReference,
    ExecutableTypeReference, ExecutableUnaryOperator,
};
use compiler__reports::CompilerFailure;
use compiler__runtime_interface::{
    ABORT_FUNCTION_CONTRACT, ASSERT_FUNCTION_CONTRACT, PRINT_FUNCTION_CONTRACT,
};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{
    AbiParam, Block, BlockArg, InstBuilder, MemFlags, Signature, TrapCode, Value, types,
};
use cranelift_codegen::isa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{FuncId, Linkage, Module, default_libcall_names};
use cranelift_native as native_isa;
use cranelift_object::{ObjectBuilder, ObjectModule};

#[derive(Clone)]
struct FunctionRecord {
    id: FuncId,
    parameter_types: Vec<ExecutableTypeReference>,
    return_type: ExecutableTypeReference,
    type_parameter_names: Vec<String>,
    type_parameter_constraint_interface_reference_by_name:
        BTreeMap<String, ExecutableInterfaceReference>,
}

#[derive(Clone)]
struct MethodRecord {
    id: FuncId,
    parameter_types: Vec<ExecutableTypeReference>,
    return_type: ExecutableTypeReference,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MethodKey {
    struct_reference: ExecutableStructReference,
    method_name: String,
}

#[derive(Clone)]
struct TypedValue {
    value: Option<Value>,
    type_reference: ExecutableTypeReference,
    terminates: bool,
}

#[derive(Clone)]
struct LocalValue {
    variable: Variable,
    type_reference: ExecutableTypeReference,
}

#[derive(Clone, Copy)]
struct LoopContext {
    header_block: Block,
    exit_block: Block,
}

struct FunctionCompilationContext {
    local_value_by_name: BTreeMap<String, LocalValue>,
    type_parameter_witness_by_name: BTreeMap<String, TypeParameterWitness>,
    loop_context: Option<LoopContext>,
}

#[derive(Clone)]
struct TypeParameterWitness {
    interface_reference: ExecutableInterfaceReference,
    witness_table_pointer: Value,
}

pub(crate) struct CompilationState<'program> {
    module: ObjectModule,
    function_record_by_callable_reference: BTreeMap<ExecutableCallableReference, FunctionRecord>,
    method_record_by_key: BTreeMap<MethodKey, MethodRecord>,
    interface_declaration_by_reference:
        BTreeMap<ExecutableInterfaceReference, &'program ExecutableInterfaceDeclaration>,
    constant_declaration_by_reference:
        BTreeMap<ExecutableConstantReference, &'program ExecutableConstantDeclaration>,
    struct_declaration_by_reference:
        BTreeMap<ExecutableStructReference, &'program ExecutableStructDeclaration>,
    external_runtime_functions: ExternalRuntimeFunctions,
}

const UNION_BOX_TAG_OFFSET: i32 = 0;
const UNION_BOX_PAYLOAD_OFFSET: i32 = 8;
const UNION_BOX_SIZE_BYTES: i64 = 16;
const INTERFACE_VALUE_DATA_POINTER_OFFSET: i32 = 0;
const INTERFACE_VALUE_VTABLE_POINTER_OFFSET: i32 = 8;
const INTERFACE_VALUE_SIZE_BYTES: i64 = 16;
const LIST_LENGTH_OFFSET: i32 = 0;
const LIST_DATA_POINTER_OFFSET: i32 = 8;
const LIST_HEADER_SIZE_BYTES: i64 = 16;

const UNION_TAG_INT64: i64 = 1;
const UNION_TAG_BOOLEAN: i64 = 2;
const UNION_TAG_STRING: i64 = 3;
const UNION_TAG_NIL: i64 = 4;
const UNION_TAG_STRUCT: i64 = 5;
const UNION_TAG_ENUM_VARIANT: i64 = 6;
const UNION_TAG_FUNCTION: i64 = 7;
pub(crate) fn ensure_program_supported(program: &ExecutableProgram) -> Result<(), CompilerFailure> {
    for constant_declaration in &program.constant_declarations {
        ensure_type_supported(&constant_declaration.type_reference);
        ensure_expression_supported(&constant_declaration.initializer)?;
    }

    for function_declaration in &program.function_declarations {
        for parameter in &function_declaration.parameters {
            ensure_type_supported(&parameter.type_reference);
        }
        ensure_type_supported(&function_declaration.return_type);

        for statement in &function_declaration.statements {
            ensure_statement_supported(statement)?;
        }
    }

    for struct_declaration in &program.struct_declarations {
        for field in &struct_declaration.fields {
            ensure_type_supported(&field.type_reference);
        }
        for method in &struct_declaration.methods {
            for parameter in &method.parameters {
                ensure_type_supported(&parameter.type_reference);
            }
            ensure_type_supported(&method.return_type);
            for statement in &method.statements {
                ensure_statement_supported(statement)?;
            }
        }
    }

    for interface_declaration in &program.interface_declarations {
        for method in &interface_declaration.methods {
            for parameter in &method.parameters {
                ensure_type_supported(&parameter.type_reference);
            }
            ensure_type_supported(&method.return_type);
        }
    }

    Ok(())
}

fn ensure_type_supported(type_reference: &ExecutableTypeReference) {
    match type_reference {
        ExecutableTypeReference::Int64
        | ExecutableTypeReference::Boolean
        | ExecutableTypeReference::String
        | ExecutableTypeReference::Nil
        | ExecutableTypeReference::Never
        | ExecutableTypeReference::List { .. }
        | ExecutableTypeReference::Function { .. }
        | ExecutableTypeReference::TypeParameter { .. }
        | ExecutableTypeReference::NominalType { .. }
        | ExecutableTypeReference::NominalTypeApplication { .. }
        | ExecutableTypeReference::Union { .. } => {}
    }
}

fn ensure_statement_supported(statement: &ExecutableStatement) -> Result<(), CompilerFailure> {
    match statement {
        ExecutableStatement::Binding { initializer, .. }
        | ExecutableStatement::Expression {
            expression: initializer,
        }
        | ExecutableStatement::Return { value: initializer } => {
            ensure_expression_supported(initializer)
        }
        ExecutableStatement::Assign { target, value } => {
            match target {
                ExecutableAssignTarget::Name { .. } => {}
                ExecutableAssignTarget::Index { target, index } => {
                    ensure_expression_supported(target)?;
                    ensure_expression_supported(index)?;
                }
            }
            ensure_expression_supported(value)
        }
        ExecutableStatement::If {
            condition,
            then_statements,
            else_statements,
        } => {
            ensure_expression_supported(condition)?;
            for nested in then_statements {
                ensure_statement_supported(nested)?;
            }
            if let Some(else_statements) = else_statements {
                for nested in else_statements {
                    ensure_statement_supported(nested)?;
                }
            }
            Ok(())
        }
        ExecutableStatement::For {
            condition,
            body_statements,
        } => {
            if let Some(condition) = condition {
                ensure_expression_supported(condition)?;
            }
            for nested in body_statements {
                ensure_statement_supported(nested)?;
            }
            Ok(())
        }
        ExecutableStatement::Break | ExecutableStatement::Continue => Ok(()),
    }
}

fn ensure_expression_supported(expression: &ExecutableExpression) -> Result<(), CompilerFailure> {
    match expression {
        ExecutableExpression::IntegerLiteral { .. }
        | ExecutableExpression::BooleanLiteral { .. }
        | ExecutableExpression::NilLiteral
        | ExecutableExpression::StringLiteral { .. }
        | ExecutableExpression::ListLiteral { .. }
        | ExecutableExpression::Identifier { .. }
        | ExecutableExpression::StructLiteral { .. }
        | ExecutableExpression::IndexAccess { .. }
        | ExecutableExpression::FieldAccess { .. }
        | ExecutableExpression::EnumVariantLiteral { .. } => Ok(()),
        ExecutableExpression::Unary { expression, .. } => ensure_expression_supported(expression),
        ExecutableExpression::Binary { left, right, .. } => {
            ensure_expression_supported(left)?;
            ensure_expression_supported(right)
        }
        ExecutableExpression::Call {
            call_target,
            arguments,
            ..
        } => {
            let _ = call_target;
            for argument in arguments {
                ensure_expression_supported(argument)?;
            }
            Ok(())
        }
        ExecutableExpression::Match { target, arms } => {
            ensure_expression_supported(target)?;
            for arm in arms {
                ensure_expression_supported(&arm.value)?;
            }
            Ok(())
        }
        ExecutableExpression::Matches { value, .. } => ensure_expression_supported(value),
    }
}

pub(crate) fn emit_object_bytes(program: &ExecutableProgram) -> Result<Vec<u8>, CompilerFailure> {
    let isa = create_native_isa()?;
    let object_builder =
        ObjectBuilder::new(isa, "coppice", default_libcall_names()).map_err(|error| {
            build_failed(
                format!("failed to initialize Cranelift object builder: {error}"),
                None,
            )
        })?;
    let mut module = ObjectModule::new(object_builder);

    let external_runtime_functions = declare_runtime_interface_functions(&mut module)?;
    let function_record_by_callable_reference =
        declare_program_functions(&mut module, &program.function_declarations)?;
    let method_record_by_key = declare_struct_methods(&mut module, &program.struct_declarations)?;
    let constant_declaration_by_reference = program
        .constant_declarations
        .iter()
        .map(|declaration| (declaration.constant_reference.clone(), declaration))
        .collect();
    let struct_declaration_by_reference = program
        .struct_declarations
        .iter()
        .map(|declaration| (declaration.struct_reference.clone(), declaration))
        .collect();
    let interface_declaration_by_reference = program
        .interface_declarations
        .iter()
        .map(|declaration| (declaration.interface_reference.clone(), declaration))
        .collect();

    let mut state = CompilationState {
        module,
        function_record_by_callable_reference,
        method_record_by_key,
        interface_declaration_by_reference,
        constant_declaration_by_reference,
        struct_declaration_by_reference,
        external_runtime_functions,
    };

    for function_declaration in &program.function_declarations {
        define_program_function(&mut state, function_declaration)?;
    }
    for struct_declaration in &program.struct_declarations {
        for method_declaration in &struct_declaration.methods {
            define_struct_method(&mut state, struct_declaration, method_declaration)?;
        }
    }

    define_process_entrypoint(&mut state, &program.entrypoint_callable_reference)?;

    let product = state.module.finish();
    product
        .emit()
        .map_err(|error| build_failed(format!("failed to emit object bytes: {error}"), None))
}

fn create_native_isa() -> Result<Arc<dyn isa::TargetIsa>, CompilerFailure> {
    let mut flag_builder = settings::builder();
    flag_builder.set("opt_level", "speed").map_err(|error| {
        build_failed(format!("failed to set optimization level: {error}"), None)
    })?;
    flag_builder
        .set("is_pic", "true")
        .map_err(|error| build_failed(format!("failed to enable PIC: {error}"), None))?;

    let isa_builder = native_isa::builder().map_err(|error| {
        build_failed(
            format!("failed to create native ISA builder: {error}"),
            None,
        )
    })?;

    isa_builder
        .finish(settings::Flags::new(flag_builder))
        .map_err(|error| build_failed(format!("failed to finalize native ISA: {error}"), None))
}

fn declare_program_functions(
    module: &mut ObjectModule,
    function_declarations: &[ExecutableFunctionDeclaration],
) -> Result<BTreeMap<ExecutableCallableReference, FunctionRecord>, CompilerFailure> {
    let mut function_record_by_callable_reference = BTreeMap::new();

    for function_declaration in function_declarations {
        let signature = build_signature_for_function(module, function_declaration);
        let symbol_name = lowered_function_symbol_name(&function_declaration.callable_reference);
        let id = module
            .declare_function(&symbol_name, Linkage::Local, &signature)
            .map_err(|error| {
                build_failed(
                    format!("failed to declare function '{symbol_name}': {error}"),
                    None,
                )
            })?;
        function_record_by_callable_reference.insert(
            function_declaration.callable_reference.clone(),
            FunctionRecord {
                id,
                parameter_types: function_declaration
                    .parameters
                    .iter()
                    .map(|parameter| parameter.type_reference.clone())
                    .collect(),
                return_type: function_declaration.return_type.clone(),
                type_parameter_names: function_declaration.type_parameter_names.clone(),
                type_parameter_constraint_interface_reference_by_name: function_declaration
                    .type_parameter_constraint_interface_reference_by_name
                    .clone(),
            },
        );
    }

    Ok(function_record_by_callable_reference)
}

fn declare_struct_methods(
    module: &mut ObjectModule,
    struct_declarations: &[ExecutableStructDeclaration],
) -> Result<BTreeMap<MethodKey, MethodRecord>, CompilerFailure> {
    let mut method_record_by_key = BTreeMap::new();
    for struct_declaration in struct_declarations {
        for method_declaration in &struct_declaration.methods {
            let signature =
                build_signature_for_method(module, struct_declaration, method_declaration);
            let symbol_name = lowered_method_symbol_name(
                &struct_declaration.struct_reference,
                &method_declaration.name,
            );
            let id = module
                .declare_function(&symbol_name, Linkage::Local, &signature)
                .map_err(|error| {
                    build_failed(
                        format!("failed to declare method '{symbol_name}': {error}"),
                        None,
                    )
                })?;
            let key = MethodKey {
                struct_reference: struct_declaration.struct_reference.clone(),
                method_name: method_declaration.name.clone(),
            };
            method_record_by_key.insert(
                key,
                MethodRecord {
                    id,
                    parameter_types: method_declaration
                        .parameters
                        .iter()
                        .map(|parameter| parameter.type_reference.clone())
                        .collect(),
                    return_type: method_declaration.return_type.clone(),
                },
            );
        }
    }

    Ok(method_record_by_key)
}

fn build_signature_for_function(
    module: &mut ObjectModule,
    function_declaration: &ExecutableFunctionDeclaration,
) -> Signature {
    let mut signature = module.make_signature();
    for parameter in &function_declaration.parameters {
        signature
            .params
            .push(AbiParam::new(cranelift_type_for(&parameter.type_reference)));
    }
    for type_parameter_name in &function_declaration.type_parameter_names {
        if function_declaration
            .type_parameter_constraint_interface_reference_by_name
            .contains_key(type_parameter_name)
        {
            signature.params.push(AbiParam::new(types::I64));
        }
    }

    if !matches!(
        function_declaration.return_type,
        ExecutableTypeReference::Nil | ExecutableTypeReference::Never
    ) {
        signature.returns.push(AbiParam::new(cranelift_type_for(
            &function_declaration.return_type,
        )));
    }

    signature
}

fn build_signature_for_method(
    module: &mut ObjectModule,
    struct_declaration: &ExecutableStructDeclaration,
    method_declaration: &ExecutableMethodDeclaration,
) -> Signature {
    let mut signature = module.make_signature();
    let self_type_reference = if struct_declaration.type_parameter_names.is_empty() {
        ExecutableTypeReference::NominalType {
            nominal_type_reference: Some(ExecutableNominalTypeReference {
                package_path: struct_declaration.struct_reference.package_path.clone(),
                symbol_name: struct_declaration.struct_reference.symbol_name.clone(),
            }),
            name: struct_declaration.name.clone(),
        }
    } else {
        ExecutableTypeReference::NominalTypeApplication {
            base_nominal_type_reference: Some(ExecutableNominalTypeReference {
                package_path: struct_declaration.struct_reference.package_path.clone(),
                symbol_name: struct_declaration.struct_reference.symbol_name.clone(),
            }),
            base_name: struct_declaration.name.clone(),
            arguments: struct_declaration
                .type_parameter_names
                .iter()
                .map(|name| ExecutableTypeReference::TypeParameter { name: name.clone() })
                .collect(),
        }
    };
    signature
        .params
        .push(AbiParam::new(cranelift_type_for(&self_type_reference)));

    for parameter in &method_declaration.parameters {
        signature
            .params
            .push(AbiParam::new(cranelift_type_for(&parameter.type_reference)));
    }

    if !matches!(
        method_declaration.return_type,
        ExecutableTypeReference::Nil | ExecutableTypeReference::Never
    ) {
        signature.returns.push(AbiParam::new(cranelift_type_for(
            &method_declaration.return_type,
        )));
    }

    signature
}

fn cranelift_type_for(type_reference: &ExecutableTypeReference) -> types::Type {
    match type_reference {
        ExecutableTypeReference::Int64
        | ExecutableTypeReference::String
        | ExecutableTypeReference::List { .. }
        | ExecutableTypeReference::Function { .. }
        | ExecutableTypeReference::TypeParameter { .. }
        | ExecutableTypeReference::NominalType { .. }
        | ExecutableTypeReference::NominalTypeApplication { .. }
        | ExecutableTypeReference::Union { .. } => types::I64,
        ExecutableTypeReference::Boolean
        | ExecutableTypeReference::Nil
        | ExecutableTypeReference::Never => types::I8,
    }
}

fn define_program_function(
    state: &mut CompilationState<'_>,
    function_declaration: &ExecutableFunctionDeclaration,
) -> Result<(), CompilerFailure> {
    let function_id = state
        .function_record_by_callable_reference
        .get(&function_declaration.callable_reference)
        .ok_or_else(|| {
            build_failed(
                format!(
                    "missing function record for '{}::{}'",
                    function_declaration.callable_reference.package_path,
                    function_declaration.callable_reference.symbol_name
                ),
                None,
            )
        })?
        .id;

    let mut context = state.module.make_context();
    context.func.signature = build_signature_for_function(&mut state.module, function_declaration);

    let mut function_builder_context = FunctionBuilderContext::new();
    {
        let mut function_builder =
            FunctionBuilder::new(&mut context.func, &mut function_builder_context);

        let entry_block = function_builder.create_block();
        function_builder.append_block_params_for_function_params(entry_block);
        function_builder.switch_to_block(entry_block);
        function_builder.seal_block(entry_block);

        let parameter_values = function_builder.block_params(entry_block).to_vec();

        let mut compilation_context = FunctionCompilationContext {
            local_value_by_name: BTreeMap::new(),
            type_parameter_witness_by_name: BTreeMap::new(),
            loop_context: None,
        };

        for (index, parameter) in function_declaration.parameters.iter().enumerate() {
            let local_value = declare_local_variable(
                &mut function_builder,
                parameter_values[index],
                parameter.type_reference.clone(),
            );
            compilation_context
                .local_value_by_name
                .insert(parameter.name.clone(), local_value);
        }
        let mut witness_parameter_offset = function_declaration.parameters.len();
        for type_parameter_name in &function_declaration.type_parameter_names {
            let Some(interface_reference) = function_declaration
                .type_parameter_constraint_interface_reference_by_name
                .get(type_parameter_name)
            else {
                continue;
            };
            let witness_table_pointer = parameter_values
                .get(witness_parameter_offset)
                .copied()
                .ok_or_else(|| {
                    build_failed(
                        format!(
                            "missing witness parameter for type parameter '{type_parameter_name}'"
                        ),
                        None,
                    )
                })?;
            witness_parameter_offset += 1;
            compilation_context.type_parameter_witness_by_name.insert(
                type_parameter_name.clone(),
                TypeParameterWitness {
                    interface_reference: interface_reference.clone(),
                    witness_table_pointer,
                },
            );
        }

        let terminated = compile_statements(
            state,
            &mut function_builder,
            &mut compilation_context,
            &function_declaration.statements,
            &function_declaration.return_type,
        )?;

        if !terminated {
            if matches!(
                function_declaration.return_type,
                ExecutableTypeReference::Nil | ExecutableTypeReference::Never
            ) {
                function_builder.ins().return_(&[]);
            } else {
                let default_value = function_builder
                    .ins()
                    .iconst(cranelift_type_for(&function_declaration.return_type), 0);
                function_builder.ins().return_(&[default_value]);
            }
        }

        function_builder.finalize();
    }

    state
        .module
        .define_function(function_id, &mut context)
        .map_err(|error| {
            build_failed(
                format!(
                    "failed to define function '{}::{}': {error}",
                    function_declaration.callable_reference.package_path,
                    function_declaration.callable_reference.symbol_name
                ),
                None,
            )
        })?;
    state.module.clear_context(&mut context);

    Ok(())
}

fn define_struct_method(
    state: &mut CompilationState<'_>,
    struct_declaration: &ExecutableStructDeclaration,
    method_declaration: &ExecutableMethodDeclaration,
) -> Result<(), CompilerFailure> {
    let method_key = MethodKey {
        struct_reference: struct_declaration.struct_reference.clone(),
        method_name: method_declaration.name.clone(),
    };
    let method_id = state
        .method_record_by_key
        .get(&method_key)
        .ok_or_else(|| {
            build_failed(
                format!(
                    "missing method record for '{}::{}'",
                    struct_declaration.struct_reference.symbol_name, method_declaration.name
                ),
                None,
            )
        })?
        .id;

    let mut context = state.module.make_context();
    context.func.signature =
        build_signature_for_method(&mut state.module, struct_declaration, method_declaration);

    let mut function_builder_context = FunctionBuilderContext::new();
    {
        let mut function_builder =
            FunctionBuilder::new(&mut context.func, &mut function_builder_context);

        let entry_block = function_builder.create_block();
        function_builder.append_block_params_for_function_params(entry_block);
        function_builder.switch_to_block(entry_block);
        function_builder.seal_block(entry_block);

        let parameter_values = function_builder.block_params(entry_block).to_vec();

        let mut compilation_context = FunctionCompilationContext {
            local_value_by_name: BTreeMap::new(),
            type_parameter_witness_by_name: BTreeMap::new(),
            loop_context: None,
        };

        let self_type_reference = if struct_declaration.type_parameter_names.is_empty() {
            ExecutableTypeReference::NominalType {
                nominal_type_reference: Some(ExecutableNominalTypeReference {
                    package_path: struct_declaration.struct_reference.package_path.clone(),
                    symbol_name: struct_declaration.struct_reference.symbol_name.clone(),
                }),
                name: struct_declaration.name.clone(),
            }
        } else {
            ExecutableTypeReference::NominalTypeApplication {
                base_nominal_type_reference: Some(ExecutableNominalTypeReference {
                    package_path: struct_declaration.struct_reference.package_path.clone(),
                    symbol_name: struct_declaration.struct_reference.symbol_name.clone(),
                }),
                base_name: struct_declaration.name.clone(),
                arguments: struct_declaration
                    .type_parameter_names
                    .iter()
                    .map(|name| ExecutableTypeReference::TypeParameter { name: name.clone() })
                    .collect(),
            }
        };
        let self_local = declare_local_variable(
            &mut function_builder,
            parameter_values[0],
            self_type_reference,
        );
        compilation_context
            .local_value_by_name
            .insert("self".to_string(), self_local);

        for (index, parameter) in method_declaration.parameters.iter().enumerate() {
            let local_value = declare_local_variable(
                &mut function_builder,
                parameter_values[index + 1],
                parameter.type_reference.clone(),
            );
            compilation_context
                .local_value_by_name
                .insert(parameter.name.clone(), local_value);
        }

        let terminated = compile_statements(
            state,
            &mut function_builder,
            &mut compilation_context,
            &method_declaration.statements,
            &method_declaration.return_type,
        )?;
        if !terminated {
            if matches!(
                method_declaration.return_type,
                ExecutableTypeReference::Nil | ExecutableTypeReference::Never
            ) {
                function_builder.ins().return_(&[]);
            } else {
                let default_value = function_builder
                    .ins()
                    .iconst(cranelift_type_for(&method_declaration.return_type), 0);
                function_builder.ins().return_(&[default_value]);
            }
        }

        function_builder.finalize();
    }

    state
        .module
        .define_function(method_id, &mut context)
        .map_err(|error| {
            build_failed(
                format!(
                    "failed to define method '{}::{}': {error}",
                    struct_declaration.struct_reference.symbol_name, method_declaration.name
                ),
                None,
            )
        })?;
    state.module.clear_context(&mut context);

    Ok(())
}

fn define_process_entrypoint(
    state: &mut CompilationState<'_>,
    entrypoint_callable_reference: &ExecutableCallableReference,
) -> Result<(), CompilerFailure> {
    let entrypoint_id = state
        .function_record_by_callable_reference
        .get(entrypoint_callable_reference)
        .ok_or_else(|| {
            build_failed(
                format!(
                    "missing entrypoint function '{}::{}'",
                    entrypoint_callable_reference.package_path,
                    entrypoint_callable_reference.symbol_name
                ),
                None,
            )
        })?
        .id;

    let mut signature = state.module.make_signature();
    signature.returns.push(AbiParam::new(types::I32));

    let main_id = state
        .module
        .declare_function("main", Linkage::Export, &signature)
        .map_err(|error| build_failed(format!("failed to declare exported main: {error}"), None))?;

    let mut context = state.module.make_context();
    context.func.signature = signature;

    let mut function_builder_context = FunctionBuilderContext::new();
    {
        let mut function_builder =
            FunctionBuilder::new(&mut context.func, &mut function_builder_context);
        let entry_block = function_builder.create_block();
        function_builder.switch_to_block(entry_block);
        function_builder.seal_block(entry_block);

        let entrypoint = state
            .module
            .declare_func_in_func(entrypoint_id, function_builder.func);
        let _ = function_builder.ins().call(entrypoint, &[]);

        let zero = function_builder.ins().iconst(types::I32, 0);
        function_builder.ins().return_(&[zero]);
        function_builder.finalize();
    }

    state
        .module
        .define_function(main_id, &mut context)
        .map_err(|error| build_failed(format!("failed to define exported main: {error}"), None))?;
    state.module.clear_context(&mut context);

    Ok(())
}

fn compile_statements(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    statements: &[ExecutableStatement],
    function_return_type: &ExecutableTypeReference,
) -> Result<bool, CompilerFailure> {
    for statement in statements {
        match statement {
            ExecutableStatement::Binding {
                name, initializer, ..
            } => {
                let initializer =
                    compile_expression(state, function_builder, compilation_context, initializer)?;
                if initializer.terminates {
                    return Ok(true);
                }
                let Some(value) = initializer.value else {
                    return Err(build_failed(
                        format!("initializer for '{name}' produced no runtime value"),
                        None,
                    ));
                };
                let local_value =
                    declare_local_variable(function_builder, value, initializer.type_reference);
                compilation_context
                    .local_value_by_name
                    .insert(name.clone(), local_value);
            }
            ExecutableStatement::Assign { target, value } => match target {
                ExecutableAssignTarget::Name { name } => {
                    let (local_variable, local_type_reference) = {
                        let local_value = compilation_context
                            .local_value_by_name
                            .get(name)
                            .ok_or_else(|| build_failed(format!("unknown local '{name}'"), None))?;
                        (local_value.variable, local_value.type_reference.clone())
                    };
                    let assigned_value =
                        compile_expression(state, function_builder, compilation_context, value)?;
                    if assigned_value.terminates {
                        return Ok(true);
                    }
                    if local_type_reference != assigned_value.type_reference {
                        return Err(build_failed(
                            format!("assignment type mismatch for local '{name}'"),
                            None,
                        ));
                    }
                    let Some(value) = assigned_value.value else {
                        return Err(build_failed(
                            format!("assignment value for '{name}' produced no runtime value"),
                            None,
                        ));
                    };
                    function_builder.def_var(local_variable, value);
                }
                ExecutableAssignTarget::Index { target, index } => {
                    compile_index_assign_statement(
                        state,
                        function_builder,
                        compilation_context,
                        target,
                        index,
                        value,
                    )?;
                }
            },
            ExecutableStatement::If {
                condition,
                then_statements,
                else_statements,
            } => {
                let condition_typed_value =
                    compile_expression(state, function_builder, compilation_context, condition)?;
                if condition_typed_value.terminates {
                    return Ok(true);
                }
                if condition_typed_value.type_reference != ExecutableTypeReference::Boolean {
                    return Err(build_failed(
                        "if condition must be boolean".to_string(),
                        None,
                    ));
                }
                let condition_value = condition_typed_value.value.ok_or_else(|| {
                    build_failed("if condition produced no runtime value".to_string(), None)
                })?;
                let zero = function_builder.ins().iconst(types::I8, 0);
                let condition_is_true =
                    function_builder
                        .ins()
                        .icmp(IntCC::NotEqual, condition_value, zero);

                let then_block = function_builder.create_block();
                let else_block = function_builder.create_block();
                let merge_block = function_builder.create_block();

                function_builder
                    .ins()
                    .brif(condition_is_true, then_block, &[], else_block, &[]);

                function_builder.switch_to_block(then_block);
                let then_terminated = compile_statements(
                    state,
                    function_builder,
                    compilation_context,
                    then_statements,
                    function_return_type,
                )?;
                if !then_terminated {
                    function_builder.ins().jump(merge_block, &[]);
                }
                function_builder.seal_block(then_block);

                function_builder.switch_to_block(else_block);
                let else_terminated = if let Some(else_statements) = else_statements {
                    compile_statements(
                        state,
                        function_builder,
                        compilation_context,
                        else_statements,
                        function_return_type,
                    )?
                } else {
                    false
                };
                if !else_terminated {
                    function_builder.ins().jump(merge_block, &[]);
                }
                function_builder.seal_block(else_block);

                if then_terminated && else_terminated {
                    return Ok(true);
                }

                function_builder.switch_to_block(merge_block);
                function_builder.seal_block(merge_block);
            }
            ExecutableStatement::For {
                condition,
                body_statements,
            } => {
                let header_block = function_builder.create_block();
                let body_block = function_builder.create_block();
                let exit_block = function_builder.create_block();

                function_builder.ins().jump(header_block, &[]);

                function_builder.switch_to_block(header_block);
                if let Some(condition) = condition {
                    let condition_typed_value = compile_expression(
                        state,
                        function_builder,
                        compilation_context,
                        condition,
                    )?;
                    if condition_typed_value.terminates {
                        return Ok(true);
                    }
                    if condition_typed_value.type_reference != ExecutableTypeReference::Boolean {
                        return Err(build_failed(
                            "for condition must be boolean".to_string(),
                            None,
                        ));
                    }
                    let condition_value = condition_typed_value.value.ok_or_else(|| {
                        build_failed("for condition produced no runtime value".to_string(), None)
                    })?;
                    let zero = function_builder.ins().iconst(types::I8, 0);
                    let condition_is_true =
                        function_builder
                            .ins()
                            .icmp(IntCC::NotEqual, condition_value, zero);
                    function_builder.ins().brif(
                        condition_is_true,
                        body_block,
                        &[],
                        exit_block,
                        &[],
                    );
                } else {
                    function_builder.ins().jump(body_block, &[]);
                }

                function_builder.switch_to_block(body_block);
                let previous_loop_context = compilation_context.loop_context;
                compilation_context.loop_context = Some(LoopContext {
                    header_block,
                    exit_block,
                });
                let body_terminated = compile_statements(
                    state,
                    function_builder,
                    compilation_context,
                    body_statements,
                    function_return_type,
                )?;
                compilation_context.loop_context = previous_loop_context;
                if !body_terminated {
                    function_builder.ins().jump(header_block, &[]);
                }
                function_builder.seal_block(body_block);
                function_builder.seal_block(header_block);

                function_builder.switch_to_block(exit_block);
                function_builder.seal_block(exit_block);
            }
            ExecutableStatement::Break => {
                let Some(loop_context) = compilation_context.loop_context else {
                    return Err(build_failed("break used outside loop".to_string(), None));
                };
                function_builder.ins().jump(loop_context.exit_block, &[]);
                return Ok(true);
            }
            ExecutableStatement::Continue => {
                let Some(loop_context) = compilation_context.loop_context else {
                    return Err(build_failed("continue used outside loop".to_string(), None));
                };
                function_builder.ins().jump(loop_context.header_block, &[]);
                return Ok(true);
            }
            ExecutableStatement::Expression { expression } => {
                let typed_expression =
                    compile_expression(state, function_builder, compilation_context, expression)?;
                if typed_expression.terminates {
                    return Ok(true);
                }
            }
            ExecutableStatement::Return { value } => {
                let typed_return =
                    compile_expression(state, function_builder, compilation_context, value)?;
                if typed_return.terminates {
                    return Ok(true);
                }
                if !is_type_assignable(state, &typed_return.type_reference, function_return_type)
                    && typed_return.type_reference != ExecutableTypeReference::Never
                {
                    return Err(build_failed(
                        "return expression type mismatch".to_string(),
                        None,
                    ));
                }

                if matches!(
                    function_return_type,
                    ExecutableTypeReference::Nil | ExecutableTypeReference::Never
                ) {
                    function_builder.ins().return_(&[]);
                } else {
                    let return_value = runtime_value_for_expected_type(
                        state,
                        function_builder,
                        typed_return.value,
                        &typed_return.type_reference,
                        function_return_type,
                    )?;
                    let Some(value) = return_value else {
                        return Err(build_failed(
                            "non-nil return produced no runtime value".to_string(),
                            None,
                        ));
                    };
                    function_builder.ins().return_(&[value]);
                }

                return Ok(true);
            }
        }
    }

    Ok(false)
}

fn compile_expression(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    expression: &ExecutableExpression,
) -> Result<TypedValue, CompilerFailure> {
    match expression {
        ExecutableExpression::IntegerLiteral { value } => Ok(TypedValue {
            value: Some(function_builder.ins().iconst(types::I64, *value)),
            type_reference: ExecutableTypeReference::Int64,
            terminates: false,
        }),
        ExecutableExpression::BooleanLiteral { value } => Ok(TypedValue {
            value: Some(function_builder.ins().iconst(types::I8, i64::from(*value))),
            type_reference: ExecutableTypeReference::Boolean,
            terminates: false,
        }),
        ExecutableExpression::NilLiteral => Ok(TypedValue {
            value: None,
            type_reference: ExecutableTypeReference::Nil,
            terminates: false,
        }),
        ExecutableExpression::StringLiteral { value } => Ok(TypedValue {
            value: Some(intern_string_literal(state, function_builder, value)?),
            type_reference: ExecutableTypeReference::String,
            terminates: false,
        }),
        ExecutableExpression::ListLiteral {
            elements,
            element_type,
        } => compile_list_literal_expression(
            state,
            function_builder,
            compilation_context,
            elements,
            element_type,
        ),
        ExecutableExpression::Identifier {
            name,
            constant_reference,
            callable_reference,
            type_reference: resolved_type_reference,
        } => {
            if let Some(local_value) = compilation_context.local_value_by_name.get(name).cloned() {
                let local_runtime_value = Some(function_builder.use_var(local_value.variable));
                let lowered_value = runtime_value_for_expected_type(
                    state,
                    function_builder,
                    local_runtime_value,
                    &local_value.type_reference,
                    resolved_type_reference,
                )?;
                return Ok(TypedValue {
                    value: lowered_value,
                    type_reference: resolved_type_reference.clone(),
                    terminates: false,
                });
            }

            if let Some(constant_reference) = constant_reference {
                let constant_declaration = state
                    .constant_declaration_by_reference
                    .get(constant_reference)
                    .copied()
                    .ok_or_else(|| {
                        build_failed(
                            format!(
                                "unknown constant '{}::{}'",
                                constant_reference.package_path, constant_reference.symbol_name
                            ),
                            None,
                        )
                    })?;
                let constant_value = compile_expression(
                    state,
                    function_builder,
                    compilation_context,
                    &constant_declaration.initializer,
                )?;
                if constant_value.terminates {
                    return Ok(constant_value);
                }
                if !is_type_assignable(
                    state,
                    &constant_value.type_reference,
                    &constant_declaration.type_reference,
                ) {
                    return Err(build_failed(
                        format!(
                            "constant '{}::{}' initializer type mismatch",
                            constant_reference.package_path, constant_reference.symbol_name
                        ),
                        None,
                    ));
                }
                return Ok(TypedValue {
                    value: runtime_value_for_expected_type(
                        state,
                        function_builder,
                        constant_value.value,
                        &constant_declaration.type_reference,
                        resolved_type_reference,
                    )?,
                    type_reference: resolved_type_reference.clone(),
                    terminates: false,
                });
            }

            if let Some(callable_reference) = callable_reference {
                let function_record = state
                    .function_record_by_callable_reference
                    .get(callable_reference)
                    .ok_or_else(|| {
                        build_failed(
                            format!(
                                "unknown function '{}::{}'",
                                callable_reference.package_path, callable_reference.symbol_name
                            ),
                            None,
                        )
                    })?;
                if !function_record.type_parameter_names.is_empty() {
                    return Err(build_failed(
                        format!(
                            "generic function '{}::{}' cannot be used as a runtime function value",
                            callable_reference.package_path, callable_reference.symbol_name
                        ),
                        None,
                    ));
                }
                let function_reference = state
                    .module
                    .declare_func_in_func(function_record.id, function_builder.func);
                let function_pointer = function_builder
                    .ins()
                    .func_addr(types::I64, function_reference);
                let function_type_reference = ExecutableTypeReference::Function {
                    parameter_types: function_record.parameter_types.clone(),
                    return_type: Box::new(function_record.return_type.clone()),
                };
                return Ok(TypedValue {
                    value: runtime_value_for_expected_type(
                        state,
                        function_builder,
                        Some(function_pointer),
                        &function_type_reference,
                        resolved_type_reference,
                    )?,
                    type_reference: resolved_type_reference.clone(),
                    terminates: false,
                });
            }

            Err(build_failed(format!("unknown local '{name}'"), None))
        }
        ExecutableExpression::EnumVariantLiteral {
            enum_variant_reference,
            type_reference,
        } => Ok(TypedValue {
            value: Some(
                function_builder
                    .ins()
                    .iconst(types::I64, enum_variant_tag(enum_variant_reference)),
            ),
            type_reference: type_reference.clone(),
            terminates: false,
        }),
        ExecutableExpression::StructLiteral {
            struct_reference,
            type_reference,
            fields,
        } => compile_struct_literal_expression(
            state,
            function_builder,
            compilation_context,
            struct_reference,
            type_reference,
            fields,
        ),
        ExecutableExpression::FieldAccess { target, field } => compile_field_access_expression(
            state,
            function_builder,
            compilation_context,
            target,
            field,
        ),
        ExecutableExpression::IndexAccess { target, index } => compile_index_access_expression(
            state,
            function_builder,
            compilation_context,
            target,
            index,
        ),
        ExecutableExpression::Unary {
            operator,
            expression,
        } => {
            let operand =
                compile_expression(state, function_builder, compilation_context, expression)?;
            if operand.terminates {
                return Ok(operand);
            }
            let operand_value = operand.value.ok_or_else(|| {
                build_failed(
                    "unary operator operand produced no runtime value".to_string(),
                    None,
                )
            })?;
            match operator {
                ExecutableUnaryOperator::Not => {
                    if operand.type_reference != ExecutableTypeReference::Boolean {
                        return Err(build_failed(
                            "not operator requires boolean operand".to_string(),
                            None,
                        ));
                    }
                    let one = function_builder.ins().iconst(types::I8, 1);
                    let negated = function_builder.ins().bxor(operand_value, one);
                    Ok(TypedValue {
                        value: Some(negated),
                        type_reference: ExecutableTypeReference::Boolean,
                        terminates: false,
                    })
                }
                ExecutableUnaryOperator::Negate => {
                    if operand.type_reference != ExecutableTypeReference::Int64 {
                        return Err(build_failed(
                            "negate operator requires int64 operand".to_string(),
                            None,
                        ));
                    }
                    Ok(TypedValue {
                        value: Some(function_builder.ins().ineg(operand_value)),
                        type_reference: ExecutableTypeReference::Int64,
                        terminates: false,
                    })
                }
            }
        }
        ExecutableExpression::Binary {
            operator,
            left,
            right,
        } => compile_binary_expression(
            state,
            function_builder,
            compilation_context,
            *operator,
            left,
            right,
        ),
        ExecutableExpression::Call {
            callee,
            call_target,
            arguments,
            type_arguments,
            ..
        } => compile_call_expression(
            state,
            function_builder,
            compilation_context,
            callee,
            call_target.as_ref(),
            arguments,
            type_arguments,
        ),
        ExecutableExpression::Matches {
            value,
            type_reference,
        } => compile_matches_expression(
            state,
            function_builder,
            compilation_context,
            value,
            type_reference,
        ),
        ExecutableExpression::Match { target, arms } => {
            compile_match_expression(state, function_builder, compilation_context, target, arms)
        }
    }
}

fn compile_binary_expression(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    operator: ExecutableBinaryOperator,
    left: &ExecutableExpression,
    right: &ExecutableExpression,
) -> Result<TypedValue, CompilerFailure> {
    let left_typed_value = compile_expression(state, function_builder, compilation_context, left)?;
    if left_typed_value.terminates {
        return Ok(left_typed_value);
    }
    let right_typed_value =
        compile_expression(state, function_builder, compilation_context, right)?;
    if right_typed_value.terminates {
        return Ok(right_typed_value);
    }

    match operator {
        ExecutableBinaryOperator::Add => {
            let left_value = left_typed_value.value.ok_or_else(|| {
                build_failed(
                    "binary left operand produced no runtime value".to_string(),
                    None,
                )
            })?;
            let right_value = right_typed_value.value.ok_or_else(|| {
                build_failed(
                    "binary right operand produced no runtime value".to_string(),
                    None,
                )
            })?;
            match (
                &left_typed_value.type_reference,
                &right_typed_value.type_reference,
            ) {
                (ExecutableTypeReference::Int64, ExecutableTypeReference::Int64) => {
                    Ok(TypedValue {
                        value: Some(function_builder.ins().iadd(left_value, right_value)),
                        type_reference: ExecutableTypeReference::Int64,
                        terminates: false,
                    })
                }
                (ExecutableTypeReference::String, ExecutableTypeReference::String) => {
                    let concatenated =
                        concatenate_strings(state, function_builder, left_value, right_value);
                    Ok(TypedValue {
                        value: Some(concatenated),
                        type_reference: ExecutableTypeReference::String,
                        terminates: false,
                    })
                }
                _ => Err(build_failed(
                    "operator '+' requires operands of the same type".to_string(),
                    None,
                )),
            }
        }
        ExecutableBinaryOperator::Subtract
        | ExecutableBinaryOperator::Multiply
        | ExecutableBinaryOperator::Divide
        | ExecutableBinaryOperator::Modulo
        | ExecutableBinaryOperator::LessThan
        | ExecutableBinaryOperator::LessThanOrEqual
        | ExecutableBinaryOperator::GreaterThan
        | ExecutableBinaryOperator::GreaterThanOrEqual => {
            let left_value = left_typed_value.value.ok_or_else(|| {
                build_failed(
                    "binary left operand produced no runtime value".to_string(),
                    None,
                )
            })?;
            let right_value = right_typed_value.value.ok_or_else(|| {
                build_failed(
                    "binary right operand produced no runtime value".to_string(),
                    None,
                )
            })?;
            if left_typed_value.type_reference != ExecutableTypeReference::Int64
                || right_typed_value.type_reference != ExecutableTypeReference::Int64
            {
                return Err(build_failed(
                    "arithmetic and ordered comparison operators require int64 operands"
                        .to_string(),
                    None,
                ));
            }

            match operator {
                ExecutableBinaryOperator::Subtract => Ok(TypedValue {
                    value: Some(function_builder.ins().isub(left_value, right_value)),
                    type_reference: ExecutableTypeReference::Int64,
                    terminates: false,
                }),
                ExecutableBinaryOperator::Multiply => Ok(TypedValue {
                    value: Some(function_builder.ins().imul(left_value, right_value)),
                    type_reference: ExecutableTypeReference::Int64,
                    terminates: false,
                }),
                ExecutableBinaryOperator::Divide => Ok(TypedValue {
                    value: Some(function_builder.ins().sdiv(left_value, right_value)),
                    type_reference: ExecutableTypeReference::Int64,
                    terminates: false,
                }),
                ExecutableBinaryOperator::Modulo => Ok(TypedValue {
                    value: Some(function_builder.ins().srem(left_value, right_value)),
                    type_reference: ExecutableTypeReference::Int64,
                    terminates: false,
                }),
                ExecutableBinaryOperator::LessThan
                | ExecutableBinaryOperator::LessThanOrEqual
                | ExecutableBinaryOperator::GreaterThan
                | ExecutableBinaryOperator::GreaterThanOrEqual => {
                    let condition_code = match operator {
                        ExecutableBinaryOperator::LessThan => IntCC::SignedLessThan,
                        ExecutableBinaryOperator::LessThanOrEqual => IntCC::SignedLessThanOrEqual,
                        ExecutableBinaryOperator::GreaterThan => IntCC::SignedGreaterThan,
                        ExecutableBinaryOperator::GreaterThanOrEqual => {
                            IntCC::SignedGreaterThanOrEqual
                        }
                        _ => unreachable!(),
                    };
                    let condition =
                        function_builder
                            .ins()
                            .icmp(condition_code, left_value, right_value);
                    let one = function_builder.ins().iconst(types::I8, 1);
                    let zero = function_builder.ins().iconst(types::I8, 0);
                    let bool_value = function_builder.ins().select(condition, one, zero);
                    Ok(TypedValue {
                        value: Some(bool_value),
                        type_reference: ExecutableTypeReference::Boolean,
                        terminates: false,
                    })
                }
                _ => unreachable!(),
            }
        }
        ExecutableBinaryOperator::EqualEqual | ExecutableBinaryOperator::NotEqual => {
            let comparable_type_reference = comparable_type_reference_for_equality(
                state,
                &left_typed_value.type_reference,
                &right_typed_value.type_reference,
            )
            .ok_or_else(|| {
                build_failed("equality operators require same type".to_string(), None)
            })?;
            let lowered_left_value = runtime_value_for_expected_type(
                state,
                function_builder,
                left_typed_value.value,
                &left_typed_value.type_reference,
                &comparable_type_reference,
            )?;
            let lowered_right_value = runtime_value_for_expected_type(
                state,
                function_builder,
                right_typed_value.value,
                &right_typed_value.type_reference,
                &comparable_type_reference,
            )?;

            let (left_value, right_value) = match (lowered_left_value, lowered_right_value) {
                (Some(left_value), Some(right_value)) => (left_value, right_value),
                (None, None)
                    if matches!(
                        comparable_type_reference,
                        ExecutableTypeReference::Nil | ExecutableTypeReference::Never
                    ) =>
                {
                    let bool_value = if matches!(operator, ExecutableBinaryOperator::EqualEqual) {
                        function_builder.ins().iconst(types::I8, 1)
                    } else {
                        function_builder.ins().iconst(types::I8, 0)
                    };
                    return Ok(TypedValue {
                        value: Some(bool_value),
                        type_reference: ExecutableTypeReference::Boolean,
                        terminates: false,
                    });
                }
                _ => {
                    return Err(build_failed(
                        "equality operand conversion produced no runtime value".to_string(),
                        None,
                    ));
                }
            };
            let condition_code = if matches!(operator, ExecutableBinaryOperator::EqualEqual) {
                IntCC::Equal
            } else {
                IntCC::NotEqual
            };
            let condition = if matches!(
                comparable_type_reference,
                ExecutableTypeReference::Union { .. }
            ) {
                let left_tag = function_builder.ins().load(
                    types::I64,
                    MemFlags::new(),
                    left_value,
                    UNION_BOX_TAG_OFFSET,
                );
                let right_tag = function_builder.ins().load(
                    types::I64,
                    MemFlags::new(),
                    right_value,
                    UNION_BOX_TAG_OFFSET,
                );
                let left_payload = function_builder.ins().load(
                    types::I64,
                    MemFlags::new(),
                    left_value,
                    UNION_BOX_PAYLOAD_OFFSET,
                );
                let right_payload = function_builder.ins().load(
                    types::I64,
                    MemFlags::new(),
                    right_value,
                    UNION_BOX_PAYLOAD_OFFSET,
                );
                let tags_equal = function_builder
                    .ins()
                    .icmp(IntCC::Equal, left_tag, right_tag);
                let payloads_equal =
                    function_builder
                        .ins()
                        .icmp(IntCC::Equal, left_payload, right_payload);
                let equal_condition = function_builder.ins().band(tags_equal, payloads_equal);
                if matches!(operator, ExecutableBinaryOperator::EqualEqual) {
                    equal_condition
                } else {
                    let one = function_builder.ins().iconst(types::I8, 1);
                    let zero = function_builder.ins().iconst(types::I8, 0);
                    let equal_as_i8 = function_builder.ins().select(equal_condition, one, zero);
                    function_builder.ins().icmp(IntCC::Equal, equal_as_i8, zero)
                }
            } else {
                function_builder
                    .ins()
                    .icmp(condition_code, left_value, right_value)
            };
            let one = function_builder.ins().iconst(types::I8, 1);
            let zero = function_builder.ins().iconst(types::I8, 0);
            let bool_value = function_builder.ins().select(condition, one, zero);
            Ok(TypedValue {
                value: Some(bool_value),
                type_reference: ExecutableTypeReference::Boolean,
                terminates: false,
            })
        }
        ExecutableBinaryOperator::And | ExecutableBinaryOperator::Or => {
            let left_value = left_typed_value.value.ok_or_else(|| {
                build_failed(
                    "binary left operand produced no runtime value".to_string(),
                    None,
                )
            })?;
            let right_value = right_typed_value.value.ok_or_else(|| {
                build_failed(
                    "binary right operand produced no runtime value".to_string(),
                    None,
                )
            })?;
            if left_typed_value.type_reference != ExecutableTypeReference::Boolean
                || right_typed_value.type_reference != ExecutableTypeReference::Boolean
            {
                return Err(build_failed(
                    "logical operators require boolean operands".to_string(),
                    None,
                ));
            }
            let value = if matches!(operator, ExecutableBinaryOperator::And) {
                function_builder.ins().band(left_value, right_value)
            } else {
                function_builder.ins().bor(left_value, right_value)
            };
            Ok(TypedValue {
                value: Some(value),
                type_reference: ExecutableTypeReference::Boolean,
                terminates: false,
            })
        }
    }
}

fn concatenate_strings(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    left_pointer: Value,
    right_pointer: Value,
) -> Value {
    let strlen = state.module.declare_func_in_func(
        state.external_runtime_functions.strlen,
        function_builder.func,
    );
    let left_length_call = function_builder.ins().call(strlen, &[left_pointer]);
    let left_length = function_builder.inst_results(left_length_call)[0];
    let right_length_call = function_builder.ins().call(strlen, &[right_pointer]);
    let right_length = function_builder.inst_results(right_length_call)[0];

    let total_length = function_builder.ins().iadd(left_length, right_length);
    let one = function_builder.ins().iconst(types::I64, 1);
    let allocation_size = function_builder.ins().iadd(total_length, one);
    let malloc = state.module.declare_func_in_func(
        state.external_runtime_functions.malloc,
        function_builder.func,
    );
    let malloc_call = function_builder.ins().call(malloc, &[allocation_size]);
    let destination_pointer = function_builder.inst_results(malloc_call)[0];

    let memcpy = state.module.declare_func_in_func(
        state.external_runtime_functions.memcpy,
        function_builder.func,
    );
    let _ = function_builder
        .ins()
        .call(memcpy, &[destination_pointer, left_pointer, left_length]);
    let right_destination_pointer = function_builder
        .ins()
        .iadd(destination_pointer, left_length);
    let _ = function_builder.ins().call(
        memcpy,
        &[right_destination_pointer, right_pointer, right_length],
    );

    let terminator_pointer = function_builder
        .ins()
        .iadd(destination_pointer, total_length);
    let zero = function_builder.ins().iconst(types::I8, 0);
    function_builder
        .ins()
        .store(MemFlags::new(), zero, terminator_pointer, 0);

    destination_pointer
}

fn compile_call_expression(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    callee: &ExecutableExpression,
    call_target: Option<&ExecutableCallTarget>,
    arguments: &[ExecutableExpression],
    type_arguments: &[ExecutableTypeReference],
) -> Result<TypedValue, CompilerFailure> {
    if let Some(call_target) = call_target {
        return match call_target {
            ExecutableCallTarget::BuiltinFunction { function_name } => {
                if !type_arguments.is_empty() {
                    return Err(build_failed(
                        format!("builtin function '{function_name}' does not take type arguments"),
                        None,
                    ));
                }
                if function_name == PRINT_FUNCTION_CONTRACT.language_name {
                    if arguments.len() != 1 {
                        return Err(build_failed(
                            "print(...) requires exactly one argument".to_string(),
                            None,
                        ));
                    }
                    let argument = compile_expression(
                        state,
                        function_builder,
                        compilation_context,
                        &arguments[0],
                    )?;
                    if argument.terminates {
                        return Ok(argument);
                    }
                    if argument.type_reference != ExecutableTypeReference::String {
                        return Err(build_failed(
                            "print(...) requires string argument".to_string(),
                            None,
                        ));
                    }
                    let pointer = argument.value.ok_or_else(|| {
                        build_failed("print argument produced no runtime value".to_string(), None)
                    })?;
                    emit_write_string_with_newline(state, function_builder, 1, pointer)?;
                    return Ok(TypedValue {
                        value: None,
                        type_reference: ExecutableTypeReference::Nil,
                        terminates: false,
                    });
                }

                if function_name == ABORT_FUNCTION_CONTRACT.language_name {
                    if arguments.len() != 1 {
                        return Err(build_failed(
                            "abort(...) requires exactly one argument".to_string(),
                            None,
                        ));
                    }
                    let argument = compile_expression(
                        state,
                        function_builder,
                        compilation_context,
                        &arguments[0],
                    )?;
                    if argument.terminates {
                        return Ok(argument);
                    }
                    if argument.type_reference != ExecutableTypeReference::String {
                        return Err(build_failed(
                            "abort(...) requires string argument".to_string(),
                            None,
                        ));
                    }
                    let pointer = argument.value.ok_or_else(|| {
                        build_failed("abort argument produced no runtime value".to_string(), None)
                    })?;
                    emit_write_string_with_newline(state, function_builder, 2, pointer)?;
                    emit_exit_call(state, function_builder, 1);
                    return Ok(TypedValue {
                        value: None,
                        type_reference: ExecutableTypeReference::Never,
                        terminates: true,
                    });
                }
                if function_name == ASSERT_FUNCTION_CONTRACT.language_name {
                    if arguments.len() != 1 {
                        return Err(build_failed(
                            "assert(...) requires exactly one argument".to_string(),
                            None,
                        ));
                    }
                    let argument = compile_expression(
                        state,
                        function_builder,
                        compilation_context,
                        &arguments[0],
                    )?;
                    if argument.terminates {
                        return Ok(argument);
                    }
                    if argument.type_reference != ExecutableTypeReference::Boolean {
                        return Err(build_failed(
                            "assert(...) requires boolean argument".to_string(),
                            None,
                        ));
                    }
                    let condition_value = argument.value.ok_or_else(|| {
                        build_failed(
                            "assert argument produced no runtime value".to_string(),
                            None,
                        )
                    })?;
                    let zero = function_builder.ins().iconst(types::I8, 0);
                    let condition_is_true =
                        function_builder
                            .ins()
                            .icmp(IntCC::NotEqual, condition_value, zero);

                    let pass_block = function_builder.create_block();
                    let fail_block = function_builder.create_block();
                    let merge_block = function_builder.create_block();
                    function_builder.ins().brif(
                        condition_is_true,
                        pass_block,
                        &[],
                        fail_block,
                        &[],
                    );

                    function_builder.switch_to_block(fail_block);
                    let message_pointer =
                        intern_string_literal(state, function_builder, "assertion failed")?;
                    emit_write_string_with_newline(state, function_builder, 2, message_pointer)?;
                    emit_exit_call(state, function_builder, 1);
                    function_builder.seal_block(fail_block);

                    function_builder.switch_to_block(pass_block);
                    function_builder.ins().jump(merge_block, &[]);
                    function_builder.seal_block(pass_block);

                    function_builder.switch_to_block(merge_block);
                    function_builder.seal_block(merge_block);
                    return Ok(TypedValue {
                        value: None,
                        type_reference: ExecutableTypeReference::Nil,
                        terminates: false,
                    });
                }

                if let Some(conversion_result) = compile_builtin_conversion_call(
                    state,
                    function_builder,
                    compilation_context,
                    function_name,
                    arguments,
                )? {
                    return Ok(conversion_result);
                }

                Err(build_failed(
                    format!("unknown builtin function '{function_name}'"),
                    None,
                ))
            }
            ExecutableCallTarget::UserDefinedFunction { callable_reference } => {
                let function_record = state
                    .function_record_by_callable_reference
                    .get(callable_reference)
                    .ok_or_else(|| {
                        build_failed(
                            format!(
                                "unknown function '{}::{}'",
                                callable_reference.package_path, callable_reference.symbol_name
                            ),
                            None,
                        )
                    })?;
                let function_id = function_record.id;
                let declared_parameter_types = function_record.parameter_types.clone();
                let declared_return_type = function_record.return_type.clone();
                let type_parameter_names = function_record.type_parameter_names.clone();
                let type_parameter_constraint_interface_reference_by_name =
                    if type_parameter_names.is_empty() {
                        BTreeMap::new()
                    } else {
                        function_record
                            .type_parameter_constraint_interface_reference_by_name
                            .clone()
                    };
                let (instantiated_parameter_types, instantiated_return_type) =
                    instantiate_generic_signature(
                        &type_parameter_names,
                        &declared_parameter_types,
                        &declared_return_type,
                        type_arguments,
                    )?;

                if instantiated_parameter_types.len() != arguments.len() {
                    return Err(build_failed(
                        format!(
                            "function '{}::{}' expected {} argument(s), got {}",
                            callable_reference.package_path,
                            callable_reference.symbol_name,
                            instantiated_parameter_types.len(),
                            arguments.len()
                        ),
                        None,
                    ));
                }

                let mut argument_values = Vec::new();
                for ((instantiated_parameter_type, declared_parameter_type), argument_expression) in
                    instantiated_parameter_types
                        .iter()
                        .zip(declared_parameter_types.iter())
                        .zip(arguments)
                {
                    let argument = compile_expression(
                        state,
                        function_builder,
                        compilation_context,
                        argument_expression,
                    )?;
                    if argument.terminates {
                        return Ok(argument);
                    }
                    if !is_type_assignable(
                        state,
                        &argument.type_reference,
                        instantiated_parameter_type,
                    ) {
                        return Err(build_failed(
                            format!(
                                "call argument type mismatch for function '{}::{}'",
                                callable_reference.package_path, callable_reference.symbol_name
                            ),
                            None,
                        ));
                    }
                    let lowered_argument = runtime_call_argument_for_declared_parameter_type(
                        state,
                        function_builder,
                        argument.value,
                        &argument.type_reference,
                        instantiated_parameter_type,
                        declared_parameter_type,
                    )?;
                    argument_values.push(lowered_argument);
                }
                for (type_parameter_index, type_parameter_name) in
                    type_parameter_names.iter().enumerate()
                {
                    let Some(interface_reference) =
                        type_parameter_constraint_interface_reference_by_name
                            .get(type_parameter_name)
                    else {
                        continue;
                    };
                    let type_argument =
                        type_arguments
                            .get(type_parameter_index)
                            .ok_or_else(|| {
                                build_failed(
                                    format!(
                                        "missing type argument for constrained type parameter '{type_parameter_name}'"
                                    ),
                                    None,
                                )
                            })?;
                    let witness_table_pointer = build_witness_table_for_constraint(
                        state,
                        function_builder,
                        type_argument,
                        interface_reference,
                    )?;
                    argument_values.push(witness_table_pointer);
                }

                let callee = state
                    .module
                    .declare_func_in_func(function_id, function_builder.func);
                let call = function_builder.ins().call(callee, &argument_values);

                if matches!(
                    instantiated_return_type,
                    ExecutableTypeReference::Nil | ExecutableTypeReference::Never
                ) {
                    let return_terminates =
                        matches!(&instantiated_return_type, ExecutableTypeReference::Never);
                    Ok(TypedValue {
                        value: None,
                        type_reference: instantiated_return_type,
                        terminates: return_terminates,
                    })
                } else {
                    let results = function_builder.inst_results(call);
                    let lowered_result = runtime_call_result_for_instantiated_return_type(
                        function_builder,
                        results[0],
                        &declared_return_type,
                        &instantiated_return_type,
                    );
                    Ok(TypedValue {
                        value: Some(lowered_result),
                        type_reference: instantiated_return_type,
                        terminates: false,
                    })
                }
            }
        };
    }

    match callee {
        ExecutableExpression::FieldAccess { .. } => compile_method_call_expression(
            state,
            function_builder,
            compilation_context,
            callee,
            arguments,
        ),
        _ => compile_function_value_call_expression(
            state,
            function_builder,
            compilation_context,
            callee,
            arguments,
        ),
    }
}

fn runtime_call_argument_for_declared_parameter_type(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    argument_value: Option<Value>,
    argument_type: &ExecutableTypeReference,
    instantiated_parameter_type: &ExecutableTypeReference,
    declared_parameter_type: &ExecutableTypeReference,
) -> Result<Value, CompilerFailure> {
    if matches!(
        declared_parameter_type,
        ExecutableTypeReference::TypeParameter { .. }
    ) {
        if matches!(argument_type, ExecutableTypeReference::Nil) {
            return Ok(function_builder.ins().iconst(types::I64, 0));
        }
        let runtime_value = argument_value.ok_or_else(|| {
            build_failed("call argument produced no runtime value".to_string(), None)
        })?;
        return Ok(i64_storage_value_for_type(
            function_builder,
            runtime_value,
            instantiated_parameter_type,
        ));
    }

    let lowered_argument = runtime_value_for_expected_type(
        state,
        function_builder,
        argument_value,
        argument_type,
        declared_parameter_type,
    )?;
    lowered_argument
        .ok_or_else(|| build_failed("call argument produced no runtime value".to_string(), None))
}

fn runtime_call_result_for_instantiated_return_type(
    function_builder: &mut FunctionBuilder<'_>,
    raw_result: Value,
    declared_return_type: &ExecutableTypeReference,
    instantiated_return_type: &ExecutableTypeReference,
) -> Value {
    if matches!(
        declared_return_type,
        ExecutableTypeReference::TypeParameter { .. }
    ) {
        return runtime_value_from_i64_storage(
            function_builder,
            raw_result,
            instantiated_return_type,
        );
    }
    raw_result
}

fn compile_function_value_call_expression(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    callee: &ExecutableExpression,
    arguments: &[ExecutableExpression],
) -> Result<TypedValue, CompilerFailure> {
    let compiled_callee = compile_expression(state, function_builder, compilation_context, callee)?;
    if compiled_callee.terminates {
        return Ok(compiled_callee);
    }
    let ExecutableTypeReference::Function {
        parameter_types,
        return_type,
    } = &compiled_callee.type_reference
    else {
        return Err(build_failed(
            format!(
                "cannot call non-function value of type {}",
                type_reference_display(&compiled_callee.type_reference)
            ),
            None,
        ));
    };
    if parameter_types.len() != arguments.len() {
        return Err(build_failed(
            format!(
                "function value expected {} argument(s), got {}",
                parameter_types.len(),
                arguments.len()
            ),
            None,
        ));
    }
    let function_pointer = compiled_callee.value.ok_or_else(|| {
        build_failed(
            "function callee produced no runtime value".to_string(),
            None,
        )
    })?;

    let mut call_values = Vec::with_capacity(arguments.len());
    for (expected_type, argument_expression) in parameter_types.iter().zip(arguments) {
        let compiled_argument = compile_expression(
            state,
            function_builder,
            compilation_context,
            argument_expression,
        )?;
        if compiled_argument.terminates {
            return Ok(compiled_argument);
        }
        if !is_type_assignable(state, &compiled_argument.type_reference, expected_type) {
            return Err(build_failed(
                format!(
                    "function argument type mismatch: expected {}, got {}",
                    type_reference_display(expected_type),
                    type_reference_display(&compiled_argument.type_reference)
                ),
                None,
            ));
        }
        let lowered_argument = runtime_value_for_expected_type(
            state,
            function_builder,
            compiled_argument.value,
            &compiled_argument.type_reference,
            expected_type,
        )?;
        let value = lowered_argument.ok_or_else(|| {
            build_failed(
                "function argument produced no runtime value".to_string(),
                None,
            )
        })?;
        call_values.push(value);
    }

    let mut call_signature = state.module.make_signature();
    for parameter_type in parameter_types {
        call_signature
            .params
            .push(AbiParam::new(cranelift_type_for(parameter_type)));
    }
    if !matches!(
        **return_type,
        ExecutableTypeReference::Nil | ExecutableTypeReference::Never
    ) {
        call_signature
            .returns
            .push(AbiParam::new(cranelift_type_for(return_type)));
    }
    let signature_reference = function_builder.import_signature(call_signature);
    let call =
        function_builder
            .ins()
            .call_indirect(signature_reference, function_pointer, &call_values);
    if matches!(
        **return_type,
        ExecutableTypeReference::Nil | ExecutableTypeReference::Never
    ) {
        Ok(TypedValue {
            value: None,
            type_reference: (**return_type).clone(),
            terminates: matches!(**return_type, ExecutableTypeReference::Never),
        })
    } else {
        Ok(TypedValue {
            value: Some(function_builder.inst_results(call)[0]),
            type_reference: (**return_type).clone(),
            terminates: false,
        })
    }
}

fn compile_builtin_conversion_call(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    function_name: &str,
    arguments: &[ExecutableExpression],
) -> Result<Option<TypedValue>, CompilerFailure> {
    if function_name != "string" {
        return Ok(None);
    }
    if arguments.len() != 1 {
        return Err(build_failed(
            format!("{function_name}(...) requires exactly one argument"),
            None,
        ));
    }

    let argument = compile_expression(state, function_builder, compilation_context, &arguments[0])?;
    if argument.terminates {
        return Ok(Some(argument));
    }

    let converted = match function_name {
        "string" => match &argument.type_reference {
            ExecutableTypeReference::Int64 => {
                let value = argument.value.ok_or_else(|| {
                    build_failed(
                        "int64 conversion argument produced no runtime value".to_string(),
                        None,
                    )
                })?;
                TypedValue {
                    value: Some(convert_int64_to_string(state, function_builder, value)?),
                    type_reference: ExecutableTypeReference::String,
                    terminates: false,
                }
            }
            ExecutableTypeReference::Boolean => {
                let value = argument.value.ok_or_else(|| {
                    build_failed(
                        "boolean conversion argument produced no runtime value".to_string(),
                        None,
                    )
                })?;
                let true_string = intern_string_literal(state, function_builder, "true")?;
                let false_string = intern_string_literal(state, function_builder, "false")?;
                let pointer = function_builder
                    .ins()
                    .select(value, true_string, false_string);
                TypedValue {
                    value: Some(pointer),
                    type_reference: ExecutableTypeReference::String,
                    terminates: false,
                }
            }
            ExecutableTypeReference::Nil => TypedValue {
                value: Some(intern_string_literal(state, function_builder, "nil")?),
                type_reference: ExecutableTypeReference::String,
                terminates: false,
            },
            _ => {
                return Err(build_failed(
                    format!(
                        "cannot convert {} to string",
                        type_reference_display(&argument.type_reference)
                    ),
                    None,
                ));
            }
        },
        _ => {
            return Ok(None);
        }
    };

    Ok(Some(converted))
}

fn compile_index_access_expression(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    target: &ExecutableExpression,
    index: &ExecutableExpression,
) -> Result<TypedValue, CompilerFailure> {
    let compiled_target = compile_expression(state, function_builder, compilation_context, target)?;
    if compiled_target.terminates {
        return Ok(compiled_target);
    }
    let ExecutableTypeReference::List { element_type } = &compiled_target.type_reference else {
        return Err(build_failed(
            format!(
                "index access target must be List, got {}",
                type_reference_display(&compiled_target.type_reference)
            ),
            None,
        ));
    };

    let compiled_index = compile_expression(state, function_builder, compilation_context, index)?;
    if compiled_index.terminates {
        return Ok(compiled_index);
    }
    if compiled_index.type_reference != ExecutableTypeReference::Int64 {
        return Err(build_failed("list index must be int64".to_string(), None));
    }

    let list_pointer = compiled_target.value.ok_or_else(|| {
        build_failed(
            "index access target produced no runtime value".to_string(),
            None,
        )
    })?;
    let index_value = compiled_index.value.ok_or_else(|| {
        build_failed(
            "index expression produced no runtime value".to_string(),
            None,
        )
    })?;
    let list_length = function_builder.ins().load(
        types::I64,
        MemFlags::new(),
        list_pointer,
        LIST_LENGTH_OFFSET,
    );
    let list_data_pointer = function_builder.ins().load(
        types::I64,
        MemFlags::new(),
        list_pointer,
        LIST_DATA_POINTER_OFFSET,
    );

    let store_block = function_builder.create_block();
    let invalid_index_block = function_builder.create_block();
    let non_negative_block = function_builder.create_block();
    let merge_block = function_builder.create_block();
    function_builder.append_block_param(merge_block, cranelift_type_for(element_type));

    let zero_value = function_builder.ins().iconst(types::I64, 0);
    let index_is_non_negative =
        function_builder
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, index_value, zero_value);
    function_builder.ins().brif(
        index_is_non_negative,
        non_negative_block,
        &[],
        invalid_index_block,
        &[],
    );
    function_builder.seal_block(non_negative_block);

    function_builder.switch_to_block(non_negative_block);
    let index_in_range =
        function_builder
            .ins()
            .icmp(IntCC::SignedLessThan, index_value, list_length);
    function_builder
        .ins()
        .brif(index_in_range, store_block, &[], invalid_index_block, &[]);
    function_builder.seal_block(store_block);
    function_builder.seal_block(invalid_index_block);

    function_builder.switch_to_block(invalid_index_block);
    function_builder.ins().trap(TrapCode::user(3).unwrap());

    function_builder.switch_to_block(store_block);
    let element_offset = function_builder.ins().imul_imm(index_value, 8);
    let element_pointer = function_builder
        .ins()
        .iadd(list_data_pointer, element_offset);
    let loaded_storage =
        function_builder
            .ins()
            .load(types::I64, MemFlags::new(), element_pointer, 0);
    let loaded_value =
        runtime_value_from_i64_storage(function_builder, loaded_storage, element_type);
    let merge_arguments = [BlockArg::Value(loaded_value)];
    function_builder.ins().jump(merge_block, &merge_arguments);
    function_builder.seal_block(merge_block);

    function_builder.switch_to_block(merge_block);
    let value = function_builder.block_params(merge_block)[0];
    Ok(TypedValue {
        value: Some(value),
        type_reference: (**element_type).clone(),
        terminates: false,
    })
}

fn compile_index_assign_statement(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    target: &ExecutableExpression,
    index: &ExecutableExpression,
    value: &ExecutableExpression,
) -> Result<(), CompilerFailure> {
    let compiled_target = compile_expression(state, function_builder, compilation_context, target)?;
    if compiled_target.terminates {
        return Ok(());
    }
    let ExecutableTypeReference::List { element_type } = &compiled_target.type_reference else {
        return Err(build_failed(
            format!(
                "index assignment target must be List, got {}",
                type_reference_display(&compiled_target.type_reference)
            ),
            None,
        ));
    };

    let compiled_index = compile_expression(state, function_builder, compilation_context, index)?;
    if compiled_index.terminates {
        return Ok(());
    }
    if compiled_index.type_reference != ExecutableTypeReference::Int64 {
        return Err(build_failed("list index must be int64".to_string(), None));
    }

    let compiled_value = compile_expression(state, function_builder, compilation_context, value)?;
    if compiled_value.terminates {
        return Ok(());
    }
    if !is_type_assignable(state, &compiled_value.type_reference, element_type) {
        return Err(build_failed(
            format!(
                "indexed assignment type mismatch: expected {}, got {}",
                type_reference_display(element_type),
                type_reference_display(&compiled_value.type_reference)
            ),
            None,
        ));
    }

    let list_pointer = compiled_target.value.ok_or_else(|| {
        build_failed(
            "index assignment target produced no runtime value".to_string(),
            None,
        )
    })?;
    let index_value = compiled_index.value.ok_or_else(|| {
        build_failed(
            "index expression produced no runtime value".to_string(),
            None,
        )
    })?;
    let lowered_value = runtime_value_for_expected_type(
        state,
        function_builder,
        compiled_value.value,
        &compiled_value.type_reference,
        element_type,
    )?
    .ok_or_else(|| {
        build_failed(
            "indexed assignment value produced no runtime value".to_string(),
            None,
        )
    })?;
    let stored_value = i64_storage_value_for_type(function_builder, lowered_value, element_type);

    let list_length = function_builder.ins().load(
        types::I64,
        MemFlags::new(),
        list_pointer,
        LIST_LENGTH_OFFSET,
    );
    let list_data_pointer = function_builder.ins().load(
        types::I64,
        MemFlags::new(),
        list_pointer,
        LIST_DATA_POINTER_OFFSET,
    );

    let invalid_index_block = function_builder.create_block();
    let non_negative_block = function_builder.create_block();
    let store_block = function_builder.create_block();

    let zero_value = function_builder.ins().iconst(types::I64, 0);
    let index_is_non_negative =
        function_builder
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, index_value, zero_value);
    function_builder.ins().brif(
        index_is_non_negative,
        non_negative_block,
        &[],
        invalid_index_block,
        &[],
    );
    function_builder.seal_block(non_negative_block);

    function_builder.switch_to_block(non_negative_block);
    let index_in_range =
        function_builder
            .ins()
            .icmp(IntCC::SignedLessThan, index_value, list_length);
    function_builder
        .ins()
        .brif(index_in_range, store_block, &[], invalid_index_block, &[]);
    function_builder.seal_block(store_block);
    function_builder.seal_block(invalid_index_block);

    function_builder.switch_to_block(invalid_index_block);
    function_builder.ins().trap(TrapCode::user(3).unwrap());

    function_builder.switch_to_block(store_block);
    let element_offset = function_builder.ins().imul_imm(index_value, 8);
    let element_pointer = function_builder
        .ins()
        .iadd(list_data_pointer, element_offset);
    function_builder
        .ins()
        .store(MemFlags::new(), stored_value, element_pointer, 0);
    Ok(())
}

fn compile_struct_literal_expression(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    struct_reference: &ExecutableStructReference,
    type_reference: &ExecutableTypeReference,
    fields: &[compiler__executable_program::ExecutableStructLiteralField],
) -> Result<TypedValue, CompilerFailure> {
    let struct_declaration = state
        .struct_declaration_by_reference
        .get(struct_reference)
        .copied()
        .ok_or_else(|| {
            build_failed(
                format!(
                    "unknown struct '{}::{}'",
                    struct_reference.package_path, struct_reference.symbol_name
                ),
                None,
            )
        })?;
    let type_substitutions_by_type_parameter_name =
        type_substitutions_for_struct_type(struct_declaration, type_reference)?;

    let allocated_pointer = allocate_heap_bytes(
        state,
        function_builder,
        i64::try_from(struct_declaration.fields.len() * 8).map_err(|_| {
            build_failed(
                "struct literal size exceeds supported allocation range".to_string(),
                None,
            )
        })?,
    )?;
    let mem_flags = MemFlags::new();

    for (field_index, declared_field) in struct_declaration.fields.iter().enumerate() {
        let provided_field = fields
            .iter()
            .find(|field| field.name == declared_field.name)
            .ok_or_else(|| {
                build_failed(
                    format!("missing field '{}' in struct literal", declared_field.name),
                    None,
                )
            })?;
        let compiled_field = compile_expression(
            state,
            function_builder,
            compilation_context,
            &provided_field.value,
        )?;
        if compiled_field.terminates {
            return Ok(compiled_field);
        }
        let expected_type = substitute_type_reference(
            &declared_field.type_reference,
            &type_substitutions_by_type_parameter_name,
        );
        if compiled_field.type_reference != expected_type {
            return Err(build_failed(
                format!(
                    "struct field '{}' type mismatch: expected {}, got {}",
                    declared_field.name,
                    type_reference_display(&expected_type),
                    type_reference_display(&compiled_field.type_reference)
                ),
                None,
            ));
        }
        let stored_value = i64_storage_value_for_type(
            function_builder,
            compiled_field.value.ok_or_else(|| {
                build_failed(
                    format!(
                        "struct field '{}' produced no runtime value",
                        declared_field.name
                    ),
                    None,
                )
            })?,
            &compiled_field.type_reference,
        );
        function_builder.ins().store(
            mem_flags,
            stored_value,
            allocated_pointer,
            i32::try_from(field_index * 8).map_err(|_| {
                build_failed(
                    "struct field offset exceeds supported range".to_string(),
                    None,
                )
            })?,
        );
    }

    Ok(TypedValue {
        value: Some(allocated_pointer),
        type_reference: type_reference.clone(),
        terminates: false,
    })
}

fn compile_list_literal_expression(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    elements: &[ExecutableExpression],
    element_type: &ExecutableTypeReference,
) -> Result<TypedValue, CompilerFailure> {
    let element_count = i64::try_from(elements.len()).map_err(|_| {
        build_failed(
            "list literal length exceeds supported range".to_string(),
            None,
        )
    })?;
    let list_data_size_bytes = element_count.checked_mul(8).ok_or_else(|| {
        build_failed(
            "list literal size exceeds supported range".to_string(),
            None,
        )
    })?;
    let list_data_pointer = allocate_heap_bytes(state, function_builder, list_data_size_bytes)?;
    let list_header_pointer = allocate_heap_bytes(state, function_builder, LIST_HEADER_SIZE_BYTES)?;
    let mem_flags = MemFlags::new();

    for (index, element_expression) in elements.iter().enumerate() {
        let compiled_element = compile_expression(
            state,
            function_builder,
            compilation_context,
            element_expression,
        )?;
        if compiled_element.terminates {
            return Ok(compiled_element);
        }
        if !is_type_assignable(state, &compiled_element.type_reference, element_type) {
            return Err(build_failed(
                format!(
                    "list element type mismatch: expected {}, got {}",
                    type_reference_display(element_type),
                    type_reference_display(&compiled_element.type_reference)
                ),
                None,
            ));
        }
        let lowered_runtime_value = runtime_value_for_expected_type(
            state,
            function_builder,
            compiled_element.value,
            &compiled_element.type_reference,
            element_type,
        )?;
        let lowered_value = lowered_runtime_value.ok_or_else(|| {
            build_failed("list element produced no runtime value".to_string(), None)
        })?;
        let stored_value =
            i64_storage_value_for_type(function_builder, lowered_value, element_type);
        let element_offset = i32::try_from(index * 8).map_err(|_| {
            build_failed(
                "list element offset exceeds supported range".to_string(),
                None,
            )
        })?;
        function_builder
            .ins()
            .store(mem_flags, stored_value, list_data_pointer, element_offset);
    }

    let element_count_value = function_builder.ins().iconst(types::I64, element_count);
    function_builder.ins().store(
        mem_flags,
        element_count_value,
        list_header_pointer,
        LIST_LENGTH_OFFSET,
    );
    function_builder.ins().store(
        mem_flags,
        list_data_pointer,
        list_header_pointer,
        LIST_DATA_POINTER_OFFSET,
    );

    Ok(TypedValue {
        value: Some(list_header_pointer),
        type_reference: ExecutableTypeReference::List {
            element_type: Box::new(element_type.clone()),
        },
        terminates: false,
    })
}

fn compile_field_access_expression(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    target: &ExecutableExpression,
    field_name: &str,
) -> Result<TypedValue, CompilerFailure> {
    let compiled_target = compile_expression(state, function_builder, compilation_context, target)?;
    if compiled_target.terminates {
        return Ok(compiled_target);
    }
    if let ExecutableTypeReference::List { .. } = &compiled_target.type_reference {
        if field_name != "length" {
            return Err(build_failed(
                format!("unknown field 'List.{field_name}'"),
                None,
            ));
        }
        let target_pointer = compiled_target.value.ok_or_else(|| {
            build_failed(
                "field access target produced no runtime value".to_string(),
                None,
            )
        })?;
        let length_value = function_builder.ins().load(
            types::I64,
            MemFlags::new(),
            target_pointer,
            LIST_LENGTH_OFFSET,
        );
        return Ok(TypedValue {
            value: Some(length_value),
            type_reference: ExecutableTypeReference::Int64,
            terminates: false,
        });
    }
    let (struct_declaration, type_substitutions_by_type_parameter_name) =
        resolve_struct_type_details(state, &compiled_target.type_reference)?;
    let (field_index, declared_field) = struct_declaration
        .fields
        .iter()
        .enumerate()
        .find(|(_, field)| field.name == field_name)
        .ok_or_else(|| {
            build_failed(
                format!("unknown field '{}.{}'", struct_declaration.name, field_name),
                None,
            )
        })?;
    let loaded_i64 = function_builder.ins().load(
        types::I64,
        MemFlags::new(),
        compiled_target.value.ok_or_else(|| {
            build_failed(
                "field access target produced no runtime value".to_string(),
                None,
            )
        })?,
        i32::try_from(field_index * 8)
            .map_err(|_| build_failed("field offset exceeds supported range".to_string(), None))?,
    );
    let field_type = substitute_type_reference(
        &declared_field.type_reference,
        &type_substitutions_by_type_parameter_name,
    );
    let loaded_value = runtime_value_from_i64_storage(function_builder, loaded_i64, &field_type);

    Ok(TypedValue {
        value: Some(loaded_value),
        type_reference: field_type,
        terminates: false,
    })
}

fn compile_method_call_expression(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    callee: &ExecutableExpression,
    arguments: &[ExecutableExpression],
) -> Result<TypedValue, CompilerFailure> {
    let ExecutableExpression::FieldAccess {
        target,
        field: method_name,
    } = callee
    else {
        return Err(build_failed(
            "AOT Cranelift backend requires resolved call target metadata".to_string(),
            None,
        ));
    };

    let compiled_receiver =
        compile_expression(state, function_builder, compilation_context, target)?;
    if compiled_receiver.terminates {
        return Ok(compiled_receiver);
    }
    if let ExecutableTypeReference::TypeParameter { name } = &compiled_receiver.type_reference {
        return compile_type_parameter_method_call_expression(
            state,
            function_builder,
            compilation_context,
            name,
            &compiled_receiver,
            method_name,
            arguments,
        );
    }
    if let Ok((struct_declaration, type_substitutions_by_type_parameter_name)) =
        resolve_struct_type_details(state, &compiled_receiver.type_reference)
    {
        return compile_struct_method_call_expression(
            state,
            function_builder,
            compilation_context,
            struct_declaration,
            &type_substitutions_by_type_parameter_name,
            &compiled_receiver,
            method_name,
            arguments,
        );
    }

    let interface_declaration_result =
        resolve_interface_declaration_by_type_reference(state, &compiled_receiver.type_reference);
    if let Ok(interface_declaration) = interface_declaration_result {
        return compile_interface_method_call_expression(
            state,
            function_builder,
            compilation_context,
            interface_declaration,
            &compiled_receiver,
            method_name,
            arguments,
        );
    }
    if let Err(interface_resolution_error) = interface_declaration_result
        && matches!(
            compiled_receiver.type_reference,
            ExecutableTypeReference::NominalType {
                nominal_type_reference: Some(_),
                ..
            } | ExecutableTypeReference::NominalTypeApplication {
                base_nominal_type_reference: Some(_),
                ..
            }
        )
    {
        return Err(interface_resolution_error);
    }

    Err(build_failed(
        format!(
            "expected struct or interface receiver type, found {}",
            type_reference_display(&compiled_receiver.type_reference)
        ),
        None,
    ))
}

fn compile_type_parameter_method_call_expression(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    type_parameter_name: &str,
    compiled_receiver: &TypedValue,
    method_name: &str,
    arguments: &[ExecutableExpression],
) -> Result<TypedValue, CompilerFailure> {
    let type_parameter_witness = compilation_context
        .type_parameter_witness_by_name
        .get(type_parameter_name)
        .cloned()
        .ok_or_else(|| {
            build_failed(
                format!(
                    "missing witness table for constrained type parameter '{type_parameter_name}'"
                ),
                None,
            )
        })?;
    let interface_declaration = resolve_interface_declaration_by_reference(
        state,
        &type_parameter_witness.interface_reference,
    )?;
    compile_interface_method_call_through_vtable(
        state,
        function_builder,
        compilation_context,
        interface_declaration,
        type_parameter_witness.witness_table_pointer,
        compiled_receiver.value.ok_or_else(|| {
            build_failed(
                "method receiver produced no runtime value".to_string(),
                None,
            )
        })?,
        method_name,
        arguments,
    )
}

fn compile_struct_method_call_expression(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    struct_declaration: &ExecutableStructDeclaration,
    type_substitutions_by_type_parameter_name: &BTreeMap<String, ExecutableTypeReference>,
    compiled_receiver: &TypedValue,
    method_name: &str,
    arguments: &[ExecutableExpression],
) -> Result<TypedValue, CompilerFailure> {
    let method_key = MethodKey {
        struct_reference: struct_declaration.struct_reference.clone(),
        method_name: method_name.to_string(),
    };
    let method_record = state
        .method_record_by_key
        .get(&method_key)
        .ok_or_else(|| {
            build_failed(
                format!(
                    "unknown method '{}.{}'",
                    struct_declaration.name, method_name
                ),
                None,
            )
        })?
        .clone();

    if method_record.parameter_types.len() != arguments.len() {
        return Err(build_failed(
            format!(
                "method '{}.{}' expected {} argument(s), got {}",
                struct_declaration.name,
                method_name,
                method_record.parameter_types.len(),
                arguments.len()
            ),
            None,
        ));
    }

    let mut call_values = Vec::with_capacity(arguments.len() + 1);
    call_values.push(compiled_receiver.value.ok_or_else(|| {
        build_failed(
            "method receiver produced no runtime value".to_string(),
            None,
        )
    })?);
    for (parameter_type, argument_expression) in method_record.parameter_types.iter().zip(arguments)
    {
        let compiled_argument = compile_expression(
            state,
            function_builder,
            compilation_context,
            argument_expression,
        )?;
        if compiled_argument.terminates {
            return Ok(compiled_argument);
        }
        let expected_type =
            substitute_type_reference(parameter_type, type_substitutions_by_type_parameter_name);
        if !is_type_assignable(state, &compiled_argument.type_reference, &expected_type) {
            return Err(build_failed(
                format!(
                    "method argument type mismatch for '{}.{}': expected {}, got {}",
                    struct_declaration.name,
                    method_name,
                    type_reference_display(&expected_type),
                    type_reference_display(&compiled_argument.type_reference)
                ),
                None,
            ));
        }
        let lowered_argument = runtime_value_for_expected_type(
            state,
            function_builder,
            compiled_argument.value,
            &compiled_argument.type_reference,
            &expected_type,
        )?;
        call_values.push(lowered_argument.ok_or_else(|| {
            build_failed(
                "method argument produced no runtime value".to_string(),
                None,
            )
        })?);
    }

    let callee = state
        .module
        .declare_func_in_func(method_record.id, function_builder.func);
    let call = function_builder.ins().call(callee, &call_values);
    let return_type = substitute_type_reference(
        &method_record.return_type,
        type_substitutions_by_type_parameter_name,
    );
    if matches!(
        return_type,
        ExecutableTypeReference::Nil | ExecutableTypeReference::Never
    ) {
        let return_terminates = matches!(&return_type, ExecutableTypeReference::Never);
        Ok(TypedValue {
            value: None,
            type_reference: return_type,
            terminates: return_terminates,
        })
    } else {
        Ok(TypedValue {
            value: Some(function_builder.inst_results(call)[0]),
            type_reference: return_type,
            terminates: false,
        })
    }
}

fn compile_interface_method_call_expression(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    interface_declaration: &ExecutableInterfaceDeclaration,
    compiled_receiver: &TypedValue,
    method_name: &str,
    arguments: &[ExecutableExpression],
) -> Result<TypedValue, CompilerFailure> {
    let interface_value_pointer = compiled_receiver.value.ok_or_else(|| {
        build_failed(
            "interface method receiver produced no runtime value".to_string(),
            None,
        )
    })?;
    let data_pointer = function_builder.ins().load(
        types::I64,
        MemFlags::new(),
        interface_value_pointer,
        INTERFACE_VALUE_DATA_POINTER_OFFSET,
    );
    let vtable_pointer = function_builder.ins().load(
        types::I64,
        MemFlags::new(),
        interface_value_pointer,
        INTERFACE_VALUE_VTABLE_POINTER_OFFSET,
    );
    compile_interface_method_call_through_vtable(
        state,
        function_builder,
        compilation_context,
        interface_declaration,
        vtable_pointer,
        data_pointer,
        method_name,
        arguments,
    )
}

fn compile_interface_method_call_through_vtable(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    interface_declaration: &ExecutableInterfaceDeclaration,
    vtable_pointer: Value,
    receiver_data_pointer: Value,
    method_name: &str,
    arguments: &[ExecutableExpression],
) -> Result<TypedValue, CompilerFailure> {
    let (method_index, method_declaration) = interface_declaration
        .methods
        .iter()
        .enumerate()
        .find(|(_, method)| method.name == method_name)
        .ok_or_else(|| {
            build_failed(
                format!(
                    "unknown method '{}.{}'",
                    interface_declaration.name, method_name
                ),
                None,
            )
        })?;

    if method_declaration.parameters.len() != arguments.len() {
        return Err(build_failed(
            format!(
                "method '{}.{}' expected {} argument(s), got {}",
                interface_declaration.name,
                method_name,
                method_declaration.parameters.len(),
                arguments.len()
            ),
            None,
        ));
    }
    let method_pointer_offset_bytes = i32::try_from(method_index * 8).map_err(|_| {
        build_failed(
            "interface method index exceeds supported offset range".to_string(),
            None,
        )
    })?;
    let function_pointer = function_builder.ins().load(
        types::I64,
        MemFlags::new(),
        vtable_pointer,
        method_pointer_offset_bytes,
    );

    let mut call_values = Vec::with_capacity(arguments.len() + 1);
    call_values.push(receiver_data_pointer);
    for (parameter, argument_expression) in method_declaration.parameters.iter().zip(arguments) {
        let compiled_argument = compile_expression(
            state,
            function_builder,
            compilation_context,
            argument_expression,
        )?;
        if compiled_argument.terminates {
            return Ok(compiled_argument);
        }
        if !is_type_assignable(
            state,
            &compiled_argument.type_reference,
            &parameter.type_reference,
        ) {
            return Err(build_failed(
                format!(
                    "method argument type mismatch for '{}.{}': expected {}, got {}",
                    interface_declaration.name,
                    method_name,
                    type_reference_display(&parameter.type_reference),
                    type_reference_display(&compiled_argument.type_reference)
                ),
                None,
            ));
        }
        let lowered_argument = runtime_value_for_expected_type(
            state,
            function_builder,
            compiled_argument.value,
            &compiled_argument.type_reference,
            &parameter.type_reference,
        )?;
        call_values.push(lowered_argument.ok_or_else(|| {
            build_failed(
                "interface method argument produced no runtime value".to_string(),
                None,
            )
        })?);
    }

    let mut call_signature = state.module.make_signature();
    call_signature.params.push(AbiParam::new(types::I64));
    for parameter in &method_declaration.parameters {
        call_signature
            .params
            .push(AbiParam::new(cranelift_type_for(&parameter.type_reference)));
    }
    if !matches!(
        method_declaration.return_type,
        ExecutableTypeReference::Nil | ExecutableTypeReference::Never
    ) {
        call_signature
            .returns
            .push(AbiParam::new(cranelift_type_for(
                &method_declaration.return_type,
            )));
    }
    let signature_reference = function_builder.import_signature(call_signature);
    let call =
        function_builder
            .ins()
            .call_indirect(signature_reference, function_pointer, &call_values);

    if matches!(
        method_declaration.return_type,
        ExecutableTypeReference::Nil | ExecutableTypeReference::Never
    ) {
        Ok(TypedValue {
            value: None,
            type_reference: method_declaration.return_type.clone(),
            terminates: matches!(
                method_declaration.return_type,
                ExecutableTypeReference::Never
            ),
        })
    } else {
        Ok(TypedValue {
            value: Some(function_builder.inst_results(call)[0]),
            type_reference: method_declaration.return_type.clone(),
            terminates: false,
        })
    }
}

fn compile_matches_expression(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    value_expression: &ExecutableExpression,
    matched_type_reference: &ExecutableTypeReference,
) -> Result<TypedValue, CompilerFailure> {
    let value = compile_expression(
        state,
        function_builder,
        compilation_context,
        value_expression,
    )?;
    if value.terminates {
        return Ok(value);
    }

    let bool_value = if matches!(value.type_reference, ExecutableTypeReference::Union { .. }) {
        let union_box_pointer = value.value.ok_or_else(|| {
            build_failed(
                "matches operand produced no runtime value".to_string(),
                None,
            )
        })?;
        emit_union_match_condition(function_builder, union_box_pointer, matched_type_reference)?
    } else if type_reference_matches_pattern_type(&value.type_reference, matched_type_reference) {
        function_builder.ins().iconst(types::I8, 1)
    } else {
        function_builder.ins().iconst(types::I8, 0)
    };

    Ok(TypedValue {
        value: Some(bool_value),
        type_reference: ExecutableTypeReference::Boolean,
        terminates: false,
    })
}

fn compile_match_expression(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    target_expression: &ExecutableExpression,
    arms: &[ExecutableMatchArm],
) -> Result<TypedValue, CompilerFailure> {
    if arms.is_empty() {
        return Err(build_failed(
            "match expression must contain at least one arm".to_string(),
            None,
        ));
    }

    let target = compile_expression(
        state,
        function_builder,
        compilation_context,
        target_expression,
    )?;
    if target.terminates {
        return Ok(target);
    }

    let mut pending_block = function_builder.create_block();
    function_builder.ins().jump(pending_block, &[]);
    function_builder.switch_to_block(pending_block);

    let merge_block = function_builder.create_block();
    let mut merged_type_reference: Option<ExecutableTypeReference> = None;
    let mut merged_result_variable: Option<Variable> = None;

    for arm in arms {
        let arm_block = function_builder.create_block();
        let next_block = function_builder.create_block();

        let condition = emit_match_arm_condition(function_builder, &target, &arm.pattern)?;
        let zero = function_builder.ins().iconst(types::I8, 0);
        let condition_is_true = function_builder
            .ins()
            .icmp(IntCC::NotEqual, condition, zero);

        function_builder
            .ins()
            .brif(condition_is_true, arm_block, &[], next_block, &[]);
        function_builder.seal_block(pending_block);
        pending_block = next_block;

        function_builder.switch_to_block(arm_block);
        let mut arm_context = FunctionCompilationContext {
            local_value_by_name: compilation_context.local_value_by_name.clone(),
            type_parameter_witness_by_name: compilation_context
                .type_parameter_witness_by_name
                .clone(),
            loop_context: compilation_context.loop_context,
        };
        bind_match_pattern_local(
            state,
            function_builder,
            &mut arm_context,
            &target,
            &arm.pattern,
        )?;
        let arm_value = compile_expression(state, function_builder, &mut arm_context, &arm.value)?;
        if arm_value.terminates {
            return Ok(arm_value);
        }
        let Some(arm_runtime_value) = arm_value.value else {
            return Err(build_failed(
                "match arm produced no runtime value".to_string(),
                None,
            ));
        };
        if let Some(existing_type) = &merged_type_reference {
            if arm_value.type_reference != *existing_type {
                return Err(build_failed(
                    "match arms must evaluate to a consistent type".to_string(),
                    None,
                ));
            }
        } else {
            merged_type_reference = Some(arm_value.type_reference.clone());
            let variable = function_builder
                .declare_var(function_builder.func.dfg.value_type(arm_runtime_value));
            merged_result_variable = Some(variable);
        }
        let merged_result_variable =
            merged_result_variable.expect("merged result variable must exist");
        function_builder.def_var(merged_result_variable, arm_runtime_value);
        function_builder.ins().jump(merge_block, &[]);
        function_builder.seal_block(arm_block);

        function_builder.switch_to_block(next_block);
    }

    function_builder.ins().trap(TrapCode::user(2).unwrap());
    function_builder.seal_block(pending_block);

    let merged_type_reference = merged_type_reference.ok_or_else(|| {
        build_failed(
            "match expression did not produce a merged type".to_string(),
            None,
        )
    })?;
    function_builder.switch_to_block(merge_block);
    function_builder.seal_block(merge_block);
    let merged_result_variable = merged_result_variable.ok_or_else(|| {
        build_failed(
            "match expression did not produce a merged value".to_string(),
            None,
        )
    })?;
    let merged_value = function_builder.use_var(merged_result_variable);

    Ok(TypedValue {
        value: Some(merged_value),
        type_reference: merged_type_reference,
        terminates: false,
    })
}

fn emit_match_arm_condition(
    function_builder: &mut FunctionBuilder<'_>,
    target: &TypedValue,
    pattern: &ExecutableMatchPattern,
) -> Result<Value, CompilerFailure> {
    let pattern_type_reference = match pattern {
        ExecutableMatchPattern::Type { type_reference }
        | ExecutableMatchPattern::Binding { type_reference, .. } => type_reference,
    };
    if matches!(target.type_reference, ExecutableTypeReference::Union { .. }) {
        let union_box_pointer = target.value.ok_or_else(|| {
            build_failed("match target produced no runtime value".to_string(), None)
        })?;
        emit_union_match_condition(function_builder, union_box_pointer, pattern_type_reference)
    } else if type_reference_matches_pattern_type(&target.type_reference, pattern_type_reference) {
        Ok(function_builder.ins().iconst(types::I8, 1))
    } else {
        Ok(function_builder.ins().iconst(types::I8, 0))
    }
}

fn bind_match_pattern_local(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    target: &TypedValue,
    pattern: &ExecutableMatchPattern,
) -> Result<(), CompilerFailure> {
    let ExecutableMatchPattern::Binding {
        binding_name,
        type_reference,
    } = pattern
    else {
        return Ok(());
    };

    let binding_runtime_value =
        if matches!(target.type_reference, ExecutableTypeReference::Union { .. }) {
            let union_box_pointer = target.value.ok_or_else(|| {
                build_failed(
                    "match binding target produced no runtime value".to_string(),
                    None,
                )
            })?;
            extract_union_payload_for_type(function_builder, union_box_pointer, type_reference)
        } else {
            runtime_value_for_expected_type(
                state,
                function_builder,
                target.value,
                &target.type_reference,
                type_reference,
            )?
        };

    let Some(binding_value) = binding_runtime_value else {
        return Err(build_failed(
            format!("match binding '{binding_name}' produced no runtime value"),
            None,
        ));
    };
    let local_value =
        declare_local_variable(function_builder, binding_value, type_reference.clone());
    compilation_context
        .local_value_by_name
        .insert(binding_name.clone(), local_value);
    Ok(())
}

fn runtime_value_for_expected_type(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    value: Option<Value>,
    actual_type: &ExecutableTypeReference,
    expected_type: &ExecutableTypeReference,
) -> Result<Option<Value>, CompilerFailure> {
    if matches!(actual_type, ExecutableTypeReference::Union { .. })
        && !matches!(expected_type, ExecutableTypeReference::Union { .. })
    {
        let union_box_pointer = value.ok_or_else(|| {
            build_failed(
                "union value expected for union-to-member conversion".to_string(),
                None,
            )
        })?;
        return Ok(extract_union_payload_for_type(
            function_builder,
            union_box_pointer,
            expected_type,
        ));
    }

    if matches!(expected_type, ExecutableTypeReference::Union { .. })
        && !matches!(actual_type, ExecutableTypeReference::Union { .. })
    {
        let raw_value = value.unwrap_or_else(|| function_builder.ins().iconst(types::I64, 0));
        let union_box_pointer = box_union_value(state, function_builder, raw_value, actual_type)?;
        return Ok(Some(union_box_pointer));
    }

    if let Ok(interface_declaration) =
        resolve_interface_declaration_by_type_reference(state, expected_type)
        && let Ok((struct_declaration, _)) = resolve_struct_type_details(state, actual_type)
        && struct_implements_interface(struct_declaration, interface_declaration)
    {
        let data_pointer = value.ok_or_else(|| {
            build_failed(
                "value expected for struct-to-interface conversion".to_string(),
                None,
            )
        })?;
        let interface_value_pointer = box_interface_value(
            state,
            function_builder,
            data_pointer,
            struct_declaration,
            interface_declaration,
        )?;
        return Ok(Some(interface_value_pointer));
    }

    Ok(value)
}

fn comparable_type_reference_for_equality(
    state: &CompilationState<'_>,
    left_type_reference: &ExecutableTypeReference,
    right_type_reference: &ExecutableTypeReference,
) -> Option<ExecutableTypeReference> {
    if is_type_assignable(state, left_type_reference, right_type_reference) {
        return Some(right_type_reference.clone());
    }
    if is_type_assignable(state, right_type_reference, left_type_reference) {
        return Some(left_type_reference.clone());
    }
    None
}

fn box_union_value(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    raw_value: Value,
    raw_type: &ExecutableTypeReference,
) -> Result<Value, CompilerFailure> {
    let union_box_pointer = allocate_heap_bytes(state, function_builder, UNION_BOX_SIZE_BYTES)?;
    let tag_value = function_builder
        .ins()
        .iconst(types::I64, union_type_tag_for_type_reference(raw_type)?);
    let payload_value = i64_storage_value_for_type(function_builder, raw_value, raw_type);
    let mem_flags = MemFlags::new();
    function_builder.ins().store(
        mem_flags,
        tag_value,
        union_box_pointer,
        UNION_BOX_TAG_OFFSET,
    );
    function_builder.ins().store(
        mem_flags,
        payload_value,
        union_box_pointer,
        UNION_BOX_PAYLOAD_OFFSET,
    );
    Ok(union_box_pointer)
}

fn emit_union_match_condition(
    function_builder: &mut FunctionBuilder<'_>,
    union_box_pointer: Value,
    matched_type_reference: &ExecutableTypeReference,
) -> Result<Value, CompilerFailure> {
    let loaded_tag = function_builder.ins().load(
        types::I64,
        MemFlags::new(),
        union_box_pointer,
        UNION_BOX_TAG_OFFSET,
    );
    let expected_tag = function_builder.ins().iconst(
        types::I64,
        union_type_tag_for_type_reference(matched_type_reference)?,
    );
    let tag_matches = function_builder
        .ins()
        .icmp(IntCC::Equal, loaded_tag, expected_tag);
    let one = function_builder.ins().iconst(types::I8, 1);
    let zero = function_builder.ins().iconst(types::I8, 0);
    let tag_matches_i8 = function_builder.ins().select(tag_matches, one, zero);

    let is_enum_variant_pattern = matches!(
        matched_type_reference,
        ExecutableTypeReference::NominalType { name, .. } if name.contains('.')
    );
    if !is_enum_variant_pattern {
        return Ok(tag_matches_i8);
    }

    let loaded_payload = function_builder.ins().load(
        types::I64,
        MemFlags::new(),
        union_box_pointer,
        UNION_BOX_PAYLOAD_OFFSET,
    );
    let ExecutableTypeReference::NominalType { name, .. } = matched_type_reference else {
        return Ok(tag_matches_i8);
    };
    let (enum_name, variant_name) = split_enum_variant_type_name(name)?;
    let expected_enum_payload = function_builder.ins().iconst(
        types::I64,
        enum_variant_tag(&ExecutableEnumVariantReference {
            enum_name: enum_name.to_string(),
            variant_name: variant_name.to_string(),
        }),
    );
    let payload_matches =
        function_builder
            .ins()
            .icmp(IntCC::Equal, loaded_payload, expected_enum_payload);
    let payload_matches_i8 = function_builder.ins().select(payload_matches, one, zero);
    Ok(function_builder
        .ins()
        .band(tag_matches_i8, payload_matches_i8))
}

fn extract_union_payload_for_type(
    function_builder: &mut FunctionBuilder<'_>,
    union_box_pointer: Value,
    payload_type_reference: &ExecutableTypeReference,
) -> Option<Value> {
    if matches!(payload_type_reference, ExecutableTypeReference::Nil) {
        return None;
    }
    let loaded_payload = function_builder.ins().load(
        types::I64,
        MemFlags::new(),
        union_box_pointer,
        UNION_BOX_PAYLOAD_OFFSET,
    );
    Some(runtime_value_from_i64_storage(
        function_builder,
        loaded_payload,
        payload_type_reference,
    ))
}

fn type_reference_matches_pattern_type(
    value_type_reference: &ExecutableTypeReference,
    pattern_type_reference: &ExecutableTypeReference,
) -> bool {
    if value_type_reference == pattern_type_reference {
        return true;
    }
    match value_type_reference {
        ExecutableTypeReference::Union { members } => members
            .iter()
            .any(|member| type_reference_matches_pattern_type(member, pattern_type_reference)),
        _ => false,
    }
}

fn is_type_assignable(
    state: &CompilationState<'_>,
    actual_type: &ExecutableTypeReference,
    expected_type: &ExecutableTypeReference,
) -> bool {
    if actual_type == expected_type {
        return true;
    }

    if let Ok(interface_declaration) =
        resolve_interface_declaration_by_type_reference(state, expected_type)
        && let Ok((struct_declaration, _)) = resolve_struct_type_details(state, actual_type)
    {
        return struct_implements_interface(struct_declaration, interface_declaration);
    }

    match expected_type {
        ExecutableTypeReference::Union { members } => members
            .iter()
            .any(|member| is_type_assignable(state, actual_type, member)),
        _ => false,
    }
}

fn union_type_tag_for_type_reference(
    type_reference: &ExecutableTypeReference,
) -> Result<i64, CompilerFailure> {
    match type_reference {
        ExecutableTypeReference::Int64 => Ok(UNION_TAG_INT64),
        ExecutableTypeReference::Boolean => Ok(UNION_TAG_BOOLEAN),
        ExecutableTypeReference::String => Ok(UNION_TAG_STRING),
        ExecutableTypeReference::Nil | ExecutableTypeReference::Never => Ok(UNION_TAG_NIL),
        ExecutableTypeReference::List { .. }
        | ExecutableTypeReference::TypeParameter { .. }
        | ExecutableTypeReference::NominalTypeApplication { .. } => Ok(UNION_TAG_STRUCT),
        ExecutableTypeReference::Function { .. } => Ok(UNION_TAG_FUNCTION),
        ExecutableTypeReference::NominalType { name, .. } => {
            if name.contains('.') {
                Ok(UNION_TAG_ENUM_VARIANT)
            } else {
                Ok(UNION_TAG_STRUCT)
            }
        }
        ExecutableTypeReference::Union { .. } => Err(build_failed(
            "nested union values are not currently supported in AOT backend".to_string(),
            None,
        )),
    }
}

fn split_enum_variant_type_name(name: &str) -> Result<(&str, &str), CompilerFailure> {
    let Some((enum_name, variant_name)) = name.rsplit_once('.') else {
        return Err(build_failed(
            format!("invalid enum variant type name '{name}'"),
            None,
        ));
    };
    if enum_name.is_empty() || variant_name.is_empty() {
        return Err(build_failed(
            format!("invalid enum variant type name '{name}'"),
            None,
        ));
    }
    Ok((enum_name, variant_name))
}

fn instantiate_generic_signature(
    type_parameter_names: &[String],
    parameter_types: &[ExecutableTypeReference],
    return_type: &ExecutableTypeReference,
    type_arguments: &[ExecutableTypeReference],
) -> Result<(Vec<ExecutableTypeReference>, ExecutableTypeReference), CompilerFailure> {
    if type_parameter_names.is_empty() {
        if !type_arguments.is_empty() {
            return Err(build_failed(
                "function does not accept type arguments".to_string(),
                None,
            ));
        }
        return Ok((parameter_types.to_vec(), return_type.clone()));
    }
    if type_parameter_names.len() != type_arguments.len() {
        return Err(build_failed(
            format!(
                "function expects {} type argument(s), got {}",
                type_parameter_names.len(),
                type_arguments.len()
            ),
            None,
        ));
    }
    let type_substitutions_by_type_parameter_name = type_parameter_names
        .iter()
        .cloned()
        .zip(type_arguments.iter().cloned())
        .collect::<BTreeMap<_, _>>();
    Ok((
        parameter_types
            .iter()
            .map(|parameter_type| {
                substitute_type_reference(
                    parameter_type,
                    &type_substitutions_by_type_parameter_name,
                )
            })
            .collect(),
        substitute_type_reference(return_type, &type_substitutions_by_type_parameter_name),
    ))
}

fn resolve_struct_type_details<'program>(
    state: &CompilationState<'program>,
    type_reference: &ExecutableTypeReference,
) -> Result<
    (
        &'program ExecutableStructDeclaration,
        BTreeMap<String, ExecutableTypeReference>,
    ),
    CompilerFailure,
> {
    match type_reference {
        ExecutableTypeReference::NominalType {
            nominal_type_reference,
            name,
        } => {
            let struct_declaration = if let Some(nominal_type_reference) = nominal_type_reference {
                state
                    .struct_declaration_by_reference
                    .values()
                    .find(|declaration| {
                        declaration.struct_reference.package_path
                            == nominal_type_reference.package_path
                            && declaration.struct_reference.symbol_name
                                == nominal_type_reference.symbol_name
                    })
            } else {
                state
                    .struct_declaration_by_reference
                    .values()
                    .find(|declaration| declaration.name == *name)
            }
            .copied()
            .ok_or_else(|| build_failed(format!("unknown struct type '{name}'"), None))?;
            Ok((struct_declaration, BTreeMap::new()))
        }
        ExecutableTypeReference::NominalTypeApplication {
            base_nominal_type_reference,
            base_name,
            arguments,
        } => {
            let struct_declaration =
                if let Some(base_nominal_type_reference) = base_nominal_type_reference {
                    state
                        .struct_declaration_by_reference
                        .values()
                        .find(|declaration| {
                            declaration.struct_reference.package_path
                                == base_nominal_type_reference.package_path
                                && declaration.struct_reference.symbol_name
                                    == base_nominal_type_reference.symbol_name
                        })
                } else {
                    state
                        .struct_declaration_by_reference
                        .values()
                        .find(|declaration| declaration.name == *base_name)
                }
                .copied()
                .ok_or_else(|| build_failed(format!("unknown struct type '{base_name}'"), None))?;
            let type_substitutions_by_type_parameter_name =
                type_substitutions_for_struct_type(struct_declaration, type_reference)?;
            if struct_declaration.type_parameter_names.len() != arguments.len() {
                return Err(build_failed(
                    format!(
                        "struct type '{}' expects {} type argument(s), got {}",
                        base_name,
                        struct_declaration.type_parameter_names.len(),
                        arguments.len()
                    ),
                    None,
                ));
            }
            Ok((
                struct_declaration,
                type_substitutions_by_type_parameter_name,
            ))
        }
        _ => Err(build_failed(
            format!(
                "expected struct receiver type, found {}",
                type_reference_display(type_reference)
            ),
            None,
        )),
    }
}

fn resolve_interface_declaration_by_type_reference<'program>(
    state: &CompilationState<'program>,
    type_reference: &ExecutableTypeReference,
) -> Result<&'program ExecutableInterfaceDeclaration, CompilerFailure> {
    let (ExecutableTypeReference::NominalType {
        nominal_type_reference: Some(interface_reference),
        ..
    }
    | ExecutableTypeReference::NominalTypeApplication {
        base_nominal_type_reference: Some(interface_reference),
        ..
    }) = type_reference
    else {
        return Err(build_failed(
            format!(
                "expected interface type, found {}",
                type_reference_display(type_reference)
            ),
            None,
        ));
    };
    state
        .interface_declaration_by_reference
        .get(&ExecutableInterfaceReference {
            package_path: interface_reference.package_path.clone(),
            symbol_name: interface_reference.symbol_name.clone(),
        })
        .copied()
        .ok_or_else(|| {
            build_failed(
                format!(
                    "unknown interface type '{}::{}'",
                    interface_reference.package_path, interface_reference.symbol_name
                ),
                None,
            )
        })
}

fn resolve_interface_declaration_by_reference<'program>(
    state: &CompilationState<'program>,
    interface_reference: &ExecutableInterfaceReference,
) -> Result<&'program ExecutableInterfaceDeclaration, CompilerFailure> {
    state
        .interface_declaration_by_reference
        .get(interface_reference)
        .copied()
        .ok_or_else(|| {
            build_failed(
                format!(
                    "unknown interface type '{}::{}'",
                    interface_reference.package_path, interface_reference.symbol_name
                ),
                None,
            )
        })
}

fn struct_implements_interface(
    struct_declaration: &ExecutableStructDeclaration,
    interface_declaration: &ExecutableInterfaceDeclaration,
) -> bool {
    struct_declaration
        .implemented_interfaces
        .iter()
        .any(|implemented_interface| {
            implemented_interface.package_path
                == interface_declaration.interface_reference.package_path
                && implemented_interface.symbol_name
                    == interface_declaration.interface_reference.symbol_name
        })
}

fn box_interface_value(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    data_pointer: Value,
    struct_declaration: &ExecutableStructDeclaration,
    interface_declaration: &ExecutableInterfaceDeclaration,
) -> Result<Value, CompilerFailure> {
    let vtable_pointer = build_interface_vtable(
        state,
        function_builder,
        struct_declaration,
        interface_declaration,
    )?;
    let interface_value_pointer =
        allocate_heap_bytes(state, function_builder, INTERFACE_VALUE_SIZE_BYTES)?;
    let mem_flags = MemFlags::new();
    function_builder.ins().store(
        mem_flags,
        data_pointer,
        interface_value_pointer,
        INTERFACE_VALUE_DATA_POINTER_OFFSET,
    );
    function_builder.ins().store(
        mem_flags,
        vtable_pointer,
        interface_value_pointer,
        INTERFACE_VALUE_VTABLE_POINTER_OFFSET,
    );
    Ok(interface_value_pointer)
}

fn build_interface_vtable(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    struct_declaration: &ExecutableStructDeclaration,
    interface_declaration: &ExecutableInterfaceDeclaration,
) -> Result<Value, CompilerFailure> {
    let vtable_size_bytes =
        i64::try_from(interface_declaration.methods.len() * 8).map_err(|_| {
            build_failed(
                "interface vtable size exceeds supported allocation range".to_string(),
                None,
            )
        })?;
    let vtable_pointer = allocate_heap_bytes(state, function_builder, vtable_size_bytes)?;
    let mem_flags = MemFlags::new();

    for (method_index, interface_method) in interface_declaration.methods.iter().enumerate() {
        let method_key = MethodKey {
            struct_reference: struct_declaration.struct_reference.clone(),
            method_name: interface_method.name.clone(),
        };
        let method_record = state.method_record_by_key.get(&method_key).ok_or_else(|| {
            build_failed(
                format!(
                    "type '{}' does not provide method '{}' required by interface '{}'",
                    struct_declaration.name, interface_method.name, interface_declaration.name
                ),
                None,
            )
        })?;
        let function_reference = state
            .module
            .declare_func_in_func(method_record.id, function_builder.func);
        let function_pointer = function_builder
            .ins()
            .func_addr(types::I64, function_reference);
        let method_offset_bytes = i32::try_from(method_index * 8).map_err(|_| {
            build_failed(
                "interface vtable offset exceeds supported range".to_string(),
                None,
            )
        })?;
        function_builder.ins().store(
            mem_flags,
            function_pointer,
            vtable_pointer,
            method_offset_bytes,
        );
    }

    Ok(vtable_pointer)
}

fn build_witness_table_for_constraint(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    type_argument: &ExecutableTypeReference,
    interface_reference: &ExecutableInterfaceReference,
) -> Result<Value, CompilerFailure> {
    let interface_declaration =
        resolve_interface_declaration_by_reference(state, interface_reference)?;
    let (struct_declaration, _) =
        resolve_struct_type_details(state, type_argument).map_err(|_| {
            build_failed(
                format!(
                    "constraint witness table currently requires struct type argument, got {}",
                    type_reference_display(type_argument)
                ),
                None,
            )
        })?;
    if !struct_implements_interface(struct_declaration, interface_declaration) {
        return Err(build_failed(
            format!(
                "type '{}' does not implement required interface '{}'",
                struct_declaration.name, interface_declaration.name
            ),
            None,
        ));
    }
    build_interface_vtable(
        state,
        function_builder,
        struct_declaration,
        interface_declaration,
    )
}

fn type_substitutions_for_struct_type(
    struct_declaration: &ExecutableStructDeclaration,
    type_reference: &ExecutableTypeReference,
) -> Result<BTreeMap<String, ExecutableTypeReference>, CompilerFailure> {
    if struct_declaration.type_parameter_names.is_empty() {
        return Ok(BTreeMap::new());
    }
    let ExecutableTypeReference::NominalTypeApplication { arguments, .. } = type_reference else {
        return Err(build_failed(
            format!(
                "generic struct '{}' requires explicit type arguments",
                struct_declaration.name
            ),
            None,
        ));
    };
    if arguments.len() != struct_declaration.type_parameter_names.len() {
        return Err(build_failed(
            format!(
                "struct '{}' expects {} type argument(s), got {}",
                struct_declaration.name,
                struct_declaration.type_parameter_names.len(),
                arguments.len()
            ),
            None,
        ));
    }
    Ok(struct_declaration
        .type_parameter_names
        .iter()
        .cloned()
        .zip(arguments.iter().cloned())
        .collect())
}

fn substitute_type_reference(
    type_reference: &ExecutableTypeReference,
    type_substitutions_by_type_parameter_name: &BTreeMap<String, ExecutableTypeReference>,
) -> ExecutableTypeReference {
    match type_reference {
        ExecutableTypeReference::TypeParameter { name } => {
            type_substitutions_by_type_parameter_name
                .get(name)
                .cloned()
                .unwrap_or_else(|| type_reference.clone())
        }
        ExecutableTypeReference::Function {
            parameter_types,
            return_type,
        } => ExecutableTypeReference::Function {
            parameter_types: parameter_types
                .iter()
                .map(|parameter_type| {
                    substitute_type_reference(
                        parameter_type,
                        type_substitutions_by_type_parameter_name,
                    )
                })
                .collect(),
            return_type: Box::new(substitute_type_reference(
                return_type,
                type_substitutions_by_type_parameter_name,
            )),
        },
        ExecutableTypeReference::Union { members } => ExecutableTypeReference::Union {
            members: members
                .iter()
                .map(|member| {
                    substitute_type_reference(member, type_substitutions_by_type_parameter_name)
                })
                .collect(),
        },
        ExecutableTypeReference::NominalTypeApplication {
            base_nominal_type_reference,
            base_name,
            arguments,
        } => ExecutableTypeReference::NominalTypeApplication {
            base_nominal_type_reference: base_nominal_type_reference.clone(),
            base_name: base_name.clone(),
            arguments: arguments
                .iter()
                .map(|argument| {
                    substitute_type_reference(argument, type_substitutions_by_type_parameter_name)
                })
                .collect(),
        },
        ExecutableTypeReference::NominalType {
            nominal_type_reference,
            name,
        } => ExecutableTypeReference::NominalType {
            nominal_type_reference: nominal_type_reference.clone(),
            name: name.clone(),
        },
        _ => type_reference.clone(),
    }
}

fn type_reference_display(type_reference: &ExecutableTypeReference) -> String {
    match type_reference {
        ExecutableTypeReference::Int64 => "int64".to_string(),
        ExecutableTypeReference::Boolean => "boolean".to_string(),
        ExecutableTypeReference::String => "string".to_string(),
        ExecutableTypeReference::Nil => "nil".to_string(),
        ExecutableTypeReference::Never => "never".to_string(),
        ExecutableTypeReference::TypeParameter { name }
        | ExecutableTypeReference::NominalType { name, .. } => name.clone(),
        ExecutableTypeReference::List { element_type } => {
            format!("List[{}]", type_reference_display(element_type))
        }
        ExecutableTypeReference::Function {
            parameter_types,
            return_type,
        } => format!(
            "function({}) -> {}",
            parameter_types
                .iter()
                .map(type_reference_display)
                .collect::<Vec<_>>()
                .join(", "),
            type_reference_display(return_type)
        ),
        ExecutableTypeReference::NominalTypeApplication {
            base_name,
            arguments,
            ..
        } => format!(
            "{}[{}]",
            base_name,
            arguments
                .iter()
                .map(type_reference_display)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ExecutableTypeReference::Union { members } => members
            .iter()
            .map(type_reference_display)
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

fn i64_storage_value_for_type(
    function_builder: &mut FunctionBuilder<'_>,
    value: Value,
    type_reference: &ExecutableTypeReference,
) -> Value {
    match type_reference {
        ExecutableTypeReference::Boolean => function_builder.ins().uextend(types::I64, value),
        _ => value,
    }
}

fn runtime_value_from_i64_storage(
    function_builder: &mut FunctionBuilder<'_>,
    stored_i64: Value,
    type_reference: &ExecutableTypeReference,
) -> Value {
    match type_reference {
        ExecutableTypeReference::Boolean => function_builder.ins().ireduce(types::I8, stored_i64),
        _ => stored_i64,
    }
}

fn enum_variant_tag(enum_variant_reference: &ExecutableEnumVariantReference) -> i64 {
    // Stable deterministic tag from enum+variant identity.
    let identity = format!(
        "{}::{}",
        enum_variant_reference.enum_name, enum_variant_reference.variant_name
    );
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in identity.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    i64::from_ne_bytes(hash.to_ne_bytes())
}

pub(crate) fn allocate_heap_bytes(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    byte_count: i64,
) -> Result<Value, CompilerFailure> {
    crate::runtime_interface_emission::allocate_heap_bytes(
        &mut state.module,
        &state.external_runtime_functions,
        function_builder,
        byte_count,
    )
}

fn emit_write_string_with_newline(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    file_descriptor: i32,
    string_pointer: Value,
) -> Result<(), CompilerFailure> {
    crate::runtime_interface_emission::emit_write_string_with_newline(
        &mut state.module,
        &state.external_runtime_functions,
        function_builder,
        file_descriptor,
        string_pointer,
    )
}

fn emit_exit_call(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    exit_code: i32,
) {
    crate::runtime_interface_emission::emit_exit_call(
        &mut state.module,
        &state.external_runtime_functions,
        function_builder,
        exit_code,
    );
}

fn intern_string_literal(
    state: &mut CompilationState<'_>,
    function_builder: &mut FunctionBuilder<'_>,
    value: &str,
) -> Result<Value, CompilerFailure> {
    crate::runtime_interface_emission::intern_string_literal(
        &mut state.module,
        &state.external_runtime_functions,
        function_builder,
        value,
    )
}

fn declare_local_variable(
    function_builder: &mut FunctionBuilder<'_>,
    value: Value,
    type_reference: ExecutableTypeReference,
) -> LocalValue {
    let value_type = function_builder.func.dfg.value_type(value);
    let variable = function_builder.declare_var(value_type);
    function_builder.def_var(variable, value);
    LocalValue {
        variable,
        type_reference,
    }
}

fn lowered_function_symbol_name(callable_reference: &ExecutableCallableReference) -> String {
    if callable_reference.package_path.is_empty() {
        return format!("coppice_{}", callable_reference.symbol_name);
    }
    format!(
        "coppice_{}_{}",
        callable_reference
            .package_path
            .replace(['/', '\\'], "_")
            .replace("::", "_"),
        callable_reference.symbol_name
    )
}

fn lowered_method_symbol_name(
    struct_reference: &ExecutableStructReference,
    method_name: &str,
) -> String {
    let package_prefix = if struct_reference.package_path.is_empty() {
        String::new()
    } else {
        format!(
            "{}_",
            struct_reference
                .package_path
                .replace(['/', '\\'], "_")
                .replace("::", "_")
        )
    };
    format!(
        "coppice_{package_prefix}{}_{}",
        struct_reference.symbol_name, method_name
    )
}
