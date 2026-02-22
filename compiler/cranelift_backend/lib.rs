use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use compiler__executable_program::ExecutableProgram;
use compiler__reports::{CompilerFailure, CompilerFailureKind};

mod linker_bridge;
mod object_emission;
mod runtime_interface_emission;

use linker_bridge::link_executable;
use object_emission::{emit_object_bytes, ensure_program_supported};

pub struct BuiltCraneliftProgram {
    pub binary_path: PathBuf,
}

pub struct BuildArtifactIdentity {
    pub executable_stem: String,
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
    let status = Command::new(binary_path).status().map_err(|error| {
        run_failed(
            format!("failed to execute binary: {error}"),
            Some(binary_path),
        )
    })?;
    Ok(status.code().unwrap_or(1))
}

pub(crate) fn build_failed(message: String, path: Option<&Path>) -> CompilerFailure {
    CompilerFailure {
        kind: CompilerFailureKind::BuildFailed,
        message,
        path: path.map(|path| path.display().to_string()),
        details: Vec::new(),
    }
}

pub(crate) fn run_failed(message: String, path: Option<&Path>) -> CompilerFailure {
    CompilerFailure {
        kind: CompilerFailureKind::RunFailed,
        message,
        path: path.map(|path| path.display().to_string()),
        details: Vec::new(),
    }
}
