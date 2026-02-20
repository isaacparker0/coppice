use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use compiler__executable_program::{
    ExecutableBinaryOperator, ExecutableCallTarget, ExecutableCallableReference,
    ExecutableExpression, ExecutableFunctionDeclaration, ExecutableProgram, ExecutableStatement,
    ExecutableTypeReference, ExecutableUnaryOperator,
};
use compiler__reports::{CompilerFailure, CompilerFailureKind};
use compiler__runtime_interface::{ABORT_FUNCTION_CONTRACT, PRINT_FUNCTION_CONTRACT};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{AbiParam, Block, InstBuilder, MemFlags, Signature, Value, types};
use cranelift_codegen::isa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{FuncId, Linkage, Module, default_libcall_names};
use cranelift_native as native_isa;
use cranelift_object::{ObjectBuilder, ObjectModule};
use runfiles::Runfiles;

pub struct BuiltCraneliftProgram {
    pub binary_path: PathBuf,
}

pub struct BuildArtifactIdentity {
    pub executable_stem: String,
}

#[derive(Clone)]
struct FunctionRecord {
    id: FuncId,
    declaration: ExecutableFunctionDeclaration,
}

#[derive(Clone, Copy)]
struct ExternalRuntimeFunctions {
    write: FuncId,
    strlen: FuncId,
    exit: FuncId,
    malloc: FuncId,
}

#[derive(Clone)]
struct TypedValue {
    value: Option<Value>,
    type_reference: ExecutableTypeReference,
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
    loop_context: Option<LoopContext>,
}

struct CompilationState {
    module: ObjectModule,
    function_record_by_callable_reference: BTreeMap<ExecutableCallableReference, FunctionRecord>,
    external_runtime_functions: ExternalRuntimeFunctions,
}

pub fn build_program(
    program: &ExecutableProgram,
    build_directory: &Path,
    artifact_identity: &BuildArtifactIdentity,
) -> Result<BuiltCraneliftProgram, CompilerFailure> {
    fs::create_dir_all(build_directory).map_err(|error| {
        build_failed(
            format!("failed to create build output directory: {error}"),
            Some(build_directory),
        )
    })?;

    ensure_program_supported(program)?;

    let executable_path = build_directory.join(&artifact_identity.executable_stem);
    let object_path = build_directory.join(format!("{}.o", artifact_identity.executable_stem));

    let object_bytes = emit_object_bytes(program)?;
    fs::write(&object_path, object_bytes).map_err(|error| {
        build_failed(
            format!("failed to write object file: {error}"),
            Some(&object_path),
        )
    })?;

    link_executable(&object_path, &executable_path)?;

    fs::remove_file(&object_path).map_err(|error| {
        build_failed(
            format!("failed to remove intermediate object file: {error}"),
            Some(&object_path),
        )
    })?;

    Ok(BuiltCraneliftProgram {
        binary_path: executable_path,
    })
}

pub fn run_program(binary_path: &Path) -> Result<i32, CompilerFailure> {
    let status = Command::new(binary_path)
        .status()
        .map_err(|error| run_failed(format!("failed to execute binary: {error}"), Some(binary_path)))?;
    Ok(status.code().unwrap_or(1))
}

fn ensure_program_supported(program: &ExecutableProgram) -> Result<(), CompilerFailure> {
    if !program.struct_declarations.is_empty() {
        return Err(build_failed(
            "AOT Cranelift backend does not support structs/methods yet".to_string(),
            None,
        ));
    }

    for function_declaration in &program.function_declarations {
        if !function_declaration.type_parameter_names.is_empty() {
            return Err(build_failed(
                format!(
                    "AOT Cranelift backend does not support generic functions yet: '{}::{}'",
                    function_declaration.callable_reference.package_path,
                    function_declaration.callable_reference.symbol_name
                ),
                None,
            ));
        }

        for parameter in &function_declaration.parameters {
            ensure_type_supported(&parameter.type_reference)?;
        }
        ensure_type_supported(&function_declaration.return_type)?;

        for statement in &function_declaration.statements {
            ensure_statement_supported(statement)?;
        }
    }

    Ok(())
}

