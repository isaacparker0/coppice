use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use compiler__reports::{CompilerFailure, CompilerFailureKind};
use runfiles::Runfiles;
use tokio::process::Command;

use crate::models::WorkspaceFileRequest;
use crate::path_sanitizer::sanitize_workspace_path;

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

pub fn write_workspace_files(
    session_directory: &Path,
    files: &[WorkspaceFileRequest],
) -> Result<(), CompilerFailure> {
    reset_session_directory(session_directory)?;

    let mut written_paths = HashSet::<String>::new();
    for file in files {
        let relative_path = normalize_workspace_relative_path(&file.path)?;
        let relative_path_key = relative_path.to_string_lossy().to_string();
        if written_paths.contains(&relative_path_key) {
            return Err(CompilerFailure {
                kind: CompilerFailureKind::ReadSource,
                message: format!("duplicate file path: {relative_path_key}"),
                path: Some(relative_path_key),
                details: Vec::new(),
            });
        }
        written_paths.insert(relative_path_key);

        let absolute_path = session_directory.join(&relative_path);
        if let Some(parent_directory) = absolute_path.parent() {
            fs::create_dir_all(parent_directory).map_err(|error| CompilerFailure {
                kind: CompilerFailureKind::ReadSource,
                message: format!("failed to create directory for {}: {error}", file.path),
                path: Some(file.path.clone()),
                details: Vec::new(),
            })?;
        }

        fs::write(&absolute_path, &file.source).map_err(|error| CompilerFailure {
            kind: CompilerFailureKind::ReadSource,
            message: format!("failed to write {}: {error}", file.path),
            path: Some(file.path.clone()),
            details: Vec::new(),
        })?;
    }

    Ok(())
}

pub async fn check_workspace_via_cli(
    session_directory: &Path,
    entrypoint_path: &str,
    timeout_duration: Duration,
    max_output_bytes: usize,
) -> Result<CheckExecution, CompilerFailure> {
    let cli_path = resolve_compiler_cli_path()?;

    let mut command = Command::new(cli_path);
    command
        .arg("check")
        .arg("--format")
        .arg("json")
        .arg(entrypoint_path)
        .current_dir(session_directory)
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
    entrypoint_path: &str,
    timeout_duration: Duration,
    max_output_bytes: usize,
) -> Result<RunExecution, CompilerFailure> {
    let cli_path = resolve_compiler_cli_path()?;

    let mut command = Command::new(cli_path);
    command
        .arg("run")
        .arg(entrypoint_path)
        .current_dir(session_directory)
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

fn resolve_compiler_cli_path() -> Result<PathBuf, CompilerFailure> {
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

fn reset_session_directory(session_directory: &Path) -> Result<(), CompilerFailure> {
    if session_directory.exists() {
        fs::remove_dir_all(session_directory).map_err(|error| CompilerFailure {
            kind: CompilerFailureKind::ReadSource,
            message: format!("failed to reset session directory: {error}"),
            path: Some(session_directory.display().to_string()),
            details: Vec::new(),
        })?;
    }

    fs::create_dir_all(session_directory).map_err(|error| CompilerFailure {
        kind: CompilerFailureKind::ReadSource,
        message: format!("failed to initialize session directory: {error}"),
        path: Some(session_directory.display().to_string()),
        details: Vec::new(),
    })
}

fn normalize_workspace_relative_path(path: &str) -> Result<PathBuf, CompilerFailure> {
    let trimmed_path = path.trim();
    if trimmed_path.is_empty() {
        return Err(invalid_workspace_path("file path cannot be empty", path));
    }
    let relative_path = Path::new(trimmed_path);
    if relative_path.is_absolute() {
        return Err(invalid_workspace_path(
            "file path must be relative to workspace root",
            trimmed_path,
        ));
    }

    if relative_path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(invalid_workspace_path(
            "file path cannot contain parent traversal",
            trimmed_path,
        ));
    }

    Ok(relative_path.to_path_buf())
}

fn invalid_workspace_path(message: &str, path: &str) -> CompilerFailure {
    CompilerFailure {
        kind: CompilerFailureKind::ReadSource,
        message: message.to_string(),
        path: Some(path.to_string()),
        details: Vec::new(),
    }
}

fn truncate_utf8_lossy(bytes: &[u8], max_output_bytes: usize) -> String {
    if bytes.len() <= max_output_bytes {
        return String::from_utf8_lossy(bytes).to_string();
    }

    let mut output = String::from_utf8_lossy(&bytes[..max_output_bytes]).to_string();
    output.push_str("\n... output truncated ...");
    output
}
