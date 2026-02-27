use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use runfiles::{Runfiles, rlocation};
use tests__snapshot_fixture_helpers::collect_snapshot_fixture_case_paths;

#[test]
fn unified_cases() {
    let runfiles = Runfiles::create().unwrap();
    let compiler = rlocation!(runfiles, "_main/compiler/cli/main").unwrap();
    let runfiles_directory = runfiles::find_runfiles_dir().unwrap().join("_main");

    let mut case_paths = Vec::new();
    collect_snapshot_fixture_case_paths(
        &runfiles_directory.join("unified_tests"),
        &runfiles_directory,
        "case.test",
        &mut case_paths,
    );
    case_paths.sort();
    assert!(!case_paths.is_empty(), "no unified fixture cases found");

    for case_path in &case_paths {
        run_case(&compiler, &runfiles_directory, case_path);
    }
}

fn run_case(compiler: &Path, runfiles_directory: &Path, case_path: &Path) {
    let case_directory = runfiles_directory.join(case_path);
    let input_directory = case_directory.join("input");
    assert!(
        input_directory.is_dir(),
        "missing input directory for case {}",
        case_path.display()
    );

    let script_path = case_directory.join("case.test");
    let script_contents = fs::read_to_string(&script_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", script_path.display()));
    let run_blocks = parse_case_script(&script_contents, case_path);
    assert!(
        !run_blocks.is_empty(),
        "case.test has no run blocks for {}",
        case_path.display()
    );

    let temp_case_directory = env::temp_dir().join(format!(
        "coppice_unified_case_{}_{}",
        sanitize_case_name(case_path),
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&temp_case_directory);
    fs::create_dir_all(&temp_case_directory).unwrap();

    let working_input_directory = temp_case_directory.join("input");
    copy_directory_tree(&input_directory, &working_input_directory);

    for (run_index, run_block) in run_blocks.iter().enumerate() {
        let run_output_directory = temp_case_directory.join(format!("output_{}", run_index + 1));
        let _ = fs::remove_dir_all(&run_output_directory);
        fs::create_dir_all(&run_output_directory).unwrap();
        run_block_and_assert(
            compiler,
            &case_directory,
            case_path,
            &working_input_directory,
            &run_output_directory,
            run_block,
            run_index + 1,
        );
    }

    let _ = fs::remove_dir_all(&temp_case_directory);
}

fn run_block_and_assert(
    compiler: &Path,
    case_directory: &Path,
    case_path: &Path,
    working_input_directory: &Path,
    run_output_directory: &Path,
    run_block: &RunBlock,
    run_number: usize,
) {
    let command_args = parse_command_tokens(&run_block.command_line);
    assert!(
        !command_args.is_empty(),
        "empty command in run {} for {}",
        run_number,
        case_path.display()
    );
    let prepared_command_args = prepare_command_args_for_execution(&command_args);
    let substituted_command_args = prepared_command_args
        .iter()
        .map(|token| substitute_placeholders(token, run_output_directory, working_input_directory))
        .collect::<Vec<_>>();

    let output = Command::new(compiler)
        .args(&substituted_command_args)
        .current_dir(working_input_directory)
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "failed to execute run {} for {}: {error}",
                run_number,
                case_path.display()
            )
        });

    let actual_exit = output.status.code().unwrap_or(1);
    let actual_stdout = normalize_process_output(&String::from_utf8_lossy(&output.stdout));
    let actual_stderr = normalize_process_output(&String::from_utf8_lossy(&output.stderr));

    if command_name(&run_block.command_line) == "build" {
        let expected_text_exit = run_block
            .expected_text_exit
            .or(run_block.expected_exit)
            .unwrap_or_else(|| {
                panic!(
                    "missing text exit expectation for run {} ({}) in {}",
                    run_number,
                    run_block.command_line,
                    case_path.display()
                )
            });
        let expected_json_exit = run_block
            .expected_json_exit
            .or(run_block.expected_exit)
            .unwrap_or_else(|| {
                panic!(
                    "missing json exit expectation for run {} ({}) in {}",
                    run_number,
                    run_block.command_line,
                    case_path.display()
                )
            });
        assert_eq!(
            expected_text_exit,
            actual_exit,
            "text exit code mismatch for run {} ({}) in {}",
            run_number,
            run_block.command_line,
            case_path.display()
        );
        assert_eq!(
            expected_json_exit,
            actual_exit,
            "json exit code mismatch for run {} ({}) in {}",
            run_number,
            run_block.command_line,
            case_path.display()
        );

        let text_stdout_path = run_block
            .expected_text_stdout_path
            .as_ref()
            .or(run_block.expected_stdout_path.as_ref())
            .unwrap_or_else(|| {
                panic!(
                    "missing text stdout expectation for run {} ({}) in {}",
                    run_number,
                    run_block.command_line,
                    case_path.display()
                )
            });
        let json_stdout_path = run_block
            .expected_json_stdout_path
            .as_ref()
            .or(run_block.expected_stdout_path.as_ref())
            .unwrap_or_else(|| {
                panic!(
                    "missing json stdout expectation for run {} ({}) in {}",
                    run_number,
                    run_block.command_line,
                    case_path.display()
                )
            });
        let text_stderr_path = run_block
            .expected_text_stderr_path
            .as_ref()
            .or(run_block.expected_stderr_path.as_ref())
            .unwrap_or_else(|| {
                panic!(
                    "missing text stderr expectation for run {} ({}) in {}",
                    run_number,
                    run_block.command_line,
                    case_path.display()
                )
            });
        let json_stderr_path = run_block
            .expected_json_stderr_path
            .as_ref()
            .or(run_block.expected_stderr_path.as_ref())
            .unwrap_or_else(|| {
                panic!(
                    "missing json stderr expectation for run {} ({}) in {}",
                    run_number,
                    run_block.command_line,
                    case_path.display()
                )
            });

        let expected_text_stdout = read_expected_text_file(
            case_directory,
            text_stdout_path,
            case_path,
            run_output_directory,
            working_input_directory,
            run_number,
            "text.stdout",
        );
        let expected_json_stdout = read_expected_text_file(
            case_directory,
            json_stdout_path,
            case_path,
            run_output_directory,
            working_input_directory,
            run_number,
            "json.stdout",
        );
        let expected_text_stderr = read_expected_text_file(
            case_directory,
            text_stderr_path,
            case_path,
            run_output_directory,
            working_input_directory,
            run_number,
            "text.stderr",
        );
        let expected_json_stderr = read_expected_text_file(
            case_directory,
            json_stderr_path,
            case_path,
            run_output_directory,
            working_input_directory,
            run_number,
            "json.stderr",
        );
        assert_eq!(
            expected_text_stdout,
            actual_stdout,
            "text stdout mismatch for run {} ({}) in {}",
            run_number,
            run_block.command_line,
            case_path.display()
        );
        assert_eq!(
            expected_json_stdout,
            actual_stdout,
            "json stdout mismatch for run {} ({}) in {}",
            run_number,
            run_block.command_line,
            case_path.display()
        );
        assert_eq!(
            expected_text_stderr,
            actual_stderr,
            "text stderr mismatch for run {} ({}) in {}",
            run_number,
            run_block.command_line,
            case_path.display()
        );
        assert_eq!(
            expected_json_stderr,
            actual_stderr,
            "json stderr mismatch for run {} ({}) in {}",
            run_number,
            run_block.command_line,
            case_path.display()
        );
    } else {
        if let Some(expected_exit) = run_block.expected_exit {
            assert_eq!(
                expected_exit,
                actual_exit,
                "exit code mismatch for run {} ({}) in {}",
                run_number,
                run_block.command_line,
                case_path.display()
            );
        }
        if let Some(expected_stdout_path) = &run_block.expected_stdout_path {
            let expected_stdout = read_expected_text_file(
                case_directory,
                expected_stdout_path,
                case_path,
                run_output_directory,
                working_input_directory,
                run_number,
                "stdout",
            );
            assert_eq!(
                expected_stdout,
                actual_stdout,
                "stdout mismatch for run {} ({}) in {}",
                run_number,
                run_block.command_line,
                case_path.display()
            );
        }
        if let Some(expected_stderr_path) = &run_block.expected_stderr_path {
            let expected_stderr = read_expected_text_file(
                case_directory,
                expected_stderr_path,
                case_path,
                run_output_directory,
                working_input_directory,
                run_number,
                "stderr",
            );
            assert_eq!(
                expected_stderr,
                actual_stderr,
                "stderr mismatch for run {} ({}) in {}",
                run_number,
                run_block.command_line,
                case_path.display()
            );
        }
    }

    if let Some(expected_artifacts_path) = &run_block.expected_artifacts_path {
        let expected_artifacts = read_expected_artifact_lines(
            case_directory,
            expected_artifacts_path,
            case_path,
            run_output_directory,
            working_input_directory,
            run_number,
        );
        let actual_artifacts = collect_artifact_paths(run_output_directory);
        assert_eq!(
            expected_artifacts,
            actual_artifacts,
            "artifact list mismatch for run {} ({}) in {}",
            run_number,
            run_block.command_line,
            case_path.display()
        );
    }
}