fn ensure_type_supported(type_reference: &ExecutableTypeReference) -> Result<(), CompilerFailure> {
    match type_reference {
        ExecutableTypeReference::Int64
        | ExecutableTypeReference::Boolean
        | ExecutableTypeReference::String
        | ExecutableTypeReference::Nil
        | ExecutableTypeReference::Never => Ok(()),
        _ => Err(build_failed(
            "AOT Cranelift backend currently supports only int64/boolean/string/nil/never"
                .to_string(),
            None,
        )),
    }
}

fn ensure_statement_supported(statement: &ExecutableStatement) -> Result<(), CompilerFailure> {
    match statement {
        ExecutableStatement::Binding { initializer, .. }
        | ExecutableStatement::Assign {
            value: initializer, ..
        }
        | ExecutableStatement::Expression {
            expression: initializer,
        }
        | ExecutableStatement::Return { value: initializer } => {
            ensure_expression_supported(initializer)
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
        | ExecutableExpression::Identifier { .. } => Ok(()),
        ExecutableExpression::Unary { expression, .. } => ensure_expression_supported(expression),
        ExecutableExpression::Binary { left, right, .. } => {
            ensure_expression_supported(left)?;
            ensure_expression_supported(right)
        }
        ExecutableExpression::Call {
            call_target,
            arguments,
            type_arguments,
            ..
        } => {
            if call_target.is_none() {
                return Err(build_failed(
                    "AOT Cranelift backend requires resolved call target metadata".to_string(),
                    None,
                ));
            }
            if !type_arguments.is_empty() {
                return Err(build_failed(
                    "AOT Cranelift backend does not support generic call type arguments"
                        .to_string(),
                    None,
                ));
            }
            for argument in arguments {
                ensure_expression_supported(argument)?;
            }
            Ok(())
        }
        _ => Err(build_failed(
            "AOT Cranelift backend does not support this expression yet".to_string(),
            None,
        )),
    }
}

fn emit_object_bytes(program: &ExecutableProgram) -> Result<Vec<u8>, CompilerFailure> {
    let isa = create_native_isa()?;
    let object_builder =
        ObjectBuilder::new(isa, "coppice", default_libcall_names()).map_err(|error| {
            build_failed(
                format!("failed to initialize Cranelift object builder: {error}"),
                None,
            )
        })?;
    let mut module = ObjectModule::new(object_builder);

    let external_runtime_functions = declare_external_runtime_functions(&mut module)?;
    let function_record_by_callable_reference =
        declare_program_functions(&mut module, &program.function_declarations)?;

    let mut state = CompilationState {
        module,
        function_record_by_callable_reference,
        external_runtime_functions,
    };

    for function_declaration in &program.function_declarations {
        define_program_function(&mut state, function_declaration)?;
    }

    define_process_entrypoint(&mut state, &program.entrypoint_callable_reference)?;

    let product = state.module.finish();
    product
        .emit()
        .map_err(|error| build_failed(format!("failed to emit object bytes: {error}"), None))
}

fn create_native_isa() -> Result<Arc<dyn isa::TargetIsa>, CompilerFailure> {
    let mut flag_builder = settings::builder();
    flag_builder
        .set("opt_level", "speed")
        .map_err(|error| build_failed(format!("failed to set optimization level: {error}"), None))?;
    flag_builder
        .set("is_pic", "true")
        .map_err(|error| build_failed(format!("failed to enable PIC: {error}"), None))?;

    let isa_builder = native_isa::builder()
        .map_err(|error| build_failed(format!("failed to create native ISA builder: {error}"), None))?;

    isa_builder
        .finish(settings::Flags::new(flag_builder))
        .map_err(|error| build_failed(format!("failed to finalize native ISA: {error}"), None))
}

fn declare_external_runtime_functions(
    module: &mut ObjectModule,
) -> Result<ExternalRuntimeFunctions, CompilerFailure> {
    let mut write_signature = module.make_signature();
    write_signature.params.push(AbiParam::new(types::I32));
    write_signature.params.push(AbiParam::new(types::I64));
    write_signature.params.push(AbiParam::new(types::I64));
    write_signature.returns.push(AbiParam::new(types::I64));
    let write = module
        .declare_function("write", Linkage::Import, &write_signature)
        .map_err(|error| build_failed(format!("failed to declare 'write': {error}"), None))?;

    let mut strlen_signature = module.make_signature();
    strlen_signature.params.push(AbiParam::new(types::I64));
    strlen_signature.returns.push(AbiParam::new(types::I64));
    let strlen = module
        .declare_function("strlen", Linkage::Import, &strlen_signature)
        .map_err(|error| build_failed(format!("failed to declare 'strlen': {error}"), None))?;

    let mut exit_signature = module.make_signature();
    exit_signature.params.push(AbiParam::new(types::I32));
    let exit = module
        .declare_function("exit", Linkage::Import, &exit_signature)
        .map_err(|error| build_failed(format!("failed to declare 'exit': {error}"), None))?;

    let mut malloc_signature = module.make_signature();
    malloc_signature.params.push(AbiParam::new(types::I64));
    malloc_signature.returns.push(AbiParam::new(types::I64));
    let malloc = module
        .declare_function("malloc", Linkage::Import, &malloc_signature)
        .map_err(|error| build_failed(format!("failed to declare 'malloc': {error}"), None))?;

    Ok(ExternalRuntimeFunctions {
        write,
        strlen,
        exit,
        malloc,
    })
}

fn declare_program_functions(
    module: &mut ObjectModule,
    function_declarations: &[ExecutableFunctionDeclaration],
) -> Result<BTreeMap<ExecutableCallableReference, FunctionRecord>, CompilerFailure> {
    let mut function_record_by_callable_reference = BTreeMap::new();

    for function_declaration in function_declarations {
        let signature = build_signature_for_function(module, function_declaration)?;
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
                declaration: function_declaration.clone(),
            },
        );
    }

    Ok(function_record_by_callable_reference)
}

