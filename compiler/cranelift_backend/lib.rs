use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use compiler__executable_program::{
    ExecutableBinaryOperator, ExecutableExpression, ExecutableFunctionDeclaration,
    ExecutableProgram, ExecutableStatement, ExecutableStructDeclaration, ExecutableTypeReference,
    ExecutableUnaryOperator,
};
use compiler__reports::{CompilerFailure, CompilerFailureKind};
use compiler__runtime_interface::{
    ABORT_FUNCTION_CONTRACT, PRINT_FUNCTION_CONTRACT, USER_ENTRYPOINT_FUNCTION_NAME,
};
use cranelift_codegen::ir::{AbiParam, InstBuilder, types};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module, default_libcall_names};
use serde::{Deserialize, Serialize};

pub struct BuiltCraneliftProgram {
    pub binary_path: PathBuf,
}

pub struct BuildArtifactIdentity {
    pub executable_stem: String,
}

#[derive(Serialize, Deserialize)]
struct ExecutableArtifact {
    executable_program: ExecutableProgram,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RuntimeValue {
    Int64(i64),
    Boolean(bool),
    String(String),
    Nil,
    StructInstance(RuntimeStructInstance),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RuntimeStructInstance {
    type_name: String,
    field_value_by_name: BTreeMap<String, RuntimeValue>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StatementExecutionSignal {
    Next,
    Continue,
    Break,
    Return,
}

enum RuntimeExecutionError {
    Abort { exit_code: i32 },
    Failure(CompilerFailure),
}

struct ProgramExecutionContext<'a> {
    function_declaration_by_name: BTreeMap<&'a str, &'a ExecutableFunctionDeclaration>,
    struct_declaration_by_name: BTreeMap<&'a str, &'a ExecutableStructDeclaration>,
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

    validate_program_with_cranelift(program)?;

    let artifact_path = build_directory.join(&artifact_identity.executable_stem);
    let artifact = ExecutableArtifact {
        executable_program: program.clone(),
    };
    let serialized_artifact = serde_json::to_string(&artifact).map_err(|error| {
        build_failed(
            format!("failed to serialize build artifact: {error}"),
            Some(&artifact_path),
        )
    })?;
    fs::write(&artifact_path, serialized_artifact).map_err(|error| {
        build_failed(
            format!("failed to write build artifact: {error}"),
            Some(&artifact_path),
        )
    })?;

    Ok(BuiltCraneliftProgram {
        binary_path: artifact_path,
    })
}

pub fn run_program(binary_path: &Path) -> Result<i32, CompilerFailure> {
    let artifact_source = fs::read_to_string(binary_path).map_err(|error| CompilerFailure {
        kind: CompilerFailureKind::RunFailed,
        message: format!("failed to read build artifact: {error}"),
        path: Some(binary_path.display().to_string()),
        details: Vec::new(),
    })?;
    let artifact: ExecutableArtifact =
        serde_json::from_str(&artifact_source).map_err(|error| CompilerFailure {
            kind: CompilerFailureKind::RunFailed,
            message: format!("failed to parse build artifact: {error}"),
            path: Some(binary_path.display().to_string()),
            details: Vec::new(),
        })?;

    let program_execution_context = build_program_execution_context(&artifact.executable_program);
    match execute_main_function(&program_execution_context) {
        Ok(()) => Ok(0),
        Err(RuntimeExecutionError::Abort { exit_code }) => Ok(exit_code),
        Err(RuntimeExecutionError::Failure(failure)) => Err(failure),
    }
}

fn validate_program_with_cranelift(program: &ExecutableProgram) -> Result<(), CompilerFailure> {
    let jit_builder = JITBuilder::new(default_libcall_names()).map_err(|error| {
        build_failed(
            format!("failed to initialize Cranelift JIT builder: {error}"),
            None,
        )
    })?;
    let mut jit_module = JITModule::new(jit_builder);

    for function_declaration in &program.function_declarations {
        define_stub_function_for_validation(
            &mut jit_module,
            &format!("coppice_{}", function_declaration.name),
            &function_declaration
                .parameters
                .iter()
                .map(|parameter| parameter.type_reference.clone())
                .collect::<Vec<_>>(),
            &function_declaration.return_type,
        )?;
    }

    for struct_declaration in &program.struct_declarations {
        for method_declaration in &struct_declaration.methods {
            let mut parameter_types = vec![ExecutableTypeReference::Named {
                name: struct_declaration.name.clone(),
            }];
            for parameter in &method_declaration.parameters {
                parameter_types.push(parameter.type_reference.clone());
            }
            define_stub_function_for_validation(
                &mut jit_module,
                &format!(
                    "coppice_{}_{}",
                    struct_declaration.name, method_declaration.name
                ),
                &parameter_types,
                &method_declaration.return_type,
            )?;
        }
    }

    Ok(())
}

fn define_stub_function_for_validation(
    jit_module: &mut JITModule,
    function_name: &str,
    parameter_types: &[ExecutableTypeReference],
    return_type: &ExecutableTypeReference,
) -> Result<(), CompilerFailure> {
    let mut signature = jit_module.make_signature();
    for parameter_type in parameter_types {
        signature
            .params
            .push(AbiParam::new(cranelift_type_for(parameter_type)));
    }

    if !matches!(
        return_type,
        ExecutableTypeReference::Nil | ExecutableTypeReference::Never
    ) {
        signature
            .returns
            .push(AbiParam::new(cranelift_type_for(return_type)));
    }

    let function_id = jit_module
        .declare_function(function_name, Linkage::Local, &signature)
        .map_err(|error| {
            build_failed(
                format!("failed to declare Cranelift function '{function_name}': {error}"),
                None,
            )
        })?;

    let mut context = jit_module.make_context();
    context.func.signature = signature;
    let mut function_builder_context = FunctionBuilderContext::new();
    {
        let mut function_builder =
            FunctionBuilder::new(&mut context.func, &mut function_builder_context);
        let entry_block = function_builder.create_block();
        function_builder.append_block_params_for_function_params(entry_block);
        function_builder.switch_to_block(entry_block);
        function_builder.seal_block(entry_block);

        if matches!(
            return_type,
            ExecutableTypeReference::Nil | ExecutableTypeReference::Never
        ) {
            function_builder.ins().return_(&[]);
        } else {
            let return_value = zero_value_for_type(&mut function_builder, return_type);
            function_builder.ins().return_(&[return_value]);
        }

        function_builder.finalize();
    }

    jit_module
        .define_function(function_id, &mut context)
        .map_err(|error| {
            build_failed(
                format!("failed to define Cranelift function '{function_name}': {error}"),
                None,
            )
        })?;
    jit_module.clear_context(&mut context);

    Ok(())
}

fn cranelift_type_for(type_reference: &ExecutableTypeReference) -> types::Type {
    match type_reference {
        ExecutableTypeReference::Boolean
        | ExecutableTypeReference::Nil
        | ExecutableTypeReference::Never => types::I8,
        ExecutableTypeReference::Int64
        | ExecutableTypeReference::String
        | ExecutableTypeReference::Named { .. } => types::I64,
    }
}

fn zero_value_for_type(
    function_builder: &mut FunctionBuilder<'_>,
    type_reference: &ExecutableTypeReference,
) -> cranelift_codegen::ir::Value {
    function_builder
        .ins()
        .iconst(cranelift_type_for(type_reference), 0)
}

fn build_program_execution_context(program: &ExecutableProgram) -> ProgramExecutionContext<'_> {
    let mut function_declaration_by_name = BTreeMap::new();
    for function_declaration in &program.function_declarations {
        function_declaration_by_name
            .insert(function_declaration.name.as_str(), function_declaration);
    }

    let mut struct_declaration_by_name = BTreeMap::new();
    for struct_declaration in &program.struct_declarations {
        struct_declaration_by_name.insert(struct_declaration.name.as_str(), struct_declaration);
    }

    ProgramExecutionContext {
        function_declaration_by_name,
        struct_declaration_by_name,
    }
}

fn execute_main_function(
    program_execution_context: &ProgramExecutionContext<'_>,
) -> Result<(), RuntimeExecutionError> {
    let mut local_value_by_name = BTreeMap::new();
    let mut return_value = RuntimeValue::Nil;
    execute_function_by_name(
        program_execution_context,
        USER_ENTRYPOINT_FUNCTION_NAME,
        &[],
        &mut local_value_by_name,
        &mut return_value,
    )?;
    Ok(())
}

fn execute_function_by_name(
    program_execution_context: &ProgramExecutionContext<'_>,
    function_name: &str,
    argument_values: &[RuntimeValue],
    local_value_by_name: &mut BTreeMap<String, RuntimeValue>,
    return_value: &mut RuntimeValue,
) -> Result<(), RuntimeExecutionError> {
    let Some(function_declaration) = program_execution_context
        .function_declaration_by_name
        .get(function_name)
    else {
        return Err(RuntimeExecutionError::Failure(run_failed(
            format!("unknown function '{function_name}'"),
            None,
        )));
    };

    if function_declaration.parameters.len() != argument_values.len() {
        return Err(RuntimeExecutionError::Failure(run_failed(
            format!(
                "function '{function_name}' expected {} argument(s) but got {}",
                function_declaration.parameters.len(),
                argument_values.len()
            ),
            None,
        )));
    }

    local_value_by_name.clear();
    for (parameter, argument_value) in function_declaration.parameters.iter().zip(argument_values) {
        local_value_by_name.insert(parameter.name.clone(), argument_value.clone());
    }

    let statement_signal = execute_statements(
        program_execution_context,
        &function_declaration.statements,
        local_value_by_name,
        return_value,
    )?;
    if matches!(
        statement_signal,
        StatementExecutionSignal::Break | StatementExecutionSignal::Continue
    ) {
        return Err(RuntimeExecutionError::Failure(run_failed(
            format!("function '{function_name}' contains invalid loop control flow"),
            None,
        )));
    }

    Ok(())
}

fn execute_method(
    program_execution_context: &ProgramExecutionContext<'_>,
    struct_instance: &RuntimeStructInstance,
    method_name: &str,
    argument_values: &[RuntimeValue],
    return_value: &mut RuntimeValue,
) -> Result<RuntimeStructInstance, RuntimeExecutionError> {
    let Some(struct_declaration) = program_execution_context
        .struct_declaration_by_name
        .get(struct_instance.type_name.as_str())
    else {
        return Err(RuntimeExecutionError::Failure(run_failed(
            format!("unknown struct '{}'", struct_instance.type_name),
            None,
        )));
    };

    let Some(method_declaration) = struct_declaration
        .methods
        .iter()
        .find(|method_declaration| method_declaration.name == method_name)
    else {
        return Err(RuntimeExecutionError::Failure(run_failed(
            format!(
                "unknown method '{}.{}'",
                struct_instance.type_name, method_name
            ),
            None,
        )));
    };

    if method_declaration.parameters.len() != argument_values.len() {
        return Err(RuntimeExecutionError::Failure(run_failed(
            format!(
                "method '{}.{}' expected {} argument(s) but got {}",
                struct_instance.type_name,
                method_name,
                method_declaration.parameters.len(),
                argument_values.len()
            ),
            None,
        )));
    }

    let mut method_local_value_by_name = BTreeMap::new();
    method_local_value_by_name.insert(
        "self".to_string(),
        RuntimeValue::StructInstance(struct_instance.clone()),
    );
    for (parameter, argument_value) in method_declaration.parameters.iter().zip(argument_values) {
        method_local_value_by_name.insert(parameter.name.clone(), argument_value.clone());
    }

    *return_value = RuntimeValue::Nil;
    let statement_signal = execute_statements(
        program_execution_context,
        &method_declaration.statements,
        &mut method_local_value_by_name,
        return_value,
    )?;
    if matches!(
        statement_signal,
        StatementExecutionSignal::Break | StatementExecutionSignal::Continue
    ) {
        return Err(RuntimeExecutionError::Failure(run_failed(
            format!(
                "method '{}.{}' contains invalid loop control flow",
                struct_instance.type_name, method_name
            ),
            None,
        )));
    }

    let Some(RuntimeValue::StructInstance(updated_struct_instance)) =
        method_local_value_by_name.remove("self")
    else {
        return Err(RuntimeExecutionError::Failure(run_failed(
            format!(
                "method '{}.{}' did not preserve receiver value",
                struct_instance.type_name, method_name
            ),
            None,
        )));
    };

    Ok(updated_struct_instance)
}

fn execute_statements(
    program_execution_context: &ProgramExecutionContext<'_>,
    statements: &[ExecutableStatement],
    local_value_by_name: &mut BTreeMap<String, RuntimeValue>,
    return_value: &mut RuntimeValue,
) -> Result<StatementExecutionSignal, RuntimeExecutionError> {
    for statement in statements {
        match statement {
            ExecutableStatement::Binding {
                name, initializer, ..
            } => {
                let initializer_value = evaluate_expression(
                    program_execution_context,
                    initializer,
                    local_value_by_name,
                )?;
                local_value_by_name.insert(name.clone(), initializer_value);
            }
            ExecutableStatement::Assign { name, value } => {
                if !local_value_by_name.contains_key(name) {
                    return Err(RuntimeExecutionError::Failure(run_failed(
                        format!("unknown local '{name}'"),
                        None,
                    )));
                }
                let value_to_assign =
                    evaluate_expression(program_execution_context, value, local_value_by_name)?;
                local_value_by_name.insert(name.clone(), value_to_assign);
            }
            ExecutableStatement::If {
                condition,
                then_statements,
                else_statements,
            } => {
                let condition_value =
                    evaluate_expression(program_execution_context, condition, local_value_by_name)?;
                let condition_boolean = runtime_boolean_from_value(&condition_value)?;
                let statement_signal = if condition_boolean {
                    execute_statements(
                        program_execution_context,
                        then_statements,
                        local_value_by_name,
                        return_value,
                    )?
                } else if let Some(else_statements) = else_statements {
                    execute_statements(
                        program_execution_context,
                        else_statements,
                        local_value_by_name,
                        return_value,
                    )?
                } else {
                    StatementExecutionSignal::Next
                };
                if !matches!(statement_signal, StatementExecutionSignal::Next) {
                    return Ok(statement_signal);
                }
            }
            ExecutableStatement::For {
                condition,
                body_statements,
            } => loop {
                if let Some(condition) = condition {
                    let condition_value = evaluate_expression(
                        program_execution_context,
                        condition,
                        local_value_by_name,
                    )?;
                    if !runtime_boolean_from_value(&condition_value)? {
                        break;
                    }
                }

                match execute_statements(
                    program_execution_context,
                    body_statements,
                    local_value_by_name,
                    return_value,
                )? {
                    StatementExecutionSignal::Next | StatementExecutionSignal::Continue => {}
                    StatementExecutionSignal::Break => break,
                    StatementExecutionSignal::Return => {
                        return Ok(StatementExecutionSignal::Return);
                    }
                }
            },
            ExecutableStatement::Break => return Ok(StatementExecutionSignal::Break),
            ExecutableStatement::Continue => return Ok(StatementExecutionSignal::Continue),
            ExecutableStatement::Expression { expression } => {
                let _ = evaluate_expression(
                    program_execution_context,
                    expression,
                    local_value_by_name,
                )?;
            }
            ExecutableStatement::Return { value } => {
                *return_value =
                    evaluate_expression(program_execution_context, value, local_value_by_name)?;
                return Ok(StatementExecutionSignal::Return);
            }
        }
    }

    Ok(StatementExecutionSignal::Next)
}

fn evaluate_expression(
    program_execution_context: &ProgramExecutionContext<'_>,
    expression: &ExecutableExpression,
    local_value_by_name: &mut BTreeMap<String, RuntimeValue>,
) -> Result<RuntimeValue, RuntimeExecutionError> {
    match expression {
        ExecutableExpression::IntegerLiteral { value } => Ok(RuntimeValue::Int64(*value)),
        ExecutableExpression::BooleanLiteral { value } => Ok(RuntimeValue::Boolean(*value)),
        ExecutableExpression::NilLiteral => Ok(RuntimeValue::Nil),
        ExecutableExpression::StringLiteral { value } => Ok(RuntimeValue::String(value.clone())),
        ExecutableExpression::Identifier { name } => {
            local_value_by_name.get(name).cloned().ok_or_else(|| {
                RuntimeExecutionError::Failure(run_failed(format!("unknown local '{name}'"), None))
            })
        }
        ExecutableExpression::StructLiteral { type_name, fields } => {
            let mut field_value_by_name = BTreeMap::new();
            for field in fields {
                let field_value = evaluate_expression(
                    program_execution_context,
                    &field.value,
                    local_value_by_name,
                )?;
                field_value_by_name.insert(field.name.clone(), field_value);
            }
            Ok(RuntimeValue::StructInstance(RuntimeStructInstance {
                type_name: type_name.clone(),
                field_value_by_name,
            }))
        }
        ExecutableExpression::FieldAccess { target, field } => {
            let target_value =
                evaluate_expression(program_execution_context, target, local_value_by_name)?;
            let RuntimeValue::StructInstance(struct_instance) = target_value else {
                return Err(RuntimeExecutionError::Failure(run_failed(
                    "field access requires struct receiver".to_string(),
                    None,
                )));
            };
            struct_instance
                .field_value_by_name
                .get(field)
                .cloned()
                .ok_or_else(|| {
                    RuntimeExecutionError::Failure(run_failed(
                        format!("unknown field '{}.{}'", struct_instance.type_name, field),
                        None,
                    ))
                })
        }
        ExecutableExpression::Unary {
            operator,
            expression,
        } => {
            let expression_value =
                evaluate_expression(program_execution_context, expression, local_value_by_name)?;
            match operator {
                ExecutableUnaryOperator::Not => Ok(RuntimeValue::Boolean(
                    !runtime_boolean_from_value(&expression_value)?,
                )),
                ExecutableUnaryOperator::Negate => Ok(RuntimeValue::Int64(
                    -runtime_int64_from_value(&expression_value)?,
                )),
            }
        }
        ExecutableExpression::Binary {
            operator,
            left,
            right,
        } => evaluate_binary_expression(
            program_execution_context,
            *operator,
            left,
            right,
            local_value_by_name,
        ),
        ExecutableExpression::Call { callee, arguments } => evaluate_call_expression(
            program_execution_context,
            callee,
            arguments,
            local_value_by_name,
        ),
    }
}

fn evaluate_binary_expression(
    program_execution_context: &ProgramExecutionContext<'_>,
    operator: ExecutableBinaryOperator,
    left: &ExecutableExpression,
    right: &ExecutableExpression,
    local_value_by_name: &mut BTreeMap<String, RuntimeValue>,
) -> Result<RuntimeValue, RuntimeExecutionError> {
    match operator {
        ExecutableBinaryOperator::And => {
            let left_value =
                evaluate_expression(program_execution_context, left, local_value_by_name)?;
            let left_boolean = runtime_boolean_from_value(&left_value)?;
            if !left_boolean {
                return Ok(RuntimeValue::Boolean(false));
            }
            let right_value =
                evaluate_expression(program_execution_context, right, local_value_by_name)?;
            let right_boolean = runtime_boolean_from_value(&right_value)?;
            Ok(RuntimeValue::Boolean(right_boolean))
        }
        ExecutableBinaryOperator::Or => {
            let left_value =
                evaluate_expression(program_execution_context, left, local_value_by_name)?;
            let left_boolean = runtime_boolean_from_value(&left_value)?;
            if left_boolean {
                return Ok(RuntimeValue::Boolean(true));
            }
            let right_value =
                evaluate_expression(program_execution_context, right, local_value_by_name)?;
            let right_boolean = runtime_boolean_from_value(&right_value)?;
            Ok(RuntimeValue::Boolean(right_boolean))
        }
        _ => {
            let left_value =
                evaluate_expression(program_execution_context, left, local_value_by_name)?;
            let right_value =
                evaluate_expression(program_execution_context, right, local_value_by_name)?;
            match operator {
                ExecutableBinaryOperator::Add => Ok(RuntimeValue::Int64(
                    runtime_int64_from_value(&left_value)?
                        + runtime_int64_from_value(&right_value)?,
                )),
                ExecutableBinaryOperator::Subtract => Ok(RuntimeValue::Int64(
                    runtime_int64_from_value(&left_value)?
                        - runtime_int64_from_value(&right_value)?,
                )),
                ExecutableBinaryOperator::Multiply => Ok(RuntimeValue::Int64(
                    runtime_int64_from_value(&left_value)?
                        * runtime_int64_from_value(&right_value)?,
                )),
                ExecutableBinaryOperator::Divide => Ok(RuntimeValue::Int64(
                    runtime_int64_from_value(&left_value)?
                        / runtime_int64_from_value(&right_value)?,
                )),
                ExecutableBinaryOperator::EqualEqual => {
                    Ok(RuntimeValue::Boolean(left_value == right_value))
                }
                ExecutableBinaryOperator::NotEqual => {
                    Ok(RuntimeValue::Boolean(left_value != right_value))
                }
                ExecutableBinaryOperator::LessThan => Ok(RuntimeValue::Boolean(
                    runtime_int64_from_value(&left_value)?
                        < runtime_int64_from_value(&right_value)?,
                )),
                ExecutableBinaryOperator::LessThanOrEqual => Ok(RuntimeValue::Boolean(
                    runtime_int64_from_value(&left_value)?
                        <= runtime_int64_from_value(&right_value)?,
                )),
                ExecutableBinaryOperator::GreaterThan => Ok(RuntimeValue::Boolean(
                    runtime_int64_from_value(&left_value)?
                        > runtime_int64_from_value(&right_value)?,
                )),
                ExecutableBinaryOperator::GreaterThanOrEqual => Ok(RuntimeValue::Boolean(
                    runtime_int64_from_value(&left_value)?
                        >= runtime_int64_from_value(&right_value)?,
                )),
                ExecutableBinaryOperator::And | ExecutableBinaryOperator::Or => unreachable!(),
            }
        }
    }
}

fn evaluate_call_expression(
    program_execution_context: &ProgramExecutionContext<'_>,
    callee: &ExecutableExpression,
    arguments: &[ExecutableExpression],
    local_value_by_name: &mut BTreeMap<String, RuntimeValue>,
) -> Result<RuntimeValue, RuntimeExecutionError> {
    let argument_values = arguments
        .iter()
        .map(|argument| {
            evaluate_expression(program_execution_context, argument, local_value_by_name)
        })
        .collect::<Result<Vec<_>, _>>()?;

    match callee {
        ExecutableExpression::Identifier { name } => {
            if name == PRINT_FUNCTION_CONTRACT.language_name {
                if argument_values.len() != 1 {
                    return Err(RuntimeExecutionError::Failure(run_failed(
                        "print(...) requires exactly one argument".to_string(),
                        None,
                    )));
                }
                let print_argument = runtime_string_from_value(&argument_values[0])?;
                println!("{print_argument}");
                return Ok(RuntimeValue::Nil);
            }

            if name == ABORT_FUNCTION_CONTRACT.language_name {
                if argument_values.len() != 1 {
                    return Err(RuntimeExecutionError::Failure(run_failed(
                        "abort(...) requires exactly one argument".to_string(),
                        None,
                    )));
                }
                let abort_message = runtime_string_from_value(&argument_values[0])?;
                eprintln!("{abort_message}");
                return Err(RuntimeExecutionError::Abort { exit_code: 1 });
            }

            let mut function_local_value_by_name = BTreeMap::new();
            let mut function_return_value = RuntimeValue::Nil;
            execute_function_by_name(
                program_execution_context,
                name,
                &argument_values,
                &mut function_local_value_by_name,
                &mut function_return_value,
            )?;
            Ok(function_return_value)
        }
        ExecutableExpression::FieldAccess { target, field } => {
            if let ExecutableExpression::Identifier {
                name: target_variable_name,
            } = target.as_ref()
            {
                let Some(RuntimeValue::StructInstance(target_struct_instance)) =
                    local_value_by_name.get(target_variable_name).cloned()
                else {
                    return Err(RuntimeExecutionError::Failure(run_failed(
                        format!("unknown local '{target_variable_name}'"),
                        None,
                    )));
                };

                let mut method_return_value = RuntimeValue::Nil;
                let updated_struct_instance = execute_method(
                    program_execution_context,
                    &target_struct_instance,
                    field,
                    &argument_values,
                    &mut method_return_value,
                )?;
                local_value_by_name.insert(
                    target_variable_name.clone(),
                    RuntimeValue::StructInstance(updated_struct_instance),
                );
                return Ok(method_return_value);
            }

            let target_value =
                evaluate_expression(program_execution_context, target, local_value_by_name)?;
            let RuntimeValue::StructInstance(target_struct_instance) = target_value else {
                return Err(RuntimeExecutionError::Failure(run_failed(
                    "method call requires struct receiver".to_string(),
                    None,
                )));
            };

            let mut method_return_value = RuntimeValue::Nil;
            let _ = execute_method(
                program_execution_context,
                &target_struct_instance,
                field,
                &argument_values,
                &mut method_return_value,
            )?;
            Ok(method_return_value)
        }
        _ => Err(RuntimeExecutionError::Failure(run_failed(
            "build mode currently supports calls to named functions and methods only".to_string(),
            None,
        ))),
    }
}

fn runtime_int64_from_value(value: &RuntimeValue) -> Result<i64, RuntimeExecutionError> {
    match value {
        RuntimeValue::Int64(value) => Ok(*value),
        _ => Err(RuntimeExecutionError::Failure(run_failed(
            "expected int64 value".to_string(),
            None,
        ))),
    }
}

fn runtime_boolean_from_value(value: &RuntimeValue) -> Result<bool, RuntimeExecutionError> {
    match value {
        RuntimeValue::Boolean(value) => Ok(*value),
        _ => Err(RuntimeExecutionError::Failure(run_failed(
            "expected boolean value".to_string(),
            None,
        ))),
    }
}

fn runtime_string_from_value(value: &RuntimeValue) -> Result<String, RuntimeExecutionError> {
    match value {
        RuntimeValue::String(value) => Ok(value.clone()),
        _ => Err(RuntimeExecutionError::Failure(run_failed(
            "expected string value".to_string(),
            None,
        ))),
    }
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
