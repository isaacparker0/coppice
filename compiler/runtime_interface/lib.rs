#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeType {
    Boolean,
    Nil,
    Never,
    String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeFunctionContract {
    pub language_name: &'static str,
    pub lowered_symbol_name: &'static str,
    pub parameter_types: &'static [RuntimeType],
    pub return_type: RuntimeType,
}

pub const USER_ENTRYPOINT_FUNCTION_NAME: &str = "main";

pub const PRINT_FUNCTION_CONTRACT: RuntimeFunctionContract = RuntimeFunctionContract {
    language_name: "print",
    lowered_symbol_name: "coppice_runtime_print",
    parameter_types: &[RuntimeType::String],
    return_type: RuntimeType::Nil,
};

pub const ABORT_FUNCTION_CONTRACT: RuntimeFunctionContract = RuntimeFunctionContract {
    language_name: "abort",
    lowered_symbol_name: "coppice_runtime_abort",
    parameter_types: &[RuntimeType::String],
    return_type: RuntimeType::Never,
};

pub const ASSERT_FUNCTION_CONTRACT: RuntimeFunctionContract = RuntimeFunctionContract {
    language_name: "assert",
    lowered_symbol_name: "coppice_runtime_assert",
    parameter_types: &[RuntimeType::Boolean],
    return_type: RuntimeType::Nil,
};