fn build_signature_for_function(
    module: &mut ObjectModule,
    function_declaration: &ExecutableFunctionDeclaration,
) -> Result<Signature, CompilerFailure> {
    let mut signature = module.make_signature();
    for parameter in &function_declaration.parameters {
        signature
            .params
            .push(AbiParam::new(cranelift_type_for(&parameter.type_reference)?));
    }

    if !matches!(
        function_declaration.return_type,
        ExecutableTypeReference::Nil | ExecutableTypeReference::Never
    ) {
        signature
            .returns
            .push(AbiParam::new(cranelift_type_for(&function_declaration.return_type)?));
    }

    Ok(signature)
}

fn cranelift_type_for(type_reference: &ExecutableTypeReference) -> Result<types::Type, CompilerFailure> {
    match type_reference {
        ExecutableTypeReference::Int64 | ExecutableTypeReference::String => Ok(types::I64),
        ExecutableTypeReference::Boolean
        | ExecutableTypeReference::Nil
        | ExecutableTypeReference::Never => Ok(types::I8),
        _ => Err(build_failed(
            "unsupported type in Cranelift lowering".to_string(),
            None,
        )),
    }
}

fn define_program_function(
    state: &mut CompilationState,
    function_declaration: &ExecutableFunctionDeclaration,
) -> Result<(), CompilerFailure> {
    let function_record = state
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
        .clone();

    let mut context = state.module.make_context();
    context.func.signature = build_signature_for_function(&mut state.module, function_declaration)?;

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
                    .iconst(cranelift_type_for(&function_declaration.return_type)?, 0);
                function_builder.ins().return_(&[default_value]);
            }
        }

        function_builder.finalize();
    }

    state
        .module
        .define_function(function_record.id, &mut context)
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

