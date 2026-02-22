use std::path::Path;
use std::process::Command;

use compiler__reports::CompilerFailure;

use crate::build_failed;

pub(crate) fn link_executable(
    object_path: &Path,
    executable_path: &Path,
) -> Result<(), CompilerFailure> {
    // TODO(isaac): Replace this host-linker bridge with a hermetic toolchain
    // wrapper once toolchains_llvm_bootstrapped exposes a  non-Bazel linker
    // interface that carries full platform link contract details.
    // https://bazelbuild.slack.com/archives/C05UNC5AANQ/p1771610074794149
    let output = match std::env::consts::OS {
        "macos" => Command::new("/usr/bin/xcrun")
            .arg("--sdk")
            .arg("macosx")
            .arg("clang++")
            .arg(object_path)
            .arg("-o")
            .arg(executable_path)
            .output()
            .map_err(|error| {
                build_failed(
                    format!("failed to invoke host linker bridge via xcrun clang++: {error}"),
                    Some(executable_path),
                )
            })?,
        "linux" => Command::new("clang++")
            .arg(object_path)
            .arg("-o")
            .arg(executable_path)
            .output()
            .map_err(|error| {
                build_failed(
                    format!("failed to invoke host linker bridge via clang++: {error}"),
                    Some(executable_path),
                )
            })?,
        unsupported_os => {
            return Err(build_failed(
                format!("host-linker bridge is not implemented for target OS '{unsupported_os}'"),
                Some(executable_path),
            ));
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(build_failed(
            format!(
                "system linker failed with status {}{}{}",
                output.status,
                if stderr.trim().is_empty() { "" } else { ": " },
                stderr.trim()
            ),
            Some(executable_path),
        ));
    }

    Ok(())
}
