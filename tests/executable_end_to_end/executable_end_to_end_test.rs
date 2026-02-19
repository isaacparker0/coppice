use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use runfiles::{Runfiles, rlocation};

enum Mode {
    Check,
    Update { workspace_directory: PathBuf },
}

#[test]
fn executable_end_to_end_cases() {
    let runfiles = Runfiles::create().unwrap();
    let compiler = rlocation!(runfiles, "_main/compiler/cli/main").unwrap();
    let runfiles_directory = runfiles::find_runfiles_dir().unwrap().join("_main");

    let mode = if env::var("UPDATE_SNAPSHOTS").is_ok() {
        let workspace_directory = env::var("BUILD_WORKSPACE_DIRECTORY").unwrap();
        Mode::Update {
            workspace_directory: PathBuf::from(workspace_directory),
        }
    } else {
        Mode::Check
    };

    let mut case_paths = Vec::new();
    collect_cases(
        &runfiles_directory.join("tests/executable_end_to_end"),
        &runfiles_directory,
        &mut case_paths,
    );
    case_paths.sort();
    assert!(
        !case_paths.is_empty(),
        "no executable end-to-end cases found"
    );

    for case_path in &case_paths {
        run_case(&compiler, &runfiles_directory, case_path, &mode);
    }
}

fn collect_cases(dir: &Path, runfiles_directory: &Path, case_paths: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            if path.join("expect.exit").is_file() {
                let case_path = path.strip_prefix(runfiles_directory).unwrap();
                case_paths.push(case_path.to_path_buf());
            } else {
                collect_cases(&path, runfiles_directory, case_paths);
            }
        }
    }
}

fn run_case(compiler: &Path, runfiles_directory: &Path, case_path: &Path, mode: &Mode) {
    let case_directory = runfiles_directory.join(case_path);
    let input_directory = case_directory.join("input");
    assert!(
        input_directory.is_dir(),
        "missing input directory for executable end-to-end case: {}",
        case_path.display()
    );

    let temp_output_directory = env::temp_dir().join(format!(
        "coppice_end_to_end_{}_{}",
        sanitize_case_name(case_path),
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&temp_output_directory);
    fs::create_dir_all(&temp_output_directory).unwrap();

    let invoke_args_text = fs::read_to_string(case_directory.join("invoke.args")).unwrap();
    let invoke_args =
        parse_lines_with_placeholders(&invoke_args_text, &temp_output_directory, &input_directory);

    let output = Command::new(compiler)
        .args(&invoke_args)
        .current_dir(&input_directory)
        .output()
        .unwrap();

    let actual_exit = output.status.code().unwrap_or(1);
    let actual_stdout = trim_one_trailing_newline(&String::from_utf8_lossy(&output.stdout));
    let actual_stderr = trim_one_trailing_newline(&String::from_utf8_lossy(&output.stderr));
    let actual_artifact_paths = collect_artifact_paths(&temp_output_directory);

    match mode {
        Mode::Update {
            workspace_directory,
        } => {
            let source_case_directory = workspace_directory.join(case_path);
            let snapshot_stdout = normalize_output_for_snapshot(
                &actual_stdout,
                &temp_output_directory,
                &input_directory,
            );
            let snapshot_stderr = normalize_output_for_snapshot(
                &actual_stderr,
                &temp_output_directory,
                &input_directory,
            );
            write_if_changed(
                &source_case_directory.join("expect.exit"),
                format!("{actual_exit}\n"),
                case_path,
            );
            write_if_changed(
                &source_case_directory.join("expect.stdout"),
                format!("{snapshot_stdout}\n"),
                case_path,
            );
            write_if_changed(
                &source_case_directory.join("expect.stderr"),
                format!("{snapshot_stderr}\n"),
                case_path,
            );
            let artifact_placeholders =
                collect_artifact_placeholders(&actual_artifact_paths, &temp_output_directory);
            let artifact_lines = if artifact_placeholders.is_empty() {
                String::new()
            } else {
                format!("{}\n", artifact_placeholders.join("\n"))
            };
            write_if_changed(
                &source_case_directory.join("expect.artifacts"),
                artifact_lines,
                case_path,
            );
        }
        Mode::Check => {
            let expected_exit: i32 = fs::read_to_string(case_directory.join("expect.exit"))
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            let expected_stdout = read_expected_output_snapshot(
                &case_directory.join("expect.stdout"),
                &temp_output_directory,
                &input_directory,
                case_path,
                "expect.stdout",
            );
            let expected_stderr = read_expected_output_snapshot(
                &case_directory.join("expect.stderr"),
                &temp_output_directory,
                &input_directory,
                case_path,
                "expect.stderr",
            );
            let artifacts_path = case_directory.join("expect.artifacts");
            assert!(
                artifacts_path.is_file(),
                "missing expect.artifacts for executable end-to-end case: {}",
                case_path.display()
            );
            let expected_artifacts = parse_lines_with_placeholders(
                &fs::read_to_string(artifacts_path).unwrap(),
                &temp_output_directory,
                &input_directory,
            );

            assert_eq!(
                expected_exit,
                actual_exit,
                "exit code mismatch for {}",
                case_path.display()
            );
            assert_eq!(
                expected_stdout,
                actual_stdout,
                "stdout mismatch for {}",
                case_path.display()
            );
            assert_eq!(
                expected_stderr,
                actual_stderr,
                "stderr mismatch for {}",
                case_path.display()
            );
            assert_eq!(
                expected_artifacts,
                actual_artifact_paths,
                "artifact list mismatch for {}",
                case_path.display()
            );
            for artifact in &actual_artifact_paths {
                let artifact_path = PathBuf::from(artifact);
                assert!(
                    artifact_path.exists(),
                    "expected artifact missing for {}: {}",
                    case_path.display(),
                    artifact_path.display()
                );
            }
        }
    }

    let _ = fs::remove_dir_all(&temp_output_directory);
}

fn collect_artifact_paths(directory: &Path) -> Vec<String> {
    let mut artifacts = Vec::new();
    if let Ok(entries) = fs::read_dir(directory) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                artifacts.extend(collect_artifact_paths(&path));
                continue;
            }
            if path.is_file() {
                artifacts.push(path.to_string_lossy().to_string());
            }
        }
    }
    artifacts.sort();
    artifacts
}

