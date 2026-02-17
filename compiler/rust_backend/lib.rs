use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use compiler__executable_program::{
    ExecutableBinaryOperator, ExecutableExpression, ExecutableProgram, ExecutableStatement,
};
use compiler__reports::{CompilerFailure, CompilerFailureKind};
use runfiles::Runfiles;

pub struct BuiltRustProgram {
    pub source_path: PathBuf,
    pub binary_path: PathBuf,
}

pub struct BuildArtifactIdentity {
    pub executable_stem: String,
}

pub fn build_program(
    program: &ExecutableProgram,
    build_directory: &Path,
    artifact_identity: &BuildArtifactIdentity,
) -> Result<BuiltRustProgram, CompilerFailure> {
    fs::create_dir_all(build_directory).map_err(|error| CompilerFailure {
        kind: CompilerFailureKind::BuildFailed,
        message: format!("failed to create build output directory: {error}"),
        path: Some(build_directory.display().to_string()),
        details: Vec::new(),
    })?;

    let source_path = build_directory.join(format!("{}.rs", artifact_identity.executable_stem));
    let binary_path = build_directory.join(&artifact_identity.executable_stem);
    let source = emit_rust_source(program)?;
    fs::write(&source_path, source).map_err(|error| CompilerFailure {
        kind: CompilerFailureKind::BuildFailed,
        message: format!("failed to write generated Rust source: {error}"),
        path: Some(source_path.display().to_string()),
        details: Vec::new(),
    })?;

    compile_with_hermetic_toolchain(&source_path, &binary_path)?;

    Ok(BuiltRustProgram {
        source_path,
        binary_path,
    })
}

pub fn run_program(binary_path: &Path) -> Result<i32, CompilerFailure> {
    let status = Command::new(binary_path)
        .status()
        .map_err(|error| CompilerFailure {
            kind: CompilerFailureKind::RunFailed,
            message: format!("failed to execute built program: {error}"),
            path: Some(binary_path.display().to_string()),
            details: Vec::new(),
        })?;
    Ok(status.code().unwrap_or(1))
}

fn emit_rust_source(program: &ExecutableProgram) -> Result<String, CompilerFailure> {
    let mut output = String::from("fn main() {\n");
    for statement in &program.statements {
        emit_statement(statement, &mut output, 1)?;
    }
    output.push_str("}\n");
    Ok(output)
}

fn emit_statement(
    statement: &ExecutableStatement,
    output: &mut String,
    indent_level: usize,
) -> Result<(), CompilerFailure> {
    let indent = "    ".repeat(indent_level);
    match statement {
        ExecutableStatement::Binding {
            name,
            mutable,
            initializer,
        } => {
            let initializer_source = emit_expression(initializer)?;
            output.push_str(&indent);
            if *mutable {
                output.push_str("let mut ");
            } else {
                output.push_str("let ");
            }
            output.push_str(name);
            output.push_str(" = ");
            output.push_str(&initializer_source);
            output.push_str(";\n");
        }
        ExecutableStatement::Assign { name, value } => {
            let value_source = emit_expression(value)?;
            output.push_str(&indent);
            output.push_str(name);
            output.push_str(" = ");
            output.push_str(&value_source);
            output.push_str(";\n");
        }
        ExecutableStatement::If {
            condition,
            then_statements,
            else_statements,
        } => {
            let condition_source = emit_expression(condition)?;
            output.push_str(&indent);
            output.push_str("if ");
            output.push_str(&condition_source);
            output.push_str(" {\n");
            for statement in then_statements {
                emit_statement(statement, output, indent_level + 1)?;
            }
            output.push_str(&indent);
            output.push('}');
            if let Some(else_statements) = else_statements {
                output.push_str(" else {\n");
                for statement in else_statements {
                    emit_statement(statement, output, indent_level + 1)?;
                }
                output.push_str(&indent);
                output.push('}');
            }
            output.push('\n');
        }
        ExecutableStatement::For {
            condition,
            body_statements,
        } => {
            output.push_str(&indent);
            if let Some(condition) = condition {
                let condition_source = emit_expression(condition)?;
                output.push_str("while ");
                output.push_str(&condition_source);
                output.push_str(" {\n");
            } else {
                output.push_str("loop {\n");
            }
            for statement in body_statements {
                emit_statement(statement, output, indent_level + 1)?;
            }
            output.push_str(&indent);
            output.push_str("}\n");
        }
        ExecutableStatement::Break => {
            output.push_str(&indent);
            output.push_str("break;\n");
        }
        ExecutableStatement::Continue => {
            output.push_str(&indent);
            output.push_str("continue;\n");
        }
        ExecutableStatement::Expression { expression } => {
            let expression_source = emit_expression(expression)?;
            output.push_str(&indent);
            output.push_str(&expression_source);
            output.push_str(";\n");
        }
        ExecutableStatement::Return { value } => {
            let value_source = emit_expression(value)?;
            if value_source != "()" {
                return Err(CompilerFailure {
                    kind: CompilerFailureKind::BuildFailed,
                    message: "build mode currently supports only return nil".to_string(),
                    path: None,
                    details: Vec::new(),
                });
            }
            output.push_str(&indent);
            output.push_str("return;\n");
        }
    }
    Ok(())
}