fn define_process_entrypoint(
    state: &mut CompilationState,
    entrypoint_callable_reference: &ExecutableCallableReference,
) -> Result<(), CompilerFailure> {
    let entrypoint_record = state
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
        .clone();

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
            .declare_func_in_func(entrypoint_record.id, function_builder.func);
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
    state: &mut CompilationState,
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
                let Some(value) = initializer.value else {
                    return Err(build_failed(
                        format!("initializer for '{name}' produced no runtime value"),
                        None,
                    ));
                };
                let local_value = declare_local_variable(
                    function_builder,
                    value,
                    initializer.type_reference,
                );
                compilation_context
                    .local_value_by_name
                    .insert(name.clone(), local_value);
            }
            ExecutableStatement::Assign { name, value } => {
                let local_value = compilation_context
                    .local_value_by_name
                    .get(name)
                    .ok_or_else(|| build_failed(format!("unknown local '{name}'"), None))?
                    .clone();
                let assigned_value =
                    compile_expression(state, function_builder, compilation_context, value)?;
                if local_value.type_reference != assigned_value.type_reference {
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
                function_builder.def_var(local_value.variable, value);
            }
            ExecutableStatement::If {
                condition,
                then_statements,
                else_statements,
            } => {
                let condition_typed_value =
                    compile_expression(state, function_builder, compilation_context, condition)?;
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
                    let condition_typed_value =
                        compile_expression(state, function_builder, compilation_context, condition)?;
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
                    function_builder
                        .ins()
                        .brif(condition_is_true, body_block, &[], exit_block, &[]);
                } else {
                    function_builder.ins().jump(body_block, &[]);
                }
                function_builder.seal_block(header_block);

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
                let _ = compile_expression(state, function_builder, compilation_context, expression)?;
            }
            ExecutableStatement::Return { value } => {
                let typed_return =
                    compile_expression(state, function_builder, compilation_context, value)?;
                if typed_return.type_reference != *function_return_type {
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
                    let Some(value) = typed_return.value else {
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
    state: &mut CompilationState,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    expression: &ExecutableExpression,
) -> Result<TypedValue, CompilerFailure> {
    match expression {
        ExecutableExpression::IntegerLiteral { value } => Ok(TypedValue {
            value: Some(function_builder.ins().iconst(types::I64, *value)),
            type_reference: ExecutableTypeReference::Int64,
        }),
        ExecutableExpression::BooleanLiteral { value } => Ok(TypedValue {
            value: Some(function_builder.ins().iconst(types::I8, i64::from(*value))),
            type_reference: ExecutableTypeReference::Boolean,
        }),
        ExecutableExpression::NilLiteral => Ok(TypedValue {
            value: None,
            type_reference: ExecutableTypeReference::Nil,
        }),
        ExecutableExpression::StringLiteral { value } => Ok(TypedValue {
            value: Some(intern_string_literal(state, function_builder, value)?),
            type_reference: ExecutableTypeReference::String,
        }),
        ExecutableExpression::Identifier { name } => {
            let local_value = compilation_context
                .local_value_by_name
                .get(name)
                .ok_or_else(|| build_failed(format!("unknown local '{name}'"), None))?
                .clone();
            Ok(TypedValue {
                value: Some(function_builder.use_var(local_value.variable)),
                type_reference: local_value.type_reference,
            })
        }
        ExecutableExpression::Unary {
            operator,
            expression,
        } => {
            let operand = compile_expression(state, function_builder, compilation_context, expression)?;
            let operand_value = operand.value.ok_or_else(|| {
                build_failed("unary operator operand produced no runtime value".to_string(), None)
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
            call_target,
            arguments,
            ..
        } => compile_call_expression(
            state,
            function_builder,
            compilation_context,
            call_target.as_ref(),
            arguments,
        ),
        _ => Err(build_failed(
            "AOT Cranelift backend does not support this expression yet".to_string(),
            None,
        )),
    }
}

fn compile_binary_expression(
    state: &mut CompilationState,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    operator: ExecutableBinaryOperator,
    left: &ExecutableExpression,
    right: &ExecutableExpression,
) -> Result<TypedValue, CompilerFailure> {
    let left_typed_value = compile_expression(state, function_builder, compilation_context, left)?;
    let right_typed_value = compile_expression(state, function_builder, compilation_context, right)?;

    let left_value = left_typed_value
        .value
        .ok_or_else(|| build_failed("binary left operand produced no runtime value".to_string(), None))?;
    let right_value = right_typed_value
        .value
        .ok_or_else(|| build_failed("binary right operand produced no runtime value".to_string(), None))?;

    match operator {
        ExecutableBinaryOperator::Add
        | ExecutableBinaryOperator::Subtract
        | ExecutableBinaryOperator::Multiply
        | ExecutableBinaryOperator::Divide
        | ExecutableBinaryOperator::LessThan
        | ExecutableBinaryOperator::LessThanOrEqual
        | ExecutableBinaryOperator::GreaterThan
        | ExecutableBinaryOperator::GreaterThanOrEqual => {
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
                ExecutableBinaryOperator::Add => Ok(TypedValue {
                    value: Some(function_builder.ins().iadd(left_value, right_value)),
                    type_reference: ExecutableTypeReference::Int64,
                }),
                ExecutableBinaryOperator::Subtract => Ok(TypedValue {
                    value: Some(function_builder.ins().isub(left_value, right_value)),
                    type_reference: ExecutableTypeReference::Int64,
                }),
                ExecutableBinaryOperator::Multiply => Ok(TypedValue {
                    value: Some(function_builder.ins().imul(left_value, right_value)),
                    type_reference: ExecutableTypeReference::Int64,
                }),
                ExecutableBinaryOperator::Divide => Ok(TypedValue {
                    value: Some(function_builder.ins().sdiv(left_value, right_value)),
                    type_reference: ExecutableTypeReference::Int64,
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
                    let condition = function_builder
                        .ins()
                        .icmp(condition_code, left_value, right_value);
                    let one = function_builder.ins().iconst(types::I8, 1);
                    let zero = function_builder.ins().iconst(types::I8, 0);
                    let bool_value = function_builder.ins().select(condition, one, zero);
                    Ok(TypedValue {
                        value: Some(bool_value),
                        type_reference: ExecutableTypeReference::Boolean,
                    })
                }
                _ => unreachable!(),
            }
        }
        ExecutableBinaryOperator::EqualEqual | ExecutableBinaryOperator::NotEqual => {
            if left_typed_value.type_reference != right_typed_value.type_reference {
                return Err(build_failed(
                    "equality operators require operands of identical type".to_string(),
                    None,
                ));
            }
            let condition_code = if matches!(operator, ExecutableBinaryOperator::EqualEqual) {
                IntCC::Equal
            } else {
                IntCC::NotEqual
            };
            let condition = function_builder
                .ins()
                .icmp(condition_code, left_value, right_value);
            let one = function_builder.ins().iconst(types::I8, 1);
            let zero = function_builder.ins().iconst(types::I8, 0);
            let bool_value = function_builder.ins().select(condition, one, zero);
            Ok(TypedValue {
                value: Some(bool_value),
                type_reference: ExecutableTypeReference::Boolean,
            })
        }
        ExecutableBinaryOperator::And | ExecutableBinaryOperator::Or => {
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
            })
        }
    }
}

fn compile_call_expression(
    state: &mut CompilationState,
    function_builder: &mut FunctionBuilder<'_>,
    compilation_context: &mut FunctionCompilationContext,
    call_target: Option<&ExecutableCallTarget>,
    arguments: &[ExecutableExpression],
) -> Result<TypedValue, CompilerFailure> {
    let Some(call_target) = call_target else {
        return Err(build_failed(
            "AOT Cranelift backend requires resolved call target metadata".to_string(),
            None,
        ));
    };

    match call_target {
        ExecutableCallTarget::BuiltinFunction { function_name } => {
            if function_name == PRINT_FUNCTION_CONTRACT.language_name {
                if arguments.len() != 1 {
                    return Err(build_failed(
                        "print(...) requires exactly one argument".to_string(),
                        None,
                    ));
                }
                let argument =
                    compile_expression(state, function_builder, compilation_context, &arguments[0])?;
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
                });
            }

            if function_name == ABORT_FUNCTION_CONTRACT.language_name {
                if arguments.len() != 1 {
                    return Err(build_failed(
                        "abort(...) requires exactly one argument".to_string(),
                        None,
                    ));
                }
                let argument =
                    compile_expression(state, function_builder, compilation_context, &arguments[0])?;
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
                });
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
                })?
                .clone();

            if function_record.declaration.parameters.len() != arguments.len() {
                return Err(build_failed(
                    format!(
                        "function '{}::{}' expected {} argument(s), got {}",
                        callable_reference.package_path,
                        callable_reference.symbol_name,
                        function_record.declaration.parameters.len(),
                        arguments.len()
                    ),
                    None,
                ));
            }

            let mut argument_values = Vec::new();
            for (parameter, argument_expression) in function_record
                .declaration
                .parameters
                .iter()
                .zip(arguments)
            {
                let argument =
                    compile_expression(state, function_builder, compilation_context, argument_expression)?;
                if argument.type_reference != parameter.type_reference {
                    return Err(build_failed(
                        format!(
                            "call argument type mismatch for function '{}::{}'",
                            callable_reference.package_path, callable_reference.symbol_name
                        ),
                        None,
                    ));
                }
                let value = argument.value.ok_or_else(|| {
                    build_failed("call argument produced no runtime value".to_string(), None)
                })?;
                argument_values.push(value);
            }

            let callee = state
                .module
                .declare_func_in_func(function_record.id, function_builder.func);
            let call = function_builder.ins().call(callee, &argument_values);

            if matches!(
                function_record.declaration.return_type,
                ExecutableTypeReference::Nil | ExecutableTypeReference::Never
            ) {
                Ok(TypedValue {
                    value: None,
                    type_reference: function_record.declaration.return_type,
                })
            } else {
                let results = function_builder.inst_results(call);
                Ok(TypedValue {
                    value: Some(results[0]),
                    type_reference: function_record.declaration.return_type,
                })
            }
        }
    }
}

