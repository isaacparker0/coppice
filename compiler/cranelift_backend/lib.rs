use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use compiler__executable_program::{
    ExecutableBinaryOperator, ExecutableCallTarget, ExecutableCallableReference,
    ExecutableEnumVariantReference, ExecutableExpression, ExecutableFunctionDeclaration,
    ExecutableMatchArm, ExecutableMatchPattern, ExecutableProgram, ExecutableStatement,
    ExecutableStructDeclaration, ExecutableStructReference, ExecutableTypeReference,
    ExecutableUnaryOperator,
};
use compiler__reports::{CompilerFailure, CompilerFailureKind};
use compiler__runtime_interface::{ABORT_FUNCTION_CONTRACT, PRINT_FUNCTION_CONTRACT};
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
    EnumVariant(RuntimeEnumVariantValue),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RuntimeStructInstance {
    struct_reference: ExecutableStructReference,
    type_reference: ExecutableTypeReference,
    field_value_by_name: BTreeMap<String, RuntimeValue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RuntimeEnumVariantValue {
    enum_variant_reference: ExecutableEnumVariantReference,
    type_reference: ExecutableTypeReference,
}

#[derive(Clone, Debug)]
struct RuntimeWitnessTable {
    type_reference: ExecutableTypeReference,
}

type RuntimeWitnessTableByTypeParameterName = BTreeMap<String, RuntimeWitnessTable>;

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
    function_declaration_by_callable_reference:
        BTreeMap<ExecutableCallableReference, &'a ExecutableFunctionDeclaration>,
    struct_declaration_by_reference:
        BTreeMap<ExecutableStructReference, &'a ExecutableStructDeclaration>,
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
    match execute_entrypoint_function(
        &program_execution_context,
        &artifact.executable_program.entrypoint_callable_reference,
    ) {
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
            &lowered_function_symbol_name(&function_declaration.callable_reference),
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
            let mut parameter_types = vec![ExecutableTypeReference::NominalType {
                name: struct_declaration.name.clone(),
            }];
            for parameter in &method_declaration.parameters {
                parameter_types.push(parameter.type_reference.clone());
            }
            define_stub_function_for_validation(
                &mut jit_module,
                &lowered_method_symbol_name(
                    &struct_declaration.struct_reference,
                    &method_declaration.name,
                ),
                &parameter_types,
                &method_declaration.return_type,
            )?;
        }
    }

    Ok(())
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
        ExecutableTypeReference::Union { .. }
        | ExecutableTypeReference::Int64
        | ExecutableTypeReference::String
        | ExecutableTypeReference::TypeParameter { .. }
        | ExecutableTypeReference::NominalTypeApplication { .. }
        | ExecutableTypeReference::NominalType { .. } => types::I64,
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
    let mut function_declaration_by_callable_reference = BTreeMap::new();
    for function_declaration in &program.function_declarations {
        function_declaration_by_callable_reference.insert(
            function_declaration.callable_reference.clone(),
            function_declaration,
        );
    }

    let mut struct_declaration_by_reference = BTreeMap::new();
    for struct_declaration in &program.struct_declarations {
        struct_declaration_by_reference.insert(
            struct_declaration.struct_reference.clone(),
            struct_declaration,
        );
    }

    ProgramExecutionContext {
        function_declaration_by_callable_reference,
        struct_declaration_by_reference,
    }
}

fn execute_entrypoint_function(
    program_execution_context: &ProgramExecutionContext<'_>,
    entrypoint_callable_reference: &ExecutableCallableReference,
) -> Result<(), RuntimeExecutionError> {
    let mut local_value_by_name = BTreeMap::new();
    let mut return_value = RuntimeValue::Nil;
    execute_function_by_callable_reference(
        program_execution_context,
        entrypoint_callable_reference,
        &[],
        &[],
        &mut local_value_by_name,
        &mut return_value,
    )?;
    Ok(())
}

