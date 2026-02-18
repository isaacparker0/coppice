use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use compiler__driver::{build_target_with_workspace_root, check_target_with_workspace_root};
use compiler__reports::{CompilerFailure, CompilerFailureKind};
use tokio::process::Command;

use crate::session_store::ensure_workspace_manifest;

pub struct RunExecution {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

pub fn write_workspace_source(
    session_directory: &Path,
    source: &str,
) -> Result<(), CompilerFailure> {
    ensure_workspace_manifest(session_directory).map_err(|error| CompilerFailure {
        kind: CompilerFailureKind::ReadSource,
        message: format!("failed to write PACKAGE.coppice: {error}"),
        path: Some(
            session_directory
                .join("PACKAGE.coppice")
                .display()
                .to_string(),
        ),
        details: Vec::new(),
    })?;

    let source_path = session_directory.join("main.bin.coppice");
    fs::write(&source_path, source).map_err(|error| CompilerFailure {
        kind: CompilerFailureKind::ReadSource,
        message: format!("failed to write main.bin.coppice: {error}"),
        path: Some(source_path.display().to_string()),
        details: Vec::new(),
    })
}

pub fn check_workspace(
    session_directory: &Path,
) -> Result<compiler__driver::CheckedTarget, CompilerFailure> {
    let workspace_root = path_string(session_directory);
    check_target_with_workspace_root(".", Some(&workspace_root))
}

pub fn build_workspace_binary(session_directory: &Path) -> Result<PathBuf, CompilerFailure> {
    let workspace_root = path_string(session_directory);
    let output_directory = session_directory.join(".coppice").join("build");
    let output_directory_string = path_string(&output_directory);
    let built_target = build_target_with_workspace_root(
        "main.bin.coppice",
        Some(&workspace_root),
        Some(&output_directory_string),
    )?;
    Ok(PathBuf::from(built_target.executable_path))
}

pub async fn run_binary(
    binary_path: &Path,
    timeout_duration: Duration,
    max_output_bytes: usize,
) -> Result<RunExecution, CompilerFailure> {
    let mut command = Command::new(binary_path);
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let output = match tokio::time::timeout(timeout_duration, command.output()).await {
        Ok(result) => result.map_err(|error| CompilerFailure {
            kind: CompilerFailureKind::RunFailed,
            message: format!("failed to execute built program: {error}"),
            path: Some(binary_path.display().to_string()),
            details: Vec::new(),
        })?,
        Err(_) => {
            return Ok(RunExecution {
                exit_code: 124,
                stdout: String::new(),
                stderr: "execution timed out".to_string(),
                timed_out: true,
            });
        }
    };

    let stdout = truncate_utf8_lossy(&output.stdout, max_output_bytes);
    let stderr = truncate_utf8_lossy(&output.stderr, max_output_bytes);
    Ok(RunExecution {
        exit_code: output.status.code().unwrap_or(1),
        stdout,
        stderr,
        timed_out: false,
    })
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn truncate_utf8_lossy(bytes: &[u8], max_bytes: usize) -> String {
    if bytes.len() <= max_bytes {
        return String::from_utf8_lossy(bytes).to_string();
    }

    let mut output = String::from_utf8_lossy(&bytes[..max_bytes]).to_string();
    output.push_str("\n... output truncated ...");
    output
}