fn emit_expression(expression: &ExecutableExpression) -> Result<String, CompilerFailure> {
    match expression {
        ExecutableExpression::IntegerLiteral { value } => Ok(value.to_string()),
        ExecutableExpression::BooleanLiteral { value } => Ok(value.to_string()),
        ExecutableExpression::NilLiteral => Ok("()".to_string()),
        ExecutableExpression::StringLiteral { value } => Ok(format!("{value:?}")),
        ExecutableExpression::Identifier { name } => Ok(name.clone()),
        ExecutableExpression::Binary {
            operator,
            left,
            right,
        } => {
            let left_source = emit_expression(left)?;
            let right_source = emit_expression(right)?;
            let operator_source = match operator {
                ExecutableBinaryOperator::Add => "+",
                ExecutableBinaryOperator::EqualEqual => "==",
                ExecutableBinaryOperator::NotEqual => "!=",
                ExecutableBinaryOperator::LessThan => "<",
                ExecutableBinaryOperator::LessThanOrEqual => "<=",
                ExecutableBinaryOperator::GreaterThan => ">",
                ExecutableBinaryOperator::GreaterThanOrEqual => ">=",
            };
            Ok(format!("({left_source} {operator_source} {right_source})"))
        }
        ExecutableExpression::Call { callee, arguments } => {
            let ExecutableExpression::Identifier { name } = callee.as_ref() else {
                return Err(CompilerFailure {
                    kind: CompilerFailureKind::BuildFailed,
                    message: "build mode currently supports only print(...) calls".to_string(),
                    path: None,
                    details: Vec::new(),
                });
            };
            if name != "print" {
                return Err(CompilerFailure {
                    kind: CompilerFailureKind::BuildFailed,
                    message: format!(
                        "build mode currently supports only 'print' calls, found '{name}(...)'"
                    ),
                    path: None,
                    details: Vec::new(),
                });
            }
            if arguments.len() != 1 {
                return Err(CompilerFailure {
                    kind: CompilerFailureKind::BuildFailed,
                    message: "build mode currently supports print(...) with exactly one argument"
                        .to_string(),
                    path: None,
                    details: Vec::new(),
                });
            }
            let argument_source = emit_expression(&arguments[0])?;
            Ok(format!("println!(\"{{}}\", {argument_source})"))
        }
    }
}

fn compile_with_hermetic_toolchain(
    source_path: &Path,
    output_binary_path: &Path,
) -> Result<(), CompilerFailure> {
    let runfiles = Runfiles::create().map_err(|error| CompilerFailure {
        kind: CompilerFailureKind::BuildFailed,
        message: format!("failed to initialize runfiles: {error}"),
        path: None,
        details: Vec::new(),
    })?;
    let rustc_path =
        resolve_tool_file_from_runfiles(&runfiles, env!("HERMETIC_RUSTC"), "HERMETIC_RUSTC")?;
    let clang_path =
        resolve_tool_file_from_runfiles(&runfiles, env!("HERMETIC_CLANG"), "HERMETIC_CLANG")?;

    let output = Command::new(&rustc_path)
        .arg("--edition=2024")
        .arg(source_path)
        .arg("-C")
        .arg(format!("linker={}", clang_path.display()))
        .arg("-o")
        .arg(output_binary_path)
        .output()
        .map_err(|error| CompilerFailure {
            kind: CompilerFailureKind::BuildFailed,
            message: format!("failed to invoke hermetic rustc: {error}"),
            path: Some(source_path.display().to_string()),
            details: Vec::new(),
        })?;

    if !output.status.success() {
        return Err(CompilerFailure {
            kind: CompilerFailureKind::BuildFailed,
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            path: Some(source_path.display().to_string()),
            details: Vec::new(),
        });
    }

    Ok(())
}

fn resolve_tool_file_from_runfiles(
    runfiles: &Runfiles,
    rlocation_path: &str,
    name: &str,
) -> Result<PathBuf, CompilerFailure> {
    runfiles
        .rlocation(rlocation_path)
        .ok_or_else(|| CompilerFailure {
            kind: CompilerFailureKind::BuildFailed,
            message: format!("failed to resolve runfile for {name}: {rlocation_path}"),
            path: None,
            details: Vec::new(),
        })
}