fn execute_function_by_callable_reference(
    program_execution_context: &ProgramExecutionContext<'_>,
    callable_reference: &ExecutableCallableReference,
    type_argument_references: &[ExecutableTypeReference],
    argument_values: &[RuntimeValue],
    local_value_by_name: &mut BTreeMap<String, RuntimeValue>,
    return_value: &mut RuntimeValue,
) -> Result<(), RuntimeExecutionError> {
    let Some(function_declaration) = program_execution_context
        .function_declaration_by_callable_reference
        .get(callable_reference)
    else {
        return Err(RuntimeExecutionError::Failure(run_failed(
            format!(
                "unknown function '{}::{}'",
                callable_reference.package_path, callable_reference.symbol_name
            ),
            None,
        )));
    };

    if function_declaration.parameters.len() != argument_values.len() {
        return Err(RuntimeExecutionError::Failure(run_failed(
            format!(
                "function '{}::{}' expected {} argument(s) but got {}",
                callable_reference.package_path,
                callable_reference.symbol_name,
                function_declaration.parameters.len(),
                argument_values.len()
            ),
            None,
        )));
    }

    if function_declaration.type_parameter_names.len() != type_argument_references.len() {
        return Err(RuntimeExecutionError::Failure(run_failed(
            format!(
                "function '{}::{}' expected {} type argument(s) but got {}",
                callable_reference.package_path,
                callable_reference.symbol_name,
                function_declaration.type_parameter_names.len(),
                type_argument_references.len()
            ),
            None,
        )));
    }

    local_value_by_name.clear();
    for (parameter, argument_value) in function_declaration.parameters.iter().zip(argument_values) {
        local_value_by_name.insert(parameter.name.clone(), argument_value.clone());
    }
    let witness_table_by_type_parameter_name = function_declaration
        .type_parameter_names
        .iter()
        .cloned()
        .zip(
            type_argument_references
                .iter()
                .cloned()
                .map(|type_reference| RuntimeWitnessTable { type_reference }),
        )
        .collect::<RuntimeWitnessTableByTypeParameterName>();

    let statement_signal = execute_statements(
        program_execution_context,
        &function_declaration.statements,
        &witness_table_by_type_parameter_name,
        local_value_by_name,
        return_value,
    )?;
    if matches!(
        statement_signal,
        StatementExecutionSignal::Break | StatementExecutionSignal::Continue
    ) {
        return Err(RuntimeExecutionError::Failure(run_failed(
            format!(
                "function '{}::{}' contains invalid loop control flow",
                callable_reference.package_path, callable_reference.symbol_name
            ),
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
        .struct_declaration_by_reference
        .get(&struct_instance.struct_reference)
    else {
        return Err(RuntimeExecutionError::Failure(run_failed(
            format!(
                "unknown struct '{}::{}'",
                struct_instance.struct_reference.package_path,
                struct_instance.struct_reference.symbol_name
            ),
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
                "unknown method '{}::{}.{}'",
                struct_instance.struct_reference.package_path,
                struct_instance.struct_reference.symbol_name,
                method_name
            ),
            None,
        )));
    };

    if method_declaration.parameters.len() != argument_values.len() {
        return Err(RuntimeExecutionError::Failure(run_failed(
            format!(
                "method '{}::{}.{}' expected {} argument(s) but got {}",
                struct_instance.struct_reference.package_path,
                struct_instance.struct_reference.symbol_name,
                method_name,
                method_declaration.parameters.len(),
                argument_values.len()
            ),
            None,
        )));
    }

    let method_type_argument_references = match &struct_instance.type_reference {
        ExecutableTypeReference::NominalTypeApplication { arguments, .. } => arguments.clone(),
        ExecutableTypeReference::NominalType { .. } => Vec::new(),
        _ => {
            return Err(RuntimeExecutionError::Failure(run_failed(
                format!(
                    "method receiver for '{}::{}' has invalid runtime type reference",
                    struct_instance.struct_reference.package_path,
                    struct_instance.struct_reference.symbol_name
                ),
                None,
            )));
        }
    };
    if struct_declaration.type_parameter_names.len() != method_type_argument_references.len() {
        return Err(RuntimeExecutionError::Failure(run_failed(
            format!(
                "method '{}::{}.{}' expected {} receiver type argument(s) but got {}",
                struct_instance.struct_reference.package_path,
                struct_instance.struct_reference.symbol_name,
                method_name,
                struct_declaration.type_parameter_names.len(),
                method_type_argument_references.len()
            ),
            None,
        )));
    }
    let witness_table_by_type_parameter_name = struct_declaration
        .type_parameter_names
        .iter()
        .cloned()
        .zip(
            method_type_argument_references
                .into_iter()
                .map(|type_reference| RuntimeWitnessTable { type_reference }),
        )
        .collect::<RuntimeWitnessTableByTypeParameterName>();

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
        &witness_table_by_type_parameter_name,
        &mut method_local_value_by_name,
        return_value,
    )?;
    if matches!(
        statement_signal,
        StatementExecutionSignal::Break | StatementExecutionSignal::Continue
    ) {
        return Err(RuntimeExecutionError::Failure(run_failed(
            format!(
                "method '{}::{}.{}' contains invalid loop control flow",
                struct_instance.struct_reference.package_path,
                struct_instance.struct_reference.symbol_name,
                method_name
            ),
            None,
        )));
    }

    let Some(RuntimeValue::StructInstance(updated_struct_instance)) =
        method_local_value_by_name.remove("self")
    else {
        return Err(RuntimeExecutionError::Failure(run_failed(
            format!(
                "method '{}::{}.{}' did not preserve receiver value",
                struct_instance.struct_reference.package_path,
                struct_instance.struct_reference.symbol_name,
                method_name
            ),
            None,
        )));
    };

    Ok(updated_struct_instance)
}

fn execute_statements(
    program_execution_context: &ProgramExecutionContext<'_>,
    statements: &[ExecutableStatement],
    witness_table_by_type_parameter_name: &RuntimeWitnessTableByTypeParameterName,
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
                    witness_table_by_type_parameter_name,
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
                let value_to_assign = evaluate_expression(
                    program_execution_context,
                    value,
                    witness_table_by_type_parameter_name,
                    local_value_by_name,
                )?;
                local_value_by_name.insert(name.clone(), value_to_assign);
            }
            ExecutableStatement::If {
                condition,
                then_statements,
                else_statements,
            } => {
                let condition_value = evaluate_expression(
                    program_execution_context,
                    condition,
                    witness_table_by_type_parameter_name,
                    local_value_by_name,
                )?;
                let condition_boolean = runtime_boolean_from_value(&condition_value)?;
                let statement_signal = if condition_boolean {
                    execute_statements(
                        program_execution_context,
                        then_statements,
                        witness_table_by_type_parameter_name,
                        local_value_by_name,
                        return_value,
                    )?
                } else if let Some(else_statements) = else_statements {
                    execute_statements(
                        program_execution_context,
                        else_statements,
                        witness_table_by_type_parameter_name,
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
                        witness_table_by_type_parameter_name,
                        local_value_by_name,
                    )?;
                    if !runtime_boolean_from_value(&condition_value)? {
                        break;
                    }
                }

                match execute_statements(
                    program_execution_context,
                    body_statements,
                    witness_table_by_type_parameter_name,
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
                    witness_table_by_type_parameter_name,
                    local_value_by_name,
                )?;
            }
            ExecutableStatement::Return { value } => {
                *return_value = evaluate_expression(
                    program_execution_context,
                    value,
                    witness_table_by_type_parameter_name,
                    local_value_by_name,
                )?;
                return Ok(StatementExecutionSignal::Return);
            }
        }
    }

    Ok(StatementExecutionSignal::Next)
}

