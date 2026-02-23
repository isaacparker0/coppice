use std::path::Path;
use std::process::Command;

use compiler__reports::CompilerFailure;
use runfiles::{Runfiles, find_runfiles_dir};

use crate::build_failed;

pub(crate) fn link_executable(
    object_path: &Path,
    executable_path: &Path,
) -> Result<(), CompilerFailure> {
    let runfiles = Runfiles::create().map_err(|error| {
        build_failed(
            format!("failed to initialize runfiles while resolving linker wrapper: {error}"),
            Some(executable_path),
        )
    })?;
    let linker_wrapper_runfile = env!("COPPICE_LINKER_WRAPPER_RUNFILE");
    let linker_wrapper_path =
        runfiles.rlocation_from(linker_wrapper_runfile, env!("REPOSITORY_NAME"));
    let Some(linker_wrapper_path) = linker_wrapper_path else {
        return Err(build_failed(
            format!("failed to resolve linker wrapper runfile path '{linker_wrapper_runfile}'"),
            Some(executable_path),
        ));
    };

    let output = Command::new(&linker_wrapper_path)
        .envs(linker_runfiles_environment())
        .arg(object_path)
        .arg("-o")
        .arg(executable_path)
        .output()
        .map_err(|error| {
            build_failed(
                format!(
                    "failed to invoke hermetic linker wrapper '{}': {error}",
                    linker_wrapper_path.display()
                ),
                Some(executable_path),
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(build_failed(
            format!(
                "hermetic linker wrapper failed with status {}{}{}",
                output.status,
                if stderr.trim().is_empty() { "" } else { ": " },
                stderr.trim()
            ),
            Some(executable_path),
        ));
    }

    Ok(())
}

fn linker_runfiles_environment() -> Vec<(String, String)> {
    const RUNFILES_ENVIRONMENT_VARIABLES: [&str; 3] =
        ["RUNFILES_DIR", "RUNFILES_MANIFEST_FILE", "TEST_SRCDIR"];

    let mut environment: Vec<(String, String)> = RUNFILES_ENVIRONMENT_VARIABLES
        .iter()
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .map(|value| ((*name).to_string(), value))
        })
        .collect();

    if !environment
        .iter()
        .any(|(name, _)| name == "RUNFILES_DIR" || name == "RUNFILES_MANIFEST_FILE")
    {
        if let Ok(runfiles_directory) = find_runfiles_dir() {
            if let Some(value) = runfiles_directory.to_str() {
                environment.push(("RUNFILES_DIR".to_string(), value.to_string()));
            }

            let manifest_path = runfiles_directory.join("MANIFEST");
            if manifest_path.is_file()
                && let Some(value) = manifest_path.to_str()
            {
                environment.push(("RUNFILES_MANIFEST_FILE".to_string(), value.to_string()));
            }
        } else if let Ok(current_executable_path) = std::env::current_exe() {
            let manifest_candidates = [
                format!("{}.runfiles_manifest", current_executable_path.display()),
                format!(
                    "{}.exe.runfiles_manifest",
                    current_executable_path.display()
                ),
            ];

            for manifest_candidate in manifest_candidates {
                let manifest_path = std::path::PathBuf::from(&manifest_candidate);
                if manifest_path.is_file() {
                    environment.push(("RUNFILES_MANIFEST_FILE".to_string(), manifest_candidate));
                    break;
                }
            }
        }
    }

    environment
}
