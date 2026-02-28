use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use runfiles::{Runfiles, rlocation};
use tests__snapshot_fixture_helpers::{
    SnapshotFixtureRunMode, collect_snapshot_fixture_case_paths,
    snapshot_fixture_run_mode_from_environment, write_snapshot_fixture_file_if_changed,
};

#[test]
fn fixture_cases() {
    let runfiles = Runfiles::create().unwrap();
    let compiler = rlocation!(runfiles, "_main/compiler/cli/main").unwrap();
    let runfiles_directory = runfiles::find_runfiles_dir().unwrap().join("_main");
    let mode = snapshot_fixture_run_mode_from_environment();

    let mut case_paths = Vec::new();
    collect_snapshot_fixture_case_paths(
        &runfiles_directory.join("unified_tests"),
        &runfiles_directory,
        "case.test",
        &mut case_paths,
    );
    case_paths.sort();
    assert!(!case_paths.is_empty(), "no fixture cases found");

    for case_path in &case_paths {
        run_case(&compiler, &runfiles_directory, case_path, &mode);
    }
}

fn run_case(
    compiler: &Path,
    runfiles_directory: &Path,
    case_path: &Path,
    mode: &SnapshotFixtureRunMode,
) {
    let case_directory = runfiles_directory.join(case_path);
    assert_valid_case_slug(case_path);
    let input_directory = case_directory.join("input");
    assert!(
        input_directory.is_dir(),
        "missing input directory for case {}",
        case_path.display()
    );
    assert_valid_case_contract_readme(&case_directory, case_path);

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
        "coppice_fixture_case_{}_{}",
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
            mode,
        );
    }

    let _ = fs::remove_dir_all(&temp_case_directory);
}

fn assert_valid_case_slug(case_path: &Path) {
    let case_slug = case_path
        .file_name()
        .and_then(|segment| segment.to_str())
        .unwrap_or_else(|| panic!("invalid fixture case path {}", case_path.display()));
    if case_slug.contains("minimal_valid") {
        assert!(
            case_slug == "minimal_valid",
            "fixture case '{}' uses forbidden minimal_valid variant; use exactly 'minimal_valid' or a factual scenario name",
            case_path.display()
        );
    }
}

