use compiler__reports::CompilerFailure;
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{InstBuilder, MemFlags, Value, types};
use cranelift_frontend::FunctionBuilder;

use crate::object_emission::{CompilationState, allocate_heap_bytes};

pub(crate) fn convert_int64_to_string(
    state: &mut CompilationState,
    function_builder: &mut FunctionBuilder<'_>,
    value: Value,
) -> Result<Value, CompilerFailure> {
    // int64 minimum value ("-9223372036854775808") is 20 chars + NUL.
    let buffer_byte_count = 21;
    let buffer_pointer = allocate_heap_bytes(state, function_builder, buffer_byte_count)?;
    let mem_flags = MemFlags::new();

    let terminator = function_builder.ins().iconst(types::I8, 0);
    function_builder
        .ins()
        .store(mem_flags, terminator, buffer_pointer, 20);

    let value_variable = function_builder.declare_var(types::I64);
    function_builder.def_var(value_variable, value);

    let index_variable = function_builder.declare_var(types::I64);
    let initial_index = function_builder.ins().iconst(types::I64, 20);
    function_builder.def_var(index_variable, initial_index);

    let write_zero_block = function_builder.create_block();
    let loop_header_block = function_builder.create_block();
    let loop_body_block = function_builder.create_block();
    let after_digits_block = function_builder.create_block();
    let write_sign_block = function_builder.create_block();
    let merge_block = function_builder.create_block();

    let zero_i64 = function_builder.ins().iconst(types::I64, 0);
    let value_is_zero = function_builder.ins().icmp(IntCC::Equal, value, zero_i64);
    function_builder
        .ins()
        .brif(value_is_zero, write_zero_block, &[], loop_header_block, &[]);
    function_builder.seal_block(write_zero_block);

    function_builder.switch_to_block(write_zero_block);
    let write_zero_index = function_builder.use_var(index_variable);
    let next_write_zero_index = function_builder.ins().iadd_imm(write_zero_index, -1);
    function_builder.def_var(index_variable, next_write_zero_index);
    let zero_digit = function_builder.ins().iconst(types::I8, i64::from(b'0'));
    let write_zero_pointer = function_builder
        .ins()
        .iadd(buffer_pointer, next_write_zero_index);
    function_builder
        .ins()
        .store(mem_flags, zero_digit, write_zero_pointer, 0);
    function_builder.ins().jump(after_digits_block, &[]);

    function_builder.switch_to_block(loop_header_block);
    let loop_value = function_builder.use_var(value_variable);
    let loop_value_is_zero = function_builder
        .ins()
        .icmp(IntCC::Equal, loop_value, zero_i64);
    function_builder.ins().brif(
        loop_value_is_zero,
        after_digits_block,
        &[],
        loop_body_block,
        &[],
    );
    function_builder.seal_block(loop_body_block);

    function_builder.switch_to_block(loop_body_block);
    let current_value = function_builder.use_var(value_variable);
    let quotient = function_builder.ins().sdiv_imm(current_value, 10);
    let remainder = function_builder.ins().srem_imm(current_value, 10);
    let remainder_is_negative =
        function_builder
            .ins()
            .icmp(IntCC::SignedLessThan, remainder, zero_i64);
    let negated_remainder = function_builder.ins().ineg(remainder);
    let absolute_remainder =
        function_builder
            .ins()
            .select(remainder_is_negative, negated_remainder, remainder);
    let ascii_zero_i64 = function_builder.ins().iconst(types::I64, i64::from(b'0'));
    let digit_i64 = function_builder
        .ins()
        .iadd(absolute_remainder, ascii_zero_i64);
    let digit_i8 = function_builder.ins().ireduce(types::I8, digit_i64);

    let write_index = function_builder.use_var(index_variable);
    let next_write_index = function_builder.ins().iadd_imm(write_index, -1);
    function_builder.def_var(index_variable, next_write_index);
    let write_pointer = function_builder
        .ins()
        .iadd(buffer_pointer, next_write_index);
    function_builder
        .ins()
        .store(mem_flags, digit_i8, write_pointer, 0);
    function_builder.def_var(value_variable, quotient);
    function_builder.ins().jump(loop_header_block, &[]);
    function_builder.seal_block(loop_header_block);
    function_builder.seal_block(after_digits_block);

    function_builder.switch_to_block(after_digits_block);
    let value_is_negative = function_builder
        .ins()
        .icmp(IntCC::SignedLessThan, value, zero_i64);
    function_builder
        .ins()
        .brif(value_is_negative, write_sign_block, &[], merge_block, &[]);
    function_builder.seal_block(write_sign_block);

    function_builder.switch_to_block(write_sign_block);
    let sign_index = function_builder.use_var(index_variable);
    let next_sign_index = function_builder.ins().iadd_imm(sign_index, -1);
    function_builder.def_var(index_variable, next_sign_index);
    let minus_digit = function_builder.ins().iconst(types::I8, i64::from(b'-'));
    let sign_pointer = function_builder.ins().iadd(buffer_pointer, next_sign_index);
    function_builder
        .ins()
        .store(mem_flags, minus_digit, sign_pointer, 0);
    function_builder.ins().jump(merge_block, &[]);
    function_builder.seal_block(merge_block);

    function_builder.switch_to_block(merge_block);
    let start_index = function_builder.use_var(index_variable);
    let string_pointer = function_builder.ins().iadd(buffer_pointer, start_index);
    Ok(string_pointer)
}
