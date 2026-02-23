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
    let linker_wrapper_runfile = env!("LINKER_WRAPPER");
    let linker_wrapper_path =
        runfiles.rlocation_from(linker_wrapper_runfile, env!("REPOSITORY_NAME"));
    let Some(linker_wrapper_path) = linker_wrapper_path else {
        return Err(build_failed(
            format!("failed to resolve linker wrapper runfile path '{linker_wrapper_runfile}'"),
            Some(executable_path),
        ));
    };
    let runfiles_directory = find_runfiles_dir().map_err(|error| {
        build_failed(
            format!("failed to locate runfiles directory for linker wrapper: {error}"),
            Some(executable_path),
        )
    })?;

    let output = Command::new(&linker_wrapper_path)
        .env("RUNFILES_DIR", &runfiles_directory)
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