#[derive(Default)]
struct RunBlock {
    command_line: String,
    expected_exit: Option<i32>,
    expected_stdout_path: Option<PathBuf>,
    expected_stderr_path: Option<PathBuf>,
    expected_text_exit: Option<i32>,
    expected_json_exit: Option<i32>,
    expected_text_stdout_path: Option<PathBuf>,
    expected_text_stderr_path: Option<PathBuf>,
    expected_json_stdout_path: Option<PathBuf>,
    expected_json_stderr_path: Option<PathBuf>,
    expected_artifacts_path: Option<PathBuf>,
}

fn parse_case_script(contents: &str, case_path: &Path) -> Vec<RunBlock> {
    let mut run_blocks = Vec::new();
    let mut current_run_block: Option<RunBlock> = None;

    for (line_index, raw_line) in contents.lines().enumerate() {
        let line_number = line_index + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(command_line) = line.strip_prefix("$ ") {
            if let Some(existing_run_block) = current_run_block.take() {
                run_blocks.push(existing_run_block);
            }
            current_run_block = Some(RunBlock {
                command_line: command_line.trim().to_string(),
                ..RunBlock::default()
            });
            continue;
        }
        if let Some(assertion) = line.strip_prefix("> ") {
            let run_block = current_run_block.as_mut().unwrap_or_else(|| {
                panic!(
                    "assertion before first command at line {} in {}",
                    line_number,
                    case_path.display()
                )
            });
            parse_assertion_line(assertion, run_block, case_path, line_number);
            continue;
        }
        panic!(
            "unrecognized line at {}:{}: {}",
            case_path.display(),
            line_number,
            line
        );
    }

    if let Some(existing_run_block) = current_run_block.take() {
        run_blocks.push(existing_run_block);
    }

    for run_block in &run_blocks {
        let has_basic_assertions = run_block.expected_exit.is_some()
            || run_block
                .expected_stdout_path
                .as_ref()
                .is_some_and(|path| !path.as_os_str().is_empty())
            || run_block
                .expected_stderr_path
                .as_ref()
                .is_some_and(|path| !path.as_os_str().is_empty())
            || run_block.expected_artifacts_path.is_some();
        let has_format_assertions = run_block.expected_text_exit.is_some()
            || run_block.expected_json_exit.is_some()
            || run_block
                .expected_text_stdout_path
                .as_ref()
                .is_some_and(|path| !path.as_os_str().is_empty())
            || run_block
                .expected_text_stderr_path
                .as_ref()
                .is_some_and(|path| !path.as_os_str().is_empty())
            || run_block
                .expected_json_stdout_path
                .as_ref()
                .is_some_and(|path| !path.as_os_str().is_empty())
            || run_block
                .expected_json_stderr_path
                .as_ref()
                .is_some_and(|path| !path.as_os_str().is_empty());
        assert!(
            has_basic_assertions || has_format_assertions,
            "run block with command '{}' in {} has no assertions",
            run_block.command_line,
            case_path.display()
        );
    }

    run_blocks
}

