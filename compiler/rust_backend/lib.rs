use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use compiler__executable_program::{ExecutableExpression, ExecutableProgram, ExecutableStatement};
use compiler__reports::{CompilerFailure, CompilerFailureKind};
use runfiles::Runfiles;

pub struct BuiltRustProgram {
    pub source_path: PathBuf,
    pub binary_path: PathBuf,
}

pub fn build_program(
    program: &ExecutableProgram,
    build_directory: &Path,
) -> Result<BuiltRustProgram, CompilerFailure> {
    fs::create_dir_all(build_directory).map_err(|error| CompilerFailure {
        kind: CompilerFailureKind::BuildFailed,
        message: format!("failed to create build output directory: {error}"),
        path: Some(build_directory.display().to_string()),
        details: Vec::new(),
    })?;

    let source_path = build_directory.join("main.rs");
    let binary_path = build_directory.join("main");
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
        match statement {
            ExecutableStatement::Expression { expression } => {
                let expression_source = emit_expression(expression)?;
                output.push_str("    ");
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
                output.push_str("    return;\n");
            }
        }
    }
    output.push_str("}\n");
    Ok(output)
}

fn emit_expression(expression: &ExecutableExpression) -> Result<String, CompilerFailure> {
    match expression {
        ExecutableExpression::NilLiteral => Ok("()".to_string()),
        ExecutableExpression::StringLiteral { value } => Ok(format!("{value:?}")),
        ExecutableExpression::Identifier { name } => {
            if name == "print" {
                return Ok("println".to_string());
            }
            Err(CompilerFailure {
                kind: CompilerFailureKind::BuildFailed,
                message: format!(
                    "build mode currently supports only 'print' identifier calls, found '{name}'"
                ),
                path: None,
                details: Vec::new(),
            })
        }
        ExecutableExpression::Call { callee, arguments } => {
            let callee_source = emit_expression(callee)?;
            if callee_source != "println" {
                return Err(CompilerFailure {
                    kind: CompilerFailureKind::BuildFailed,
                    message: "build mode currently supports only print(...) calls".to_string(),
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
