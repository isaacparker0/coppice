use std::collections::{HashMap, HashSet};
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

    let run_command_name = run_block.command_name.as_str();

    if run_command_name == "build" {
        let text_run = execute_command(
            compiler,
            &prepared_command_args,
            Some("text"),
            working_input_directory,
            run_output_directory,
            case_path,
            run_number,
        );
        let json_run = execute_command(
            compiler,
            &prepared_command_args,
            Some("json"),
            working_input_directory,
            run_output_directory,
            case_path,
            run_number,
        );

        let expected_text_exit =
            read_expected_exit_code_for_build(case_directory, run_block, case_path, "text");
        let expected_json_exit =
            read_expected_exit_code_for_build(case_directory, run_block, case_path, "json");
        assert_eq!(
            expected_text_exit,
            text_run.exit_code,
            "text exit code mismatch for run {} ({}) in {}",
            run_number,
            run_block.command_line,
            case_path.display()
        );
        assert_eq!(
            expected_json_exit,
            json_run.exit_code,
            "json exit code mismatch for run {} ({}) in {}",
            run_number,
            run_block.command_line,
            case_path.display()
        );

        let expected_text_stdout = read_expected_text_file(
            case_directory,
            run_block,
            case_path,
            run_output_directory,
            working_input_directory,
            run_number,
            "text.stdout",
        );
        let expected_json_stdout = read_expected_text_file(
            case_directory,
            run_block,
            case_path,
            run_output_directory,
            working_input_directory,
            run_number,
            "json.stdout",
        );
        let expected_stderr = read_expected_text_file(
            case_directory,
            run_block,
            case_path,
            run_output_directory,
            working_input_directory,
            run_number,
            "stderr",
        );
        assert_eq!(
            expected_text_stdout,
            text_run.stdout,
            "text stdout mismatch for run {} ({}) in {}",
            run_number,
            run_block.command_line,
            case_path.display()
        );
        assert_eq!(
            expected_json_stdout,
            json_run.stdout,
            "json stdout mismatch for run {} ({}) in {}",
            run_number,
            run_block.command_line,
            case_path.display()
        );
        assert_eq!(
            expected_stderr,
            text_run.stderr,
            "text stderr mismatch for run {} ({}) in {}",
            run_number,
            run_block.command_line,
            case_path.display()
        );
        assert_eq!(
            expected_stderr,
            json_run.stderr,
            "json stderr mismatch for run {} ({}) in {}",
            run_number,
            run_block.command_line,
            case_path.display()
        );
    } else {
        let run_result = execute_command(
            compiler,
            &prepared_command_args,
            None,
            working_input_directory,
            run_output_directory,
            case_path,
            run_number,
        );
        let expected_exit = read_expected_exit_code(case_directory, run_block, case_path);
        assert_eq!(
            expected_exit,
            run_result.exit_code,
            "exit code mismatch for run {} ({}) in {}",
            run_number,
            run_block.command_line,
            case_path.display()
        );
        let expected_stdout = read_expected_text_file(
            case_directory,
            run_block,
            case_path,
            run_output_directory,
            working_input_directory,
            run_number,
            "stdout",
        );
        assert_eq!(
            expected_stdout,
            run_result.stdout,
            "stdout mismatch for run {} ({}) in {}",
            run_number,
            run_block.command_line,
            case_path.display()
        );
        let expected_stderr = read_expected_text_file(
            case_directory,
            run_block,
            case_path,
            run_output_directory,
            working_input_directory,
            run_number,
            "stderr",
        );
        assert_eq!(
            expected_stderr,
            run_result.stderr,
            "stderr mismatch for run {} ({}) in {}",
            run_number,
            run_block.command_line,
            case_path.display()
        );
    }

    if run_command_name == "build" || run_command_name == "run" {
        let expected_artifacts = read_expected_artifact_lines(
            case_directory,
            run_block,
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

struct ExecutedCommand {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

fn execute_command(
    compiler: &Path,
    prepared_command_args: &[String],
    report_format: Option<&str>,
    working_input_directory: &Path,
    run_output_directory: &Path,
    case_path: &Path,
    run_number: usize,
) -> ExecutedCommand {
    let mut final_command_args = prepared_command_args.to_vec();
    if let Some(format) = report_format {
        final_command_args.push("--format".to_string());
        final_command_args.push(format.to_string());
    }
    let substituted_command_args = final_command_args
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
    ExecutedCommand {
        exit_code: output.status.code().unwrap_or(1),
        stdout: normalize_process_output(&String::from_utf8_lossy(&output.stdout)),
        stderr: normalize_process_output(&String::from_utf8_lossy(&output.stderr)),
    }
}

#[derive(Default)]
struct RunBlock {
    label: Option<String>,
    command_line: String,
    command_name: String,
    expectation_stem: String,
}

fn parse_case_script(contents: &str, case_path: &Path) -> Vec<RunBlock> {
    let mut run_blocks = Vec::new();

    for (line_index, raw_line) in contents.lines().enumerate() {
        let line_number = line_index + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (label, command_line) = parse_labeled_run_line(line, case_path, line_number);
        let command_name = parse_command_name(&command_line, case_path, line_number);
        run_blocks.push(RunBlock {
            label,
            command_line,
            command_name,
            ..RunBlock::default()
        });
    }

    assign_expectation_stems(&mut run_blocks, case_path);
    run_blocks
}

fn parse_labeled_run_line(
    line: &str,
    case_path: &Path,
    line_number: usize,
) -> (Option<String>, String) {
    if let Some(after_open_bracket) = line.strip_prefix('[') {
        let (label, command_line) = after_open_bracket.split_once(']').unwrap_or_else(|| {
            panic!(
                "invalid label syntax at {}:{} (missing closing ']')",
                case_path.display(),
                line_number
            )
        });
        assert!(
            !label.trim().is_empty(),
            "empty label at {}:{}",
            case_path.display(),
            line_number
        );
        assert!(
            label
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_'),
            "invalid label '{}' at {}:{} (labels may only use [A-Za-z0-9_])",
            label,
            case_path.display(),
            line_number
        );
        let command_line = command_line.trim_start();
        assert!(
            !command_line.is_empty(),
            "missing command after label '{}' at {}:{}",
            label,
            case_path.display(),
            line_number
        );
        return (Some(label.to_string()), command_line.to_string());
    }

    (None, line.to_string())
}

fn parse_command_name(command_line: &str, case_path: &Path, line_number: usize) -> String {
    let command_tokens = parse_command_tokens(command_line);
    let command_name = command_tokens.first().map_or_else(
        || {
            panic!(
                "invalid case.test syntax at {}:{}; expected '[label] <command>' or '<command>'",
                case_path.display(),
                line_number
            )
        },
        String::as_str,
    );
    assert!(
        command_name_starts_with_valid_character(command_name)
            && command_name_has_valid_remaining_characters(command_name),
        "invalid case.test syntax at {}:{}; expected '[label] <command>' or '<command>'",
        case_path.display(),
        line_number
    );
    command_name.to_string()
}

fn command_name_starts_with_valid_character(command_name: &str) -> bool {
    command_name
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_lowercase())
}

fn command_name_has_valid_remaining_characters(command_name: &str) -> bool {
    command_name.chars().skip(1).all(|character| {
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || character == '_'
            || character == '-'
    })
}

fn assign_expectation_stems(run_blocks: &mut [RunBlock], case_path: &Path) {
    let mut run_count_by_command: HashMap<String, usize> = HashMap::new();
    for run_block in run_blocks.iter() {
        let run_command_name = run_block.command_name.clone();
        *run_count_by_command.entry(run_command_name).or_insert(0) += 1;
    }

    let mut used_labels_by_command: HashMap<String, HashSet<String>> = HashMap::new();
    for run_block in run_blocks.iter_mut() {
        let run_command_name = run_block.command_name.clone();
        let run_count = run_count_by_command
            .get(&run_command_name)
            .copied()
            .unwrap_or(0);
        if run_count == 1 {
            assert!(
                run_block.label.is_none(),
                "label is not allowed for single '{}' run in {}",
                run_command_name,
                case_path.display()
            );
            run_block.expectation_stem = run_command_name;
            continue;
        }

        let label = run_block.label.clone().unwrap_or_else(|| {
            panic!(
                "label is required because '{}' appears multiple times in {}",
                run_command_name,
                case_path.display()
            )
        });
        let used_labels = used_labels_by_command
            .entry(run_command_name.clone())
            .or_default();
        assert!(
            used_labels.insert(label.clone()),
            "duplicate label '{}' for '{}' runs in {}",
            label,
            run_command_name,
            case_path.display()
        );
        run_block.expectation_stem = label;
    }
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

fn read_expected_text_file(
    case_directory: &Path,
    run_block: &RunBlock,
    case_path: &Path,
    run_output_directory: &Path,
    working_input_directory: &Path,
    run_number: usize,
    expected_suffix: &str,
) -> String {
    let relative_path = expectation_relative_path(&run_block.expectation_stem, expected_suffix);
    let full_path = case_directory.join(&relative_path);
    let raw_contents = fs::read_to_string(&full_path).unwrap_or_else(|error| {
        panic!(
            "failed to read {} expectation '{}' for run {} in {}: {error}",
            expected_suffix,
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
    run_block: &RunBlock,
    case_path: &Path,
    run_output_directory: &Path,
    working_input_directory: &Path,
    run_number: usize,
) -> Vec<String> {
    let relative_path = expectation_relative_path(&run_block.expectation_stem, "artifacts");
    let full_path = case_directory.join(&relative_path);
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

fn read_expected_exit_code_for_build(
    case_directory: &Path,
    run_block: &RunBlock,
    case_path: &Path,
    report_format: &str,
) -> i32 {
    let format_specific_suffix = format!("{report_format}.exit");
    read_optional_expected_exit_code(
        case_directory,
        run_block,
        case_path,
        &format_specific_suffix,
    )
    .unwrap_or_else(|| read_expected_exit_code(case_directory, run_block, case_path))
}

fn read_expected_exit_code(case_directory: &Path, run_block: &RunBlock, case_path: &Path) -> i32 {
    let relative_path = expectation_relative_path(&run_block.expectation_stem, "exit");
    let full_path = case_directory.join(&relative_path);
    assert!(
        full_path.is_file(),
        "missing required exit expectation '{}' in {}",
        relative_path.display(),
        case_path.display()
    );
    let raw_exit_code = fs::read_to_string(&full_path).unwrap_or_else(|error| {
        panic!(
            "failed to read exit expectation '{}' in {}: {error}",
            relative_path.display(),
            case_path.display()
        )
    });
    let exit_code_text = raw_exit_code.trim();
    exit_code_text.parse::<i32>().unwrap_or_else(|error| {
        panic!(
            "invalid exit code '{}' in {} for {}: {error}",
            exit_code_text,
            relative_path.display(),
            case_path.display()
        )
    })
}

fn read_optional_expected_exit_code(
    case_directory: &Path,
    run_block: &RunBlock,
    case_path: &Path,
    expected_suffix: &str,
) -> Option<i32> {
    let relative_path = expectation_relative_path(&run_block.expectation_stem, expected_suffix);
    let full_path = case_directory.join(&relative_path);
    if !full_path.is_file() {
        return None;
    }
    let raw_exit_code = fs::read_to_string(&full_path).unwrap_or_else(|error| {
        panic!(
            "failed to read exit expectation '{}' in {}: {error}",
            relative_path.display(),
            case_path.display()
        )
    });
    let exit_code_text = raw_exit_code.trim();
    Some(exit_code_text.parse::<i32>().unwrap_or_else(|error| {
        panic!(
            "invalid exit code '{}' in {} for {}: {error}",
            exit_code_text,
            relative_path.display(),
            case_path.display()
        )
    }))
}

fn expectation_relative_path(expectation_stem: &str, expected_suffix: &str) -> PathBuf {
    PathBuf::from("expect").join(format!("{expectation_stem}.{expected_suffix}"))
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
