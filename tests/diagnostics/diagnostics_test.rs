use std::path::Path;
use std::process::Command;

use compiler__reports::ReportFormat;
use runfiles::{Runfiles, rlocation};
use serde_json::Value;
use tests__snapshot_fixture_helpers::{
    SnapshotFixtureRunMode, collect_snapshot_fixture_case_paths,
    normalize_snapshot_fixture_process_output, read_snapshot_fixture_file,
    snapshot_fixture_run_mode_from_environment, write_snapshot_fixture_file_if_changed,
};

#[test]
fn diagnostics_cases() {
    let runfiles = Runfiles::create().unwrap();
    let compiler = rlocation!(runfiles, "_main/compiler/cli/main").unwrap();
    let runfiles_directory = runfiles::find_runfiles_dir().unwrap().join("_main");

    let mode = snapshot_fixture_run_mode_from_environment();

    let mut case_paths = Vec::new();
    collect_snapshot_fixture_case_paths(
        &runfiles_directory.join("tests/diagnostics"),
        &runfiles_directory,
        "expect.text",
        &mut case_paths,
    );
    assert!(!case_paths.is_empty(), "no diagnostics cases found");

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
    let input_directory = case_directory.join("input");
    assert!(
        input_directory.is_dir(),
        "missing input directory for diagnostics case: {}",
        case_path.display()
    );
    let text_run = run_build(compiler, &input_directory, ReportFormat::Text);
    let json_run = run_build(compiler, &input_directory, ReportFormat::Json);
    let expected_exit = expected_exit_code(case_path);
    match mode {
        SnapshotFixtureRunMode::Update {
            workspace_directory,
        } => {
            let source_expect_text = workspace_directory.join(case_path).join("expect.text");
            write_snapshot_fixture_file_if_changed(
                &source_expect_text,
                format!("{}\n", text_run.output),
                case_path,
            );

            let source_expect_json = workspace_directory.join(case_path).join("expect.json");
            write_snapshot_fixture_file_if_changed(
                &source_expect_json,
                format!("{}\n", json_run.output),
                case_path,
            );
        }
        SnapshotFixtureRunMode::Check => {
            let expect_text_path = runfiles_directory.join(case_path).join("expect.text");
            let expect_json_path = runfiles_directory.join(case_path).join("expect.json");
            assert!(
                expect_text_path.is_file(),
                "missing expect.text for diagnostics case: {}",
                case_path.display()
            );
            assert!(
                expect_json_path.is_file(),
                "missing expect.json for diagnostics case: {}",
                case_path.display()
            );
            let expected_text =
                read_snapshot_fixture_file(&expect_text_path, case_path, "expect.text");
            let expected_json =
                read_snapshot_fixture_file(&expect_json_path, case_path, "expect.json");
            let expected_json_value: Value =
                serde_json::from_str(&expected_json).unwrap_or_else(|error| {
                    panic!("invalid expected JSON for {}: {error}", case_path.display())
                });
            let actual_json_value: Value =
                serde_json::from_str(&json_run.output).unwrap_or_else(|error| {
                    panic!("invalid actual JSON for {}: {error}", case_path.display())
                });

            assert_exit_and_case_naming(case_path, expected_exit, text_run.exit_code, "text");
            assert_exit_and_case_naming(case_path, expected_exit, json_run.exit_code, "json");
            assert_single_error_diagnostic(case_path, expected_exit, &text_run.output);
            assert_json_output_contract(case_path, expected_exit, &json_run.output);
            assert_eq!(
                expected_text,
                text_run.output,
                "text output mismatch for {}\n\n\
                 To update: UPDATE_SNAPSHOTS=1 bazel run //tests/diagnostics:diagnostics_test",
                case_path.display()
            );
            assert_eq!(
                expected_json_value,
                actual_json_value,
                "json output mismatch for {}\n\n\
                 To update: UPDATE_SNAPSHOTS=1 bazel run //tests/diagnostics:diagnostics_test",
                case_path.display()
            );
        }
    }
}

struct CheckRun {
    exit_code: i32,
    output: String,
}

fn run_build(compiler: &Path, input_directory: &Path, format: ReportFormat) -> CheckRun {
    let output = Command::new(compiler)
        .arg("build")
        .arg("--format")
        .arg(format.as_str())
        .current_dir(input_directory)
        .output()
        .unwrap();

    let mut combined_output = String::new();
    combined_output.push_str(&String::from_utf8_lossy(&output.stdout));
    combined_output.push_str(&String::from_utf8_lossy(&output.stderr));
    CheckRun {
        exit_code: output.status.code().unwrap_or(1),
        output: normalize_snapshot_fixture_process_output(&combined_output),
    }
}

fn expected_exit_code(case_path: &Path) -> i32 {
    let case_name = case_path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("case path must end with a valid UTF-8 directory name");
    i32::from(case_name != "minimal_valid")
}

fn assert_exit_and_case_naming(
    case_path: &Path,
    expected_exit: i32,
    actual_exit: i32,
    format: &str,
) {
    assert_eq!(
        expected_exit,
        actual_exit,
        "{format} exit code mismatch for {}\n\n\
         To update: UPDATE_SNAPSHOTS=1 bazel run //tests/diagnostics:diagnostics_test",
        case_path.display()
    );
}

fn assert_single_error_diagnostic(case_path: &Path, exit_code: i32, output: &str) {
    if exit_code == 0 {
        return;
    }

    let error_count = output
        .lines()
        .filter(|line| line.contains(": error:"))
        .count();
    assert_eq!(
        1,
        error_count,
        "error fixtures must contain exactly one ': error:' diagnostic in expect.text: {}",
        case_path.display()
    );
}

fn assert_json_output_contract(case_path: &Path, exit_code: i32, output: &str) {
    let value: Value = serde_json::from_str(output)
        .unwrap_or_else(|error| panic!("invalid JSON output for {}: {error}", case_path.display()));

    let ok = value
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("JSON output missing boolean 'ok': {}", case_path.display()));
    let diagnostics = value
        .get("diagnostics")
        .and_then(Value::as_array)
        .unwrap_or_else(|| {
            panic!(
                "JSON output missing array 'diagnostics': {}",
                case_path.display()
            )
        });
    let error = value.get("error");

    if exit_code == 0 {
        assert!(
            ok,
            "success fixtures must have ok=true in expect.json: {}",
            case_path.display()
        );
        assert!(
            diagnostics.is_empty(),
            "success fixtures must have empty diagnostics in expect.json: {}",
            case_path.display()
        );
        assert!(
            error.is_none(),
            "success fixtures must not contain error in expect.json: {}",
            case_path.display()
        );
        return;
    }

    assert!(
        !ok,
        "error fixtures must have ok=false in expect.json: {}",
        case_path.display()
    );
    let has_error = error.is_some_and(|value| !value.is_null());
    let has_diagnostics = !diagnostics.is_empty();
    assert!(
        has_error || has_diagnostics,
        "error fixtures must include diagnostics or error in expect.json: {}",
        case_path.display()
    );
    assert!(
        !(has_error && has_diagnostics),
        "error fixtures must not include both diagnostics and error in expect.json: {}",
        case_path.display()
    );
    if has_diagnostics {
        assert_eq!(
            1,
            diagnostics.len(),
            "error fixtures with diagnostics must contain exactly one diagnostic in expect.json: {}",
            case_path.display()
        );
    }
}