fn evaluate_expression(
    program_execution_context: &ProgramExecutionContext<'_>,
    expression: &ExecutableExpression,
    witness_table_by_type_parameter_name: &RuntimeWitnessTableByTypeParameterName,
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
        ExecutableExpression::EnumVariantLiteral {
            enum_variant_reference,
            type_reference,
        } => Ok(RuntimeValue::EnumVariant(RuntimeEnumVariantValue {
            enum_variant_reference: enum_variant_reference.clone(),
            type_reference: resolve_type_reference_for_call(
                type_reference,
                witness_table_by_type_parameter_name,
            ),
        })),
        ExecutableExpression::StructLiteral {
            struct_reference,
            type_reference,
            fields,
        } => {
            let mut field_value_by_name = BTreeMap::new();
            for field in fields {
                let field_value = evaluate_expression(
                    program_execution_context,
                    &field.value,
                    witness_table_by_type_parameter_name,
                    local_value_by_name,
                )?;
                field_value_by_name.insert(field.name.clone(), field_value);
            }
            Ok(RuntimeValue::StructInstance(RuntimeStructInstance {
                struct_reference: struct_reference.clone(),
                type_reference: resolve_type_reference_for_call(
                    type_reference,
                    witness_table_by_type_parameter_name,
                ),
                field_value_by_name,
            }))
        }
        ExecutableExpression::FieldAccess { target, field } => {
            let target_value = evaluate_expression(
                program_execution_context,
                target,
                witness_table_by_type_parameter_name,
                local_value_by_name,
            )?;
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
                        format!(
                            "unknown field '{}::{}.{}'",
                            struct_instance.struct_reference.package_path,
                            struct_instance.struct_reference.symbol_name,
                            field
                        ),
                        None,
                    ))
                })
        }
        ExecutableExpression::Unary {
            operator,
            expression,
        } => {
            let expression_value = evaluate_expression(
                program_execution_context,
                expression,
                witness_table_by_type_parameter_name,
                local_value_by_name,
            )?;
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
            witness_table_by_type_parameter_name,
            local_value_by_name,
        ),
        ExecutableExpression::Call {
            callee,
            call_target,
            arguments,
            type_arguments,
        } => evaluate_call_expression(
            program_execution_context,
            callee,
            call_target.as_ref(),
            arguments,
            type_arguments,
            witness_table_by_type_parameter_name,
            local_value_by_name,
        ),
        ExecutableExpression::Match { target, arms } => {
            let target_value = evaluate_expression(
                program_execution_context,
                target,
                witness_table_by_type_parameter_name,
                local_value_by_name,
            )?;
            evaluate_match_expression(
                program_execution_context,
                &target_value,
                arms,
                witness_table_by_type_parameter_name,
                local_value_by_name,
            )
        }
        ExecutableExpression::Matches {
            value,
            type_reference,
        } => {
            let value = evaluate_expression(
                program_execution_context,
                value,
                witness_table_by_type_parameter_name,
                local_value_by_name,
            )?;
            Ok(RuntimeValue::Boolean(runtime_value_matches_type_reference(
                &value,
                type_reference,
                witness_table_by_type_parameter_name,
            )))
        }
    }
}