fn parse_assertion_line(
    assertion: &str,
    run_block: &mut RunBlock,
    case_path: &Path,
    line_number: usize,
) {
    let (key, value) = assertion.split_once(' ').unwrap_or_else(|| {
        panic!(
            "invalid assertion at {}:{} (expected '<key> <value>')",
            case_path.display(),
            line_number
        )
    });
    match key {
        "exit" => {
            run_block.expected_exit = Some(value.parse::<i32>().unwrap_or_else(|error| {
                panic!(
                    "invalid exit code at {}:{}: {} ({error})",
                    case_path.display(),
                    line_number,
                    value
                )
            }));
        }
        "stdout" => {
            run_block.expected_stdout_path = Some(parse_expected_file_reference(
                value,
                case_path,
                line_number,
                "stdout",
            ));
        }
        "stderr" => {
            run_block.expected_stderr_path = Some(parse_expected_file_reference(
                value,
                case_path,
                line_number,
                "stderr",
            ));
        }
        "artifacts" => {
            run_block.expected_artifacts_path = Some(parse_expected_file_reference(
                value,
                case_path,
                line_number,
                "artifacts",
            ));
        }
        "text.exit" => {
            run_block.expected_text_exit = Some(value.parse::<i32>().unwrap_or_else(|error| {
                panic!(
                    "invalid text.exit code at {}:{}: {} ({error})",
                    case_path.display(),
                    line_number,
                    value
                )
            }));
        }
        "json.exit" => {
            run_block.expected_json_exit = Some(value.parse::<i32>().unwrap_or_else(|error| {
                panic!(
                    "invalid json.exit code at {}:{}: {} ({error})",
                    case_path.display(),
                    line_number,
                    value
                )
            }));
        }
        "text.stdout" => {
            run_block.expected_text_stdout_path = Some(parse_expected_file_reference(
                value,
                case_path,
                line_number,
                "text.stdout",
            ));
        }
        "text.stderr" => {
            run_block.expected_text_stderr_path = Some(parse_expected_file_reference(
                value,
                case_path,
                line_number,
                "text.stderr",
            ));
        }
        "json.stdout" => {
            run_block.expected_json_stdout_path = Some(parse_expected_file_reference(
                value,
                case_path,
                line_number,
                "json.stdout",
            ));
        }
        "json.stderr" => {
            run_block.expected_json_stderr_path = Some(parse_expected_file_reference(
                value,
                case_path,
                line_number,
                "json.stderr",
            ));
        }
        _ => panic!(
            "unknown assertion key '{}' at {}:{}",
            key,
            case_path.display(),
            line_number
        ),
    }
}