fn emit_write_string_with_newline(
    state: &mut CompilationState,
    function_builder: &mut FunctionBuilder<'_>,
    file_descriptor: i32,
    string_pointer: Value,
) -> Result<(), CompilerFailure> {
    let strlen = state
        .module
        .declare_func_in_func(state.external_runtime_functions.strlen, function_builder.func);
    let strlen_call = function_builder.ins().call(strlen, &[string_pointer]);
    let length = function_builder.inst_results(strlen_call)[0];

    let write = state
        .module
        .declare_func_in_func(state.external_runtime_functions.write, function_builder.func);
    let file_descriptor = function_builder
        .ins()
        .iconst(types::I32, i64::from(file_descriptor));
    let _ = function_builder
        .ins()
        .call(write, &[file_descriptor, string_pointer, length]);

    let newline_pointer = intern_string_literal(state, function_builder, "\n")?;
    let one = function_builder.ins().iconst(types::I64, 1);
    let _ = function_builder
        .ins()
        .call(write, &[file_descriptor, newline_pointer, one]);

    Ok(())
}

fn emit_exit_call(
    state: &mut CompilationState,
    function_builder: &mut FunctionBuilder<'_>,
    exit_code: i32,
) {
    let exit = state
        .module
        .declare_func_in_func(state.external_runtime_functions.exit, function_builder.func);
    let exit_code = function_builder.ins().iconst(types::I32, i64::from(exit_code));
    let _ = function_builder.ins().call(exit, &[exit_code]);
    function_builder.ins().return_(&[]);
}