fn evaluate_match_expression(
    program_execution_context: &ProgramExecutionContext<'_>,
    target_value: &RuntimeValue,
    arms: &[ExecutableMatchArm],
    witness_table_by_type_parameter_name: &RuntimeWitnessTableByTypeParameterName,
    local_value_by_name: &mut BTreeMap<String, RuntimeValue>,
) -> Result<RuntimeValue, RuntimeExecutionError> {
    for arm in arms {
        if !runtime_value_matches_match_pattern(
            target_value,
            &arm.pattern,
            witness_table_by_type_parameter_name,
        ) {
            continue;
        }

        let binding_name_to_restore = match &arm.pattern {
            ExecutableMatchPattern::Binding { binding_name, .. } => Some(binding_name),
            ExecutableMatchPattern::Type { .. } => None,
        };
        let previous_value = binding_name_to_restore
            .and_then(|binding_name| local_value_by_name.get(binding_name).cloned());
        if let Some(binding_name) = binding_name_to_restore {
            local_value_by_name.insert(binding_name.clone(), target_value.clone());
        }

        let arm_value_result = evaluate_expression(
            program_execution_context,
            &arm.value,
            witness_table_by_type_parameter_name,
            local_value_by_name,
        );

        if let Some(binding_name) = binding_name_to_restore {
            if let Some(previous_value) = previous_value {
                local_value_by_name.insert(binding_name.clone(), previous_value);
            } else {
                local_value_by_name.remove(binding_name);
            }
        }

        return arm_value_result;
    }

    Err(RuntimeExecutionError::Failure(run_failed(
        "match expression had no matching arm at runtime".to_string(),
        None,
    )))
}