fn parse_expected_file_reference(
    value: &str,
    case_path: &Path,
    line_number: usize,
    key: &str,
) -> PathBuf {
    let path = value.strip_prefix('@').unwrap_or_else(|| {
        panic!(
            "expected @<path> for '{}' at {}:{}",
            key,
            case_path.display(),
            line_number
        )
    });
    PathBuf::from(path)
}

fn parse_command_tokens(command_line: &str) -> Vec<String> {
    command_line
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect()
}

fn prepare_command_args_for_execution(command_args: &[String]) -> Vec<String> {
    let command_name = command_args[0].as_str();
    if command_name != "build" && command_name != "run" {
        return command_args.to_vec();
    }
    let mut prepared = command_args.to_vec();
    prepared.push("--output-dir".to_string());
    prepared.push("${TMP_OUTPUT_DIR}".to_string());
    prepared
}

fn command_name(command_line: &str) -> &str {
    command_line.split_whitespace().next().unwrap_or_default()
}

fn read_expected_text_file(
    case_directory: &Path,
    relative_path: &Path,
    case_path: &Path,
    run_output_directory: &Path,
    working_input_directory: &Path,
    run_number: usize,
    stream_name: &str,
) -> String {
    let full_path = case_directory.join(relative_path);
    let raw_contents = fs::read_to_string(&full_path).unwrap_or_else(|error| {
        panic!(
            "failed to read {} expectation '{}' for run {} in {}: {error}",
            stream_name,
            relative_path.display(),
            run_number,
            case_path.display()
        )
    });
    let contents = raw_contents.strip_suffix('\n').unwrap_or(&raw_contents);
    substitute_placeholders(contents, run_output_directory, working_input_directory)
}

fn read_expected_artifact_lines(
    case_directory: &Path,
    relative_path: &Path,
    case_path: &Path,
    run_output_directory: &Path,
    working_input_directory: &Path,
    run_number: usize,
) -> Vec<String> {
    let full_path = case_directory.join(relative_path);
    let raw_contents = fs::read_to_string(&full_path).unwrap_or_else(|error| {
        panic!(
            "failed to read artifacts expectation '{}' for run {} in {}: {error}",
            relative_path.display(),
            run_number,
            case_path.display()
        )
    });
    raw_contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('#'))
        .map(|line| substitute_placeholders(line, run_output_directory, working_input_directory))
        .collect()
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

fn copy_directory_tree(source_directory: &Path, destination_directory: &Path) {
    fs::create_dir_all(destination_directory).unwrap();
    let source_entries = fs::read_dir(source_directory).unwrap();
    for entry in source_entries.flatten() {
        let source_path = entry.path();
        let destination_path = destination_directory.join(entry.file_name());
        if source_path.is_dir() {
            copy_directory_tree(&source_path, &destination_path);
            continue;
        }
        if source_path.is_file() {
            fs::copy(&source_path, &destination_path).unwrap();
        }
    }
}

fn substitute_placeholders(
    template: &str,
    run_output_directory: &Path,
    working_input_directory: &Path,
) -> String {
    template
        .replace(
            "${TMP_OUTPUT_DIR}",
            run_output_directory.to_string_lossy().as_ref(),
        )
        .replace(
            "${INPUT_DIR}",
            working_input_directory.to_string_lossy().as_ref(),
        )
}

fn normalize_process_output(value: &str) -> String {
    value.strip_suffix('\n').unwrap_or(value).to_string()
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