fn run_block_and_assert(
    compiler: &Path,
    case_directory: &Path,
    case_path: &Path,
    working_input_directory: &Path,
    run_output_directory: &Path,
    run_block: &RunBlock,
    run_number: usize,
    mode: &SnapshotFixtureRunMode,
) {
    let command_args = parse_command_tokens(&run_block.command_line);
    assert!(
        !command_args.is_empty(),
        "empty command in run {} for {}",
        run_number,
        case_path.display()
    );
    let run_command = parse_run_command(&run_block.command_name, case_path, run_number);
    let prepared_command_args = prepare_command_args_for_execution(run_command, &command_args);
    let run_actual = execute_run_and_collect_actual_outputs(
        compiler,
        run_command,
        &prepared_command_args,
        working_input_directory,
        run_output_directory,
        case_path,
        run_number,
    );

    match mode {
        SnapshotFixtureRunMode::Check => {
            assert_expected_outputs_match(
                case_directory,
                case_path,
                working_input_directory,
                run_output_directory,
                run_block,
                run_number,
                run_command,
                &run_actual,
            );
        }
        SnapshotFixtureRunMode::Update {
            workspace_directory,
        } => {
            update_expected_outputs(
                &workspace_directory.join(case_path),
                case_path,
                working_input_directory,
                run_output_directory,
                run_block,
                run_command,
                &run_actual,
            );
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum RunCommand {
    Build,
    Run,
    Fix,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum OutputKind {
    Exit,
    Stdout,
    Stderr,
    Artifacts,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum OutputFormat {
    None,
    Text,
    Json,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct OutputKey {
    kind: OutputKind,
    format: OutputFormat,
}

enum OutputValue {
    ExitCode(i32),
    Text(String),
    ArtifactPaths(Vec<String>),
}

struct RunActual {
    value_by_output_key: HashMap<OutputKey, OutputValue>,
}

fn parse_run_command(command_name: &str, case_path: &Path, run_number: usize) -> RunCommand {
    match command_name {
        "build" => RunCommand::Build,
        "run" => RunCommand::Run,
        "fix" => RunCommand::Fix,
        _ => panic!(
            "unsupported command '{}' in run {} for {}; expected one of: build, run, fix",
            command_name,
            run_number,
            case_path.display()
        ),
    }
}

fn prepare_command_args_for_execution(
    run_command: RunCommand,
    command_args: &[String],
) -> Vec<String> {
    if run_command != RunCommand::Build && run_command != RunCommand::Run {
        return command_args.to_vec();
    }
    let mut prepared = command_args.to_vec();
    prepared.push("--output-dir".to_string());
    prepared.push("${TMP_OUTPUT_DIR}".to_string());
    prepared
}

fn execute_run_and_collect_actual_outputs(
    compiler: &Path,
    run_command: RunCommand,
    prepared_command_args: &[String],
    working_input_directory: &Path,
    run_output_directory: &Path,
    case_path: &Path,
    run_number: usize,
) -> RunActual {
    let mut value_by_output_key = HashMap::new();

    if run_command == RunCommand::Build {
        let text_run = execute_command(
            compiler,
            prepared_command_args,
            Some("text"),
            working_input_directory,
            run_output_directory,
            case_path,
            run_number,
        );
        let json_run = execute_command(
            compiler,
            prepared_command_args,
            Some("json"),
            working_input_directory,
            run_output_directory,
            case_path,
            run_number,
        );
        assert_eq!(
            text_run.stderr,
            json_run.stderr,
            "build stderr differs between text and json for run {} in {}",
            run_number,
            case_path.display()
        );
        value_by_output_key.insert(
            OutputKey {
                kind: OutputKind::Exit,
                format: OutputFormat::Text,
            },
            OutputValue::ExitCode(text_run.exit_code),
        );
        value_by_output_key.insert(
            OutputKey {
                kind: OutputKind::Exit,
                format: OutputFormat::Json,
            },
            OutputValue::ExitCode(json_run.exit_code),
        );
        value_by_output_key.insert(
            OutputKey {
                kind: OutputKind::Exit,
                format: OutputFormat::None,
            },
            OutputValue::ExitCode(text_run.exit_code),
        );
        value_by_output_key.insert(
            OutputKey {
                kind: OutputKind::Stdout,
                format: OutputFormat::Text,
            },
            OutputValue::Text(text_run.stdout),
        );
        value_by_output_key.insert(
            OutputKey {
                kind: OutputKind::Stdout,
                format: OutputFormat::Json,
            },
            OutputValue::Text(json_run.stdout),
        );
        value_by_output_key.insert(
            OutputKey {
                kind: OutputKind::Stderr,
                format: OutputFormat::None,
            },
            OutputValue::Text(text_run.stderr),
        );
    } else {
        let run_result = execute_command(
            compiler,
            prepared_command_args,
            None,
            working_input_directory,
            run_output_directory,
            case_path,
            run_number,
        );
        value_by_output_key.insert(
            OutputKey {
                kind: OutputKind::Exit,
                format: OutputFormat::None,
            },
            OutputValue::ExitCode(run_result.exit_code),
        );
        value_by_output_key.insert(
            OutputKey {
                kind: OutputKind::Stdout,
                format: OutputFormat::None,
            },
            OutputValue::Text(run_result.stdout),
        );
        value_by_output_key.insert(
            OutputKey {
                kind: OutputKind::Stderr,
                format: OutputFormat::None,
            },
            OutputValue::Text(run_result.stderr),
        );
    }

    if run_command_has_artifacts(run_command) {
        value_by_output_key.insert(
            OutputKey {
                kind: OutputKind::Artifacts,
                format: OutputFormat::None,
            },
            OutputValue::ArtifactPaths(collect_artifact_paths(run_output_directory)),
        );
    }

    RunActual {
        value_by_output_key,
    }
}

fn run_command_has_artifacts(run_command: RunCommand) -> bool {
    run_command == RunCommand::Build || run_command == RunCommand::Run
}

fn output_keys_for_check(run_command: RunCommand) -> Vec<OutputKey> {
    match run_command {
        RunCommand::Build => vec![
            OutputKey {
                kind: OutputKind::Exit,
                format: OutputFormat::Text,
            },
            OutputKey {
                kind: OutputKind::Exit,
                format: OutputFormat::Json,
            },
            OutputKey {
                kind: OutputKind::Stdout,
                format: OutputFormat::Text,
            },
            OutputKey {
                kind: OutputKind::Stdout,
                format: OutputFormat::Json,
            },
            OutputKey {
                kind: OutputKind::Stderr,
                format: OutputFormat::None,
            },
            OutputKey {
                kind: OutputKind::Artifacts,
                format: OutputFormat::None,
            },
        ],
        RunCommand::Run => vec![
            OutputKey {
                kind: OutputKind::Exit,
                format: OutputFormat::None,
            },
            OutputKey {
                kind: OutputKind::Stdout,
                format: OutputFormat::None,
            },
            OutputKey {
                kind: OutputKind::Stderr,
                format: OutputFormat::None,
            },
            OutputKey {
                kind: OutputKind::Artifacts,
                format: OutputFormat::None,
            },
        ],
        RunCommand::Fix => vec![
            OutputKey {
                kind: OutputKind::Exit,
                format: OutputFormat::None,
            },
            OutputKey {
                kind: OutputKind::Stdout,
                format: OutputFormat::None,
            },
            OutputKey {
                kind: OutputKind::Stderr,
                format: OutputFormat::None,
            },
        ],
    }
}

fn output_keys_for_update(run_command: RunCommand) -> Vec<OutputKey> {
    match run_command {
        RunCommand::Build => vec![
            OutputKey {
                kind: OutputKind::Exit,
                format: OutputFormat::None,
            },
            OutputKey {
                kind: OutputKind::Stdout,
                format: OutputFormat::Text,
            },
            OutputKey {
                kind: OutputKind::Stdout,
                format: OutputFormat::Json,
            },
            OutputKey {
                kind: OutputKind::Stderr,
                format: OutputFormat::None,
            },
            OutputKey {
                kind: OutputKind::Artifacts,
                format: OutputFormat::None,
            },
        ],
        RunCommand::Run => vec![
            OutputKey {
                kind: OutputKind::Exit,
                format: OutputFormat::None,
            },
            OutputKey {
                kind: OutputKind::Stdout,
                format: OutputFormat::None,
            },
            OutputKey {
                kind: OutputKind::Stderr,
                format: OutputFormat::None,
            },
            OutputKey {
                kind: OutputKind::Artifacts,
                format: OutputFormat::None,
            },
        ],
        RunCommand::Fix => vec![
            OutputKey {
                kind: OutputKind::Exit,
                format: OutputFormat::None,
            },
            OutputKey {
                kind: OutputKind::Stdout,
                format: OutputFormat::None,
            },
            OutputKey {
                kind: OutputKind::Stderr,
                format: OutputFormat::None,
            },
        ],
    }
}

fn assert_expected_outputs_match(
    case_directory: &Path,
    case_path: &Path,
    working_input_directory: &Path,
    run_output_directory: &Path,
    run_block: &RunBlock,
    run_number: usize,
    run_command: RunCommand,
    run_actual: &RunActual,
) {
    for output_key in output_keys_for_check(run_command) {
        let actual_output_value = actual_output_value_for_key(run_actual, output_key);
        match output_key.kind {
            OutputKind::Exit => {
                let expected_exit = if run_command == RunCommand::Build
                    && output_key.format != OutputFormat::None
                {
                    read_expected_exit_code_for_build(
                        case_directory,
                        run_block,
                        case_path,
                        output_format_suffix(output_key.format),
                    )
                } else {
                    read_expected_exit_code(case_directory, run_block, case_path)
                };
                assert_output_value_matches_exit(
                    output_key,
                    expected_exit,
                    actual_output_value,
                    run_number,
                    run_block,
                    case_path,
                );
            }
            OutputKind::Stdout | OutputKind::Stderr => {
                let expected_text = read_expected_text_file(
                    case_directory,
                    run_block,
                    case_path,
                    run_output_directory,
                    working_input_directory,
                    run_number,
                    output_suffix_for_key(output_key),
                );
                assert_output_value_matches_text(
                    output_key,
                    &expected_text,
                    actual_output_value,
                    run_number,
                    run_block,
                    case_path,
                );
            }
            OutputKind::Artifacts => {
                let expected_artifact_paths = read_expected_artifact_lines(
                    case_directory,
                    run_block,
                    case_path,
                    run_output_directory,
                    working_input_directory,
                    run_number,
                );
                assert_output_value_matches_artifacts(
                    &expected_artifact_paths,
                    actual_output_value,
                    run_number,
                    run_block,
                    case_path,
                );
            }
        }
    }
}

fn assert_output_value_matches_exit(
    output_key: OutputKey,
    expected_exit: i32,
    actual_output_value: &OutputValue,
    run_number: usize,
    run_block: &RunBlock,
    case_path: &Path,
) {
    let OutputValue::ExitCode(actual_exit) = actual_output_value else {
        panic!(
            "internal error: expected exit output value for {:?} in run {} ({}) in {}",
            output_key,
            run_number,
            run_block.command_line,
            case_path.display()
        )
    };
    assert_eq!(
        expected_exit,
        *actual_exit,
        "{} mismatch for run {} ({}) in {}",
        output_key_label(output_key),
        run_number,
        run_block.command_line,
        case_path.display()
    );
}

fn assert_output_value_matches_text(
    output_key: OutputKey,
    expected_text: &str,
    actual_output_value: &OutputValue,
    run_number: usize,
    run_block: &RunBlock,
    case_path: &Path,
) {
    let OutputValue::Text(actual_text) = actual_output_value else {
        panic!(
            "internal error: expected text output value for {:?} in run {} ({}) in {}",
            output_key,
            run_number,
            run_block.command_line,
            case_path.display()
        )
    };
    assert_eq!(
        expected_text,
        actual_text,
        "{} mismatch for run {} ({}) in {}",
        output_key_label(output_key),
        run_number,
        run_block.command_line,
        case_path.display()
    );
}

fn assert_output_value_matches_artifacts(
    expected_artifact_paths: &[String],
    actual_output_value: &OutputValue,
    run_number: usize,
    run_block: &RunBlock,
    case_path: &Path,
) {
    let OutputValue::ArtifactPaths(actual_artifact_paths) = actual_output_value else {
        panic!(
            "internal error: expected artifact output value in run {} ({}) in {}",
            run_number,
            run_block.command_line,
            case_path.display()
        )
    };
    assert_eq!(
        expected_artifact_paths,
        actual_artifact_paths,
        "artifact list mismatch for run {} ({}) in {}",
        run_number,
        run_block.command_line,
        case_path.display()
    );
}

fn update_expected_outputs(
    source_case_directory: &Path,
    case_path: &Path,
    working_input_directory: &Path,
    run_output_directory: &Path,
    run_block: &RunBlock,
    run_command: RunCommand,
    run_actual: &RunActual,
) {
    for output_key in output_keys_for_update(run_command) {
        let actual_output_value = actual_output_value_for_key(run_actual, output_key);
        match actual_output_value {
            OutputValue::ExitCode(exit_code) => {
                write_expected_exit_file(
                    source_case_directory,
                    run_block,
                    case_path,
                    output_suffix_for_key(output_key),
                    *exit_code,
                );
            }
            OutputValue::Text(text) => {
                let normalized_output = normalize_output_for_snapshot(
                    text,
                    run_output_directory,
                    working_input_directory,
                );
                write_expected_text_file(
                    source_case_directory,
                    run_block,
                    case_path,
                    output_suffix_for_key(output_key),
                    &normalized_output,
                );
            }
            OutputValue::ArtifactPaths(artifact_paths) => {
                let artifact_placeholders =
                    collect_artifact_placeholders(artifact_paths, run_output_directory);
                write_expected_artifacts_file(
                    source_case_directory,
                    run_block,
                    case_path,
                    &artifact_placeholders,
                );
            }
        }
    }
}

fn output_suffix_for_key(output_key: OutputKey) -> &'static str {
    match (output_key.kind, output_key.format) {
        (OutputKind::Exit, OutputFormat::None) => "exit",
        (OutputKind::Exit, OutputFormat::Text) => "text.exit",
        (OutputKind::Exit, OutputFormat::Json) => "json.exit",
        (OutputKind::Stdout, OutputFormat::None) => "stdout",
        (OutputKind::Stdout, OutputFormat::Text) => "text.stdout",
        (OutputKind::Stdout, OutputFormat::Json) => "json.stdout",
        (OutputKind::Stderr, OutputFormat::None) => "stderr",
        (OutputKind::Artifacts, OutputFormat::None) => "artifacts",
        _ => panic!("invalid output key combination for expectation suffix: {output_key:?}"),
    }
}

fn output_key_label(output_key: OutputKey) -> &'static str {
    match (output_key.kind, output_key.format) {
        (OutputKind::Exit, OutputFormat::None) => "exit",
        (OutputKind::Exit, OutputFormat::Text) => "text exit code",
        (OutputKind::Exit, OutputFormat::Json) => "json exit code",
        (OutputKind::Stdout, OutputFormat::None) => "stdout",
        (OutputKind::Stdout, OutputFormat::Text) => "text stdout",
        (OutputKind::Stdout, OutputFormat::Json) => "json stdout",
        (OutputKind::Stderr, OutputFormat::None) => "stderr",
        (OutputKind::Artifacts, OutputFormat::None) => "artifact list",
        _ => "output",
    }
}

fn output_format_suffix(output_format: OutputFormat) -> &'static str {
    match output_format {
        OutputFormat::Text => "text",
        OutputFormat::Json => "json",
        OutputFormat::None => {
            panic!("output format suffix is only valid for text/json formats")
        }
    }
}

fn actual_output_value_for_key(run_actual: &RunActual, output_key: OutputKey) -> &OutputValue {
    run_actual
        .value_by_output_key
        .get(&output_key)
        .unwrap_or_else(|| panic!("missing actual output value for key {output_key:?}"))
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
    let contents = if raw_contents.is_empty() {
        ""
    } else {
        assert!(
            raw_contents.ends_with('\n'),
            "{} expectation '{}' must end with a trailing newline in {}",
            expected_suffix,
            relative_path.display(),
            case_path.display()
        );
        let stripped = raw_contents.strip_suffix('\n').unwrap();
        assert!(
            !stripped.is_empty(),
            "{} expectation '{}' must be empty (no newline) or non-empty text ending with a trailing newline in {}",
            expected_suffix,
            relative_path.display(),
            case_path.display()
        );
        stripped
    };
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

fn write_expected_text_file(
    source_case_directory: &Path,
    run_block: &RunBlock,
    case_path: &Path,
    expected_suffix: &str,
    content: &str,
) {
    let relative_path = expectation_relative_path(&run_block.expectation_stem, expected_suffix);
    let full_path = source_case_directory.join(&relative_path);
    write_snapshot_fixture_file_if_changed(&full_path, content, case_path);
}

fn write_expected_exit_file(
    source_case_directory: &Path,
    run_block: &RunBlock,
    case_path: &Path,
    expected_suffix: &str,
    exit_code: i32,
) {
    write_expected_text_file(
        source_case_directory,
        run_block,
        case_path,
        expected_suffix,
        &exit_code.to_string(),
    );
}

fn write_expected_artifacts_file(
    source_case_directory: &Path,
    run_block: &RunBlock,
    case_path: &Path,
    artifact_paths: &[String],
) {
    let relative_path = expectation_relative_path(&run_block.expectation_stem, "artifacts");
    let full_path = source_case_directory.join(&relative_path);
    let content = artifact_paths.join("\n");
    write_snapshot_fixture_file_if_changed(&full_path, &content, case_path);
}

fn collect_artifact_placeholders(
    artifact_paths: &[String],
    run_output_directory: &Path,
) -> Vec<String> {
    let mut artifact_placeholders = Vec::new();
    for artifact_path in artifact_paths {
        let relative_path = Path::new(artifact_path)
            .strip_prefix(run_output_directory)
            .unwrap();
        artifact_placeholders.push(format!(
            "${{TMP_OUTPUT_DIR}}/{}",
            relative_path.to_string_lossy()
        ));
    }
    artifact_placeholders
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

fn normalize_output_for_snapshot(
    value: &str,
    run_output_directory: &Path,
    working_input_directory: &Path,
) -> String {
    value
        .replace(
            run_output_directory.to_string_lossy().as_ref(),
            "${TMP_OUTPUT_DIR}",
        )
        .replace(
            working_input_directory.to_string_lossy().as_ref(),
            "${INPUT_DIR}",
        )
}

fn assert_valid_case_contract_readme(case_directory: &Path, case_path: &Path) {
    let readme_path = case_directory.join("README.md");
    assert!(
        readme_path.is_file(),
        "missing README.md for fixture case {}",
        case_path.display()
    );
    let readme_contents = fs::read_to_string(&readme_path).unwrap_or_else(|error| {
        panic!(
            "failed to read README.md for fixture case {}: {error}",
            case_path.display()
        )
    });
    let readme_lines = readme_contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    assert!(
        !readme_lines.is_empty(),
        "README.md must contain exactly one sentence for fixture case {}",
        case_path.display()
    );
    for line in &readme_lines {
        assert!(
            !line.starts_with('#')
                && !line.starts_with('-')
                && !line.starts_with('*')
                && !line.starts_with("```")
                && !line
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_ascii_digit())
                && !line.contains('`'),
            "README.md must contain a plain one-sentence contract (no headings/lists/code) for fixture case {}",
            case_path.display()
        );
    }
    let sentence = readme_lines.join(" ");
    assert!(
        sentence.ends_with('.'),
        "README.md contract sentence must end with '.' for fixture case {}",
        case_path.display()
    );
    let sentence_without_period = sentence.strip_suffix('.').unwrap();
    assert!(
        !sentence_without_period.contains('.'),
        "README.md must contain exactly one sentence for fixture case {}",
        case_path.display()
    );
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
