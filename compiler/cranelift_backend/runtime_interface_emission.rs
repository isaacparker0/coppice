use compiler__reports::CompilerFailure;
use cranelift_codegen::ir::{AbiParam, InstBuilder, TrapCode, Value, types};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::{FuncId, Module};
use cranelift_object::ObjectModule;

use crate::build_failed;

#[derive(Clone, Copy)]
pub(crate) struct ExternalRuntimeFunctions {
    pub write: FuncId,
    pub strlen: FuncId,
    pub exit: FuncId,
    pub malloc: FuncId,
}

pub(crate) fn declare_runtime_interface_functions(
    module: &mut ObjectModule,
) -> Result<ExternalRuntimeFunctions, CompilerFailure> {
    let mut write_signature = module.make_signature();
    write_signature.params.push(AbiParam::new(types::I32));
    write_signature.params.push(AbiParam::new(types::I64));
    write_signature.params.push(AbiParam::new(types::I64));
    write_signature.returns.push(AbiParam::new(types::I64));
    let write = module
        .declare_function("write", cranelift_module::Linkage::Import, &write_signature)
        .map_err(|error| build_failed(format!("failed to declare 'write': {error}"), None))?;

    let mut strlen_signature = module.make_signature();
    strlen_signature.params.push(AbiParam::new(types::I64));
    strlen_signature.returns.push(AbiParam::new(types::I64));
    let strlen = module
        .declare_function(
            "strlen",
            cranelift_module::Linkage::Import,
            &strlen_signature,
        )
        .map_err(|error| build_failed(format!("failed to declare 'strlen': {error}"), None))?;

    let mut exit_signature = module.make_signature();
    exit_signature.params.push(AbiParam::new(types::I32));
    let exit = module
        .declare_function("exit", cranelift_module::Linkage::Import, &exit_signature)
        .map_err(|error| build_failed(format!("failed to declare 'exit': {error}"), None))?;

    let mut malloc_signature = module.make_signature();
    malloc_signature.params.push(AbiParam::new(types::I64));
    malloc_signature.returns.push(AbiParam::new(types::I64));
    let malloc = module
        .declare_function(
            "malloc",
            cranelift_module::Linkage::Import,
            &malloc_signature,
        )
        .map_err(|error| build_failed(format!("failed to declare 'malloc': {error}"), None))?;

    Ok(ExternalRuntimeFunctions {
        write,
        strlen,
        exit,
        malloc,
    })
}

pub(crate) fn allocate_heap_bytes(
    module: &mut ObjectModule,
    external_runtime_functions: &ExternalRuntimeFunctions,
    function_builder: &mut FunctionBuilder<'_>,
    byte_count: i64,
) -> Result<Value, CompilerFailure> {
    if byte_count < 0 {
        return Err(build_failed(
            "attempted to allocate negative byte count".to_string(),
            None,
        ));
    }
    let byte_count_value = function_builder.ins().iconst(types::I64, byte_count);
    let malloc =
        module.declare_func_in_func(external_runtime_functions.malloc, function_builder.func);
    let malloc_call = function_builder.ins().call(malloc, &[byte_count_value]);
    Ok(function_builder.inst_results(malloc_call)[0])
}

pub(crate) fn emit_write_string_with_newline(
    module: &mut ObjectModule,
    external_runtime_functions: &ExternalRuntimeFunctions,
    function_builder: &mut FunctionBuilder<'_>,
    file_descriptor: i32,
    string_pointer: Value,
) -> Result<(), CompilerFailure> {
    let strlen =
        module.declare_func_in_func(external_runtime_functions.strlen, function_builder.func);
    let strlen_call = function_builder.ins().call(strlen, &[string_pointer]);
    let length = function_builder.inst_results(strlen_call)[0];

    let write =
        module.declare_func_in_func(external_runtime_functions.write, function_builder.func);
    let file_descriptor = function_builder
        .ins()
        .iconst(types::I32, i64::from(file_descriptor));
    let _ = function_builder
        .ins()
        .call(write, &[file_descriptor, string_pointer, length]);

    let newline_pointer =
        intern_string_literal(module, external_runtime_functions, function_builder, "\n")?;
    let one = function_builder.ins().iconst(types::I64, 1);
    let _ = function_builder
        .ins()
        .call(write, &[file_descriptor, newline_pointer, one]);

    Ok(())
}

pub(crate) fn emit_exit_call(
    module: &mut ObjectModule,
    external_runtime_functions: &ExternalRuntimeFunctions,
    function_builder: &mut FunctionBuilder<'_>,
    exit_code: i32,
) {
    let exit = module.declare_func_in_func(external_runtime_functions.exit, function_builder.func);
    let exit_code = function_builder
        .ins()
        .iconst(types::I32, i64::from(exit_code));
    let _ = function_builder.ins().call(exit, &[exit_code]);
    function_builder.ins().trap(TrapCode::user(1).unwrap());
}

pub(crate) fn intern_string_literal(
    module: &mut ObjectModule,
    external_runtime_functions: &ExternalRuntimeFunctions,
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

    let malloc =
        module.declare_func_in_func(external_runtime_functions.malloc, function_builder.func);
    let malloc_call = function_builder.ins().call(malloc, &[total_size_value]);
    let pointer = function_builder.inst_results(malloc_call)[0];

    let mem_flags = cranelift_codegen::ir::MemFlags::new();
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
