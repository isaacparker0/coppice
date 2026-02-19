use std::fs;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use compiler__reports::{CompilerFailure, CompilerFailureKind};
use runfiles::Runfiles;
use tokio::process::Command;

use crate::path_sanitizer::sanitize_workspace_path;
use crate::session_store::ensure_workspace_manifest;

pub struct RunExecution {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

pub struct CheckExecution {
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
        message: format!("failed to write PACKAGE.copp: {error}"),
        path: Some(session_directory.join("PACKAGE.copp").display().to_string()),
        details: Vec::new(),
    })?;

    let source_path = session_directory.join("main.bin.copp");
    fs::write(&source_path, source).map_err(|error| CompilerFailure {
        kind: CompilerFailureKind::ReadSource,
        message: format!("failed to write main.bin.copp: {error}"),
        path: Some(source_path.display().to_string()),
        details: Vec::new(),
    })
}

pub async fn check_workspace_via_cli(
    session_directory: &Path,
    timeout_duration: Duration,
    max_output_bytes: usize,
) -> Result<CheckExecution, CompilerFailure> {
    let cli_path = resolve_compiler_cli_path()?;

    let mut command = Command::new(cli_path);
    command
        .arg("--workspace-root")
        .arg(session_directory)
        .arg("check")
        .arg("--format")
        .arg("json")
        .arg("main.bin.copp")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = match tokio::time::timeout(timeout_duration, command.output()).await {
        Ok(result) => result.map_err(|error| CompilerFailure {
            kind: CompilerFailureKind::RunFailed,
            message: format!("failed to execute check via compiler cli: {error}"),
            path: Some(session_directory.display().to_string()),
            details: Vec::new(),
        })?,
        Err(_) => {
            return Ok(CheckExecution {
                exit_code: 124,
                stdout: String::new(),
                stderr: "check timed out".to_string(),
                timed_out: true,
            });
        }
    };

    let stdout = truncate_utf8_lossy(&output.stdout, max_output_bytes);
    let stderr = sanitize_workspace_path(
        &truncate_utf8_lossy(&output.stderr, max_output_bytes),
        session_directory,
    );
    Ok(CheckExecution {
        exit_code: output.status.code().unwrap_or(1),
        stdout,
        stderr,
        timed_out: false,
    })
}

pub async fn run_workspace_via_cli(
    session_directory: &Path,
    timeout_duration: Duration,
    max_output_bytes: usize,
) -> Result<RunExecution, CompilerFailure> {
    let cli_path = resolve_compiler_cli_path()?;

    let mut command = Command::new(cli_path);
    command
        .arg("--workspace-root")
        .arg(session_directory)
        .arg("run")
        .arg("main.bin.copp")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = match tokio::time::timeout(timeout_duration, command.output()).await {
        Ok(result) => result.map_err(|error| CompilerFailure {
            kind: CompilerFailureKind::RunFailed,
            message: format!("failed to execute run via compiler cli: {error}"),
            path: Some(session_directory.display().to_string()),
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
    let stderr = sanitize_workspace_path(
        &truncate_utf8_lossy(&output.stderr, max_output_bytes),
        session_directory,
    );
    Ok(RunExecution {
        exit_code: output.status.code().unwrap_or(1),
        stdout,
        stderr,
        timed_out: false,
    })
}

fn resolve_compiler_cli_path() -> Result<std::path::PathBuf, CompilerFailure> {
    let runfiles = Runfiles::create().map_err(|error| CompilerFailure {
        kind: CompilerFailureKind::RunFailed,
        message: format!("failed to initialize runfiles for compiler cli: {error}"),
        path: None,
        details: Vec::new(),
    })?;
    runfiles
        .rlocation("_main/compiler/cli/main")
        .ok_or_else(|| CompilerFailure {
            kind: CompilerFailureKind::RunFailed,
            message: "failed to resolve runfiles path for compiler/cli/main".to_string(),
            path: None,
            details: Vec::new(),
        })
}

fn truncate_utf8_lossy(bytes: &[u8], max_output_bytes: usize) -> String {
    if bytes.len() <= max_output_bytes {
        return String::from_utf8_lossy(bytes).to_string();
    }

    let mut output = String::from_utf8_lossy(&bytes[..max_output_bytes]).to_string();
    output.push_str("\n... output truncated ...");
    output
}