fn collect_artifact_placeholders(
    artifact_paths: &[String],
    temp_output_directory: &Path,
) -> Vec<String> {
    let mut artifact_placeholders = Vec::new();
    for artifact_path in artifact_paths {
        let relative_path = Path::new(artifact_path)
            .strip_prefix(temp_output_directory)
            .unwrap();
        artifact_placeholders.push(format!(
            "${{TMP_OUTPUT_DIR}}/{}",
            relative_path.to_string_lossy()
        ));
    }
    artifact_placeholders
}

fn parse_lines_with_placeholders(
    contents: &str,
    temp_output_directory: &Path,
    input_directory: &Path,
) -> Vec<String> {
    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('#'))
        .map(|line| substitute_placeholders(line, temp_output_directory, input_directory))
        .collect()
}

fn substitute_placeholders(
    template: &str,
    temp_output_directory: &Path,
    input_directory: &Path,
) -> String {
    template
        .replace(
            "${TMP_OUTPUT_DIR}",
            temp_output_directory.to_string_lossy().as_ref(),
        )
        .replace("${INPUT_DIR}", input_directory.to_string_lossy().as_ref())
}

fn trim_one_trailing_newline(value: &str) -> String {
    value.strip_suffix('\n').unwrap_or(value).to_string()
}

fn read_expected_output_snapshot(
    path: &Path,
    temp_output_directory: &Path,
    input_directory: &Path,
    case_path: &Path,
    file_name: &str,
) -> String {
    let raw_contents = fs::read_to_string(path).unwrap_or_else(|error| {
        panic!(
            "failed to read {} for executable end-to-end case {}: {}",
            file_name,
            case_path.display(),
            error
        )
    });
    assert!(
        raw_contents.ends_with('\n'),
        "{} must end with a trailing newline for executable end-to-end case {}",
        file_name,
        case_path.display()
    );
    let substituted_contents =
        substitute_placeholders(&raw_contents, temp_output_directory, input_directory);
    trim_one_trailing_newline(&substituted_contents)
}

fn normalize_output_for_snapshot(
    value: &str,
    temp_output_directory: &Path,
    input_directory: &Path,
) -> String {
    value
        .replace(
            temp_output_directory.to_string_lossy().as_ref(),
            "${TMP_OUTPUT_DIR}",
        )
        .replace(input_directory.to_string_lossy().as_ref(), "${INPUT_DIR}")
}

fn sanitize_case_name(case_path: &Path) -> String {
    case_path
        .to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn write_if_changed(path: &Path, content: String, case_path: &Path) {
    let existing = fs::read_to_string(path).unwrap_or_default();
    if existing != content {
        fs::write(path, content).unwrap();
        println!("updated: {}", case_path.display());
    }
}
