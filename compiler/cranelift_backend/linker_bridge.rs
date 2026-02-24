use std::path::Path;
use std::process::Command;

use compiler__reports::CompilerFailure;
use runfiles::Runfiles;

use crate::build_failed;

pub(crate) fn link_executable(
    object_path: &Path,
    executable_path: &Path,
) -> Result<(), CompilerFailure> {
    let runfiles = Runfiles::create().map_err(|error| {
        build_failed(
            format!("failed to initialize runfiles for linker wrapper: {error}"),
            Some(executable_path),
        )
    })?;

    let linker_wrapper = runfiles
        .rlocation_from(env!("LLVM_LINKER_WRAPPER"), env!("REPOSITORY_NAME"))
        .ok_or_else(|| {
            build_failed(
                format!(
                    "failed to resolve runfile for linker wrapper: {}",
                    env!("LLVM_LINKER_WRAPPER")
                ),
                Some(executable_path),
            )
        })?;

    let runfiles_dir = runfiles::find_runfiles_dir().map_err(|error| {
        build_failed(
            format!("failed to locate runfiles directory for linker wrapper: {error}"),
            Some(executable_path),
        )
    })?;

    let output = Command::new(linker_wrapper)
        .arg(object_path)
        .arg("-o")
        .arg(executable_path)
        .env("RUNFILES_DIR", runfiles_dir)
        .output()
        .map_err(|error| {
            build_failed(
                format!("failed to invoke llvm linker wrapper: {error}"),
                Some(executable_path),
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(build_failed(
            format!(
                "llvm linker wrapper failed with status {}{}{}",
                output.status,
                if stderr.trim().is_empty() { "" } else { ": " },
                stderr.trim()
            ),
            Some(executable_path),
        ));
    }

    Ok(())
}