fn intern_string_literal(
    state: &mut CompilationState,
    function_builder: &mut FunctionBuilder<'_>,
    value: &str,
) -> Result<Value, CompilerFailure> {
    let total_size = value.len() + 1;
    let total_size_value = function_builder.ins().iconst(
        types::I64,
        i64::try_from(total_size).map_err(|_| {
            build_failed(
                "string literal is too large to allocate in AOT backend".to_string(),
                None,
            )
        })?,
    );

    let malloc = state
        .module
        .declare_func_in_func(state.external_runtime_functions.malloc, function_builder.func);
    let malloc_call = function_builder.ins().call(malloc, &[total_size_value]);
    let pointer = function_builder.inst_results(malloc_call)[0];

    let mem_flags = MemFlags::new();
    for (index, byte) in value.as_bytes().iter().enumerate() {
        let byte_value = function_builder.ins().iconst(types::I8, i64::from(*byte));
        function_builder.ins().store(
            mem_flags,
            byte_value,
            pointer,
            i32::try_from(index).map_err(|_| {
                build_failed(
                    "string literal index overflow in AOT backend".to_string(),
                    None,
                )
            })?,
        );
    }

    let terminator_offset = i32::try_from(value.len()).map_err(|_| {
        build_failed(
            "string literal terminator offset overflow in AOT backend".to_string(),
            None,
        )
    })?;
    let terminator = function_builder.ins().iconst(types::I8, 0);
    function_builder
        .ins()
        .store(mem_flags, terminator, pointer, terminator_offset);

    Ok(pointer)
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

fn link_executable(object_path: &Path, executable_path: &Path) -> Result<(), CompilerFailure> {
    let linker_path = resolve_hermetic_linker_path()?;

    let output = Command::new(&linker_path)
        .arg(object_path)
        .arg("-o")
        .arg(executable_path)
        .arg("-fuse-ld=lld")
        .output()
        .map_err(|error| {
            build_failed(
                format!(
                    "failed to invoke hermetic linker driver '{}': {error}",
                    linker_path.display()
                ),
                Some(executable_path),
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(build_failed(
            format!(
                "system linker failed with status {}{}{}",
                output.status,
                if stderr.trim().is_empty() { "" } else { ": " },
                stderr.trim()
            ),
            Some(executable_path),
        ));
    }

    Ok(())
}

fn resolve_hermetic_linker_path() -> Result<PathBuf, CompilerFailure> {
    let runfiles = Runfiles::create().map_err(|error| {
        build_failed(
            format!("failed to initialize runfiles for hermetic linker: {error}"),
            None,
        )
    })?;

    runfiles
        .rlocation_from(env!("COPPICE_LLVM_CLANGXX"), env!("REPOSITORY_NAME"))
        .ok_or_else(|| {
            build_failed(
                format!(
                    "failed to resolve hermetic linker runfile: {}",
                    env!("COPPICE_LLVM_CLANGXX")
                ),
                None,
            )
        })
}

fn build_failed(message: String, path: Option<&Path>) -> CompilerFailure {
    CompilerFailure {
        kind: CompilerFailureKind::BuildFailed,
        message,
        path: path.map(|path| path.display().to_string()),
        details: Vec::new(),
    }
}

fn run_failed(message: String, path: Option<&Path>) -> CompilerFailure {
    CompilerFailure {
        kind: CompilerFailureKind::RunFailed,
        message,
        path: path.map(|path| path.display().to_string()),
        details: Vec::new(),
    }
}