fn evaluate_binary_expression(
    program_execution_context: &ProgramExecutionContext<'_>,
    operator: ExecutableBinaryOperator,
    left: &ExecutableExpression,
    right: &ExecutableExpression,
    witness_table_by_type_parameter_name: &RuntimeWitnessTableByTypeParameterName,
    local_value_by_name: &mut BTreeMap<String, RuntimeValue>,
) -> Result<RuntimeValue, RuntimeExecutionError> {
    match operator {
        ExecutableBinaryOperator::And => {
            let left_value = evaluate_expression(
                program_execution_context,
                left,
                witness_table_by_type_parameter_name,
                local_value_by_name,
            )?;
            let left_boolean = runtime_boolean_from_value(&left_value)?;
            if !left_boolean {
                return Ok(RuntimeValue::Boolean(false));
            }
            let right_value = evaluate_expression(
                program_execution_context,
                right,
                witness_table_by_type_parameter_name,
                local_value_by_name,
            )?;
            let right_boolean = runtime_boolean_from_value(&right_value)?;
            Ok(RuntimeValue::Boolean(right_boolean))
        }
        ExecutableBinaryOperator::Or => {
            let left_value = evaluate_expression(
                program_execution_context,
                left,
                witness_table_by_type_parameter_name,
                local_value_by_name,
            )?;
            let left_boolean = runtime_boolean_from_value(&left_value)?;
            if left_boolean {
                return Ok(RuntimeValue::Boolean(true));
            }
            let right_value = evaluate_expression(
                program_execution_context,
                right,
                witness_table_by_type_parameter_name,
                local_value_by_name,
            )?;
            let right_boolean = runtime_boolean_from_value(&right_value)?;
            Ok(RuntimeValue::Boolean(right_boolean))
        }
        _ => {
            let left_value = evaluate_expression(
                program_execution_context,
                left,
                witness_table_by_type_parameter_name,
                local_value_by_name,
            )?;
            let right_value = evaluate_expression(
                program_execution_context,
                right,
                witness_table_by_type_parameter_name,
                local_value_by_name,
            )?;
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
                ExecutableBinaryOperator::Modulo => Ok(RuntimeValue::Int64(
                    runtime_int64_from_value(&left_value)?
                        % runtime_int64_from_value(&right_value)?,
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
    call_target: Option<&ExecutableCallTarget>,
    arguments: &[ExecutableExpression],
    type_arguments: &[ExecutableTypeReference],
    witness_table_by_type_parameter_name: &RuntimeWitnessTableByTypeParameterName,
    local_value_by_name: &mut BTreeMap<String, RuntimeValue>,
) -> Result<RuntimeValue, RuntimeExecutionError> {
    let argument_values = arguments
        .iter()
        .map(|argument| {
            evaluate_expression(
                program_execution_context,
                argument,
                witness_table_by_type_parameter_name,
                local_value_by_name,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    if let Some(call_target) = call_target {
        match call_target {
            ExecutableCallTarget::BuiltinFunction { function_name } => {
                if !type_arguments.is_empty() {
                    return Err(RuntimeExecutionError::Failure(run_failed(
                        format!("builtin function '{function_name}' does not take type arguments"),
                        None,
                    )));
                }
                if function_name == PRINT_FUNCTION_CONTRACT.language_name {
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

                if function_name == ABORT_FUNCTION_CONTRACT.language_name {
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

                return Err(RuntimeExecutionError::Failure(run_failed(
                    format!("unknown builtin function '{function_name}'"),
                    None,
                )));
            }
            ExecutableCallTarget::UserDefinedFunction { callable_reference } => {
                let mut function_local_value_by_name = BTreeMap::new();
                let mut function_return_value = RuntimeValue::Nil;
                let resolved_type_arguments = type_arguments
                    .iter()
                    .map(|type_argument| {
                        resolve_type_reference_for_call(
                            type_argument,
                            witness_table_by_type_parameter_name,
                        )
                    })
                    .collect::<Vec<_>>();
                execute_function_by_callable_reference(
                    program_execution_context,
                    callable_reference,
                    &resolved_type_arguments,
                    &argument_values,
                    &mut function_local_value_by_name,
                    &mut function_return_value,
                )?;
                return Ok(function_return_value);
            }
        }
    }

    match callee {
        ExecutableExpression::Identifier { .. } => Err(RuntimeExecutionError::Failure(run_failed(
            "build mode requires resolved call target metadata for function calls".to_string(),
            None,
        ))),
        ExecutableExpression::FieldAccess { target, field } => {
            if !type_arguments.is_empty() {
                return Err(RuntimeExecutionError::Failure(run_failed(
                    "method calls do not take type arguments".to_string(),
                    None,
                )));
            }
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
                Ok(method_return_value)
            } else {
                let target_value = evaluate_expression(
                    program_execution_context,
                    target,
                    witness_table_by_type_parameter_name,
                    local_value_by_name,
                )?;
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

fn runtime_value_matches_match_pattern(
    value: &RuntimeValue,
    pattern: &ExecutableMatchPattern,
    witness_table_by_type_parameter_name: &RuntimeWitnessTableByTypeParameterName,
) -> bool {
    match pattern {
        ExecutableMatchPattern::Type { type_reference }
        | ExecutableMatchPattern::Binding { type_reference, .. } => {
            runtime_value_matches_type_reference(
                value,
                type_reference,
                witness_table_by_type_parameter_name,
            )
        }
    }
}

fn runtime_value_matches_type_reference(
    value: &RuntimeValue,
    type_reference: &ExecutableTypeReference,
    witness_table_by_type_parameter_name: &RuntimeWitnessTableByTypeParameterName,
) -> bool {
    match type_reference {
        ExecutableTypeReference::Int64 => matches!(value, RuntimeValue::Int64(_)),
        ExecutableTypeReference::Boolean => matches!(value, RuntimeValue::Boolean(_)),
        ExecutableTypeReference::String => matches!(value, RuntimeValue::String(_)),
        ExecutableTypeReference::Nil => matches!(value, RuntimeValue::Nil),
        ExecutableTypeReference::Never => false,
        ExecutableTypeReference::Union { members } => members.iter().any(|member| {
            runtime_value_matches_type_reference(
                value,
                member,
                witness_table_by_type_parameter_name,
            )
        }),
        ExecutableTypeReference::TypeParameter { name } => witness_table_by_type_parameter_name
            .get(name)
            .is_some_and(|witness_table| {
                runtime_value_matches_type_reference(
                    value,
                    &witness_table.type_reference,
                    witness_table_by_type_parameter_name,
                )
            }),
        ExecutableTypeReference::NominalTypeApplication {
            base_name,
            arguments,
        } => {
            if let RuntimeValue::StructInstance(struct_instance) = value {
                if !named_type_reference_matches_struct_instance(base_name, struct_instance) {
                    return false;
                }
                return runtime_type_reference_arguments_match_instantiation(
                    arguments,
                    &struct_instance.type_reference,
                );
            }
            if let RuntimeValue::EnumVariant(enum_variant_value) = value {
                if !named_type_reference_matches_enum_variant(base_name, enum_variant_value) {
                    return false;
                }
                return runtime_type_reference_arguments_match_instantiation(
                    arguments,
                    &enum_variant_value.type_reference,
                );
            }
            false
        }
        ExecutableTypeReference::NominalType { name } => {
            if let RuntimeValue::StructInstance(struct_instance) = value {
                return named_type_reference_matches_struct_instance(name, struct_instance);
            }
            if let RuntimeValue::EnumVariant(enum_variant_value) = value {
                return named_type_reference_matches_enum_variant(name, enum_variant_value);
            }
            false
        }
    }
}

fn runtime_type_reference_arguments_match_instantiation(
    pattern_arguments: &[ExecutableTypeReference],
    value_type_reference: &ExecutableTypeReference,
) -> bool {
    let ExecutableTypeReference::NominalTypeApplication {
        arguments: value_arguments,
        ..
    } = value_type_reference
    else {
        return pattern_arguments.is_empty();
    };
    if pattern_arguments.len() != value_arguments.len() {
        return false;
    }
    pattern_arguments
        .iter()
        .zip(value_arguments)
        .all(|(pattern_argument, value_argument)| {
            runtime_type_reference_structurally_equals(pattern_argument, value_argument)
        })
}

fn runtime_type_reference_structurally_equals(
    left: &ExecutableTypeReference,
    right: &ExecutableTypeReference,
) -> bool {
    match (left, right) {
        (ExecutableTypeReference::Int64, ExecutableTypeReference::Int64)
        | (ExecutableTypeReference::Boolean, ExecutableTypeReference::Boolean)
        | (ExecutableTypeReference::String, ExecutableTypeReference::String)
        | (ExecutableTypeReference::Nil, ExecutableTypeReference::Nil)
        | (ExecutableTypeReference::Never, ExecutableTypeReference::Never) => true,
        (
            ExecutableTypeReference::TypeParameter { name: left_name },
            ExecutableTypeReference::TypeParameter { name: right_name },
        )
        | (
            ExecutableTypeReference::NominalType { name: left_name },
            ExecutableTypeReference::NominalType { name: right_name },
        ) => left_name == right_name,
        (
            ExecutableTypeReference::NominalTypeApplication {
                base_name: left_base_name,
                arguments: left_arguments,
            },
            ExecutableTypeReference::NominalTypeApplication {
                base_name: right_base_name,
                arguments: right_arguments,
            },
        ) => {
            left_base_name == right_base_name
                && left_arguments.len() == right_arguments.len()
                && left_arguments.iter().zip(right_arguments).all(
                    |(left_argument, right_argument)| {
                        runtime_type_reference_structurally_equals(left_argument, right_argument)
                    },
                )
        }
        (
            ExecutableTypeReference::Union {
                members: left_members,
            },
            ExecutableTypeReference::Union {
                members: right_members,
            },
        ) => {
            left_members.len() == right_members.len()
                && left_members
                    .iter()
                    .zip(right_members)
                    .all(|(left_member, right_member)| {
                        runtime_type_reference_structurally_equals(left_member, right_member)
                    })
        }
        _ => false,
    }
}

fn resolve_type_reference_for_call(
    type_reference: &ExecutableTypeReference,
    witness_table_by_type_parameter_name: &RuntimeWitnessTableByTypeParameterName,
) -> ExecutableTypeReference {
    match type_reference {
        ExecutableTypeReference::Union { members } => ExecutableTypeReference::Union {
            members: members
                .iter()
                .map(|member| {
                    resolve_type_reference_for_call(member, witness_table_by_type_parameter_name)
                })
                .collect(),
        },
        ExecutableTypeReference::TypeParameter { name } => {
            witness_table_by_type_parameter_name.get(name).map_or_else(
                || type_reference.clone(),
                |witness_table| witness_table.type_reference.clone(),
            )
        }
        ExecutableTypeReference::NominalTypeApplication {
            base_name,
            arguments,
        } => ExecutableTypeReference::NominalTypeApplication {
            base_name: base_name.clone(),
            arguments: arguments
                .iter()
                .map(|argument| {
                    resolve_type_reference_for_call(argument, witness_table_by_type_parameter_name)
                })
                .collect(),
        },
        _ => type_reference.clone(),
    }
}

fn named_type_reference_matches_struct_instance(
    type_reference_name: &str,
    struct_instance: &RuntimeStructInstance,
) -> bool {
    if struct_instance.struct_reference.symbol_name == type_reference_name {
        return true;
    }

    let Some(last_type_reference_segment) = type_reference_name.rsplit('.').next() else {
        return false;
    };
    if struct_instance.struct_reference.symbol_name != last_type_reference_segment {
        return false;
    }

    let Some(type_reference_prefix) = type_reference_name
        .strip_suffix(last_type_reference_segment)
        .and_then(|prefix| prefix.strip_suffix('.'))
    else {
        return false;
    };
    if type_reference_prefix.is_empty() {
        return true;
    }

    let normalized_struct_package_path = struct_instance
        .struct_reference
        .package_path
        .replace('/', ".");
    normalized_struct_package_path == type_reference_prefix
}

fn named_type_reference_matches_enum_variant(
    type_reference_name: &str,
    enum_variant_value: &RuntimeEnumVariantValue,
) -> bool {
    let variant_full_name = format!(
        "{}.{}",
        enum_variant_value.enum_variant_reference.enum_name,
        enum_variant_value.enum_variant_reference.variant_name
    );
    type_reference_name == variant_full_name
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
