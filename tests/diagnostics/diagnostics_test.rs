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
fn diagnostics_cases() {
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
        &runfiles_directory.join("tests/diagnostics"),
        &runfiles_directory,
        &mut case_paths,
    );
    assert!(!case_paths.is_empty(), "no diagnostics cases found");

    for case_path in &case_paths {
        run_case(&compiler, &runfiles_directory, case_path, &mode);
    }
}

fn collect_cases(dir: &Path, runfiles_directory: &Path, case_paths: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            if path.join("expect.txt").is_file() {
                let case_path = path.strip_prefix(runfiles_directory).unwrap();
                case_paths.push(case_path.to_path_buf());
            } else {
                collect_cases(&path, runfiles_directory, case_paths);
            }
        }
    }
}

fn run_case(compiler: &Path, runfiles_directory: &Path, case_path: &Path, mode: &Mode) {
    let output = Command::new(compiler)
        .arg("check")
        // Currently all fixture tests are single file, so we hardcode the
        // assumption of `input/main.coppice`. In the future when we support
        // multi-file fixtures this will need to change.
        .arg("input/main.coppice")
        .current_dir(runfiles_directory.join(case_path))
        .output()
        .unwrap();

    let mut combined = String::new();
    combined.push_str(&String::from_utf8_lossy(&output.stdout));
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    if combined.ends_with('\n') {
        combined.pop();
    }

    let actual_exit = output.status.code().unwrap_or(1);
    match mode {
        Mode::Update {
            workspace_directory,
        } => {
            let actual_expect = format!("# exit: {actual_exit}\n{combined}\n");
            let source_expect = workspace_directory.join(case_path).join("expect.txt");
            let existing = fs::read_to_string(&source_expect).unwrap_or_default();
            if existing != actual_expect {
                fs::write(&source_expect, &actual_expect).unwrap();
                println!("updated: {}", case_path.display());
            }
        }
        Mode::Check => {
            let expect_contents =
                fs::read_to_string(runfiles_directory.join(case_path).join("expect.txt")).unwrap();
            let (expected_exit, expected_output) = parse_expect(&expect_contents);
            assert_case_name_matches_exit(case_path, expected_exit);
            assert_eq!(
                expected_exit,
                actual_exit,
                "exit code mismatch for {}\n\n\
                 To update: UPDATE_SNAPSHOTS=1 bazel run //tests/diagnostics:diagnostics_test",
                case_path.display()
            );
            assert_eq!(
                expected_output,
                combined,
                "output mismatch for {}\n\n\
                 To update: UPDATE_SNAPSHOTS=1 bazel run //tests/diagnostics:diagnostics_test",
                case_path.display()
            );
        }
    }
}

fn assert_case_name_matches_exit(case_path: &Path, exit_code: i32) {
    let case_name = case_path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("case path must end with a valid UTF-8 directory name");
    if exit_code == 0 {
        assert_eq!(
            case_name,
            "minimal_valid",
            "success fixtures must be named 'minimal_valid': {}",
            case_path.display()
        );
    } else {
        assert_ne!(
            case_name,
            "minimal_valid",
            "error fixtures must not be named 'minimal_valid': {}",
            case_path.display()
        );
    }
}

fn parse_expect(contents: &str) -> (i32, String) {
    let mut lines = contents.lines();
    let header = lines.next().unwrap();
    let expected_exit = header
        .strip_prefix("# exit: ")
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();

    let mut remainder = String::new();
    if let Some((_, rest)) = contents.split_once('\n') {
        remainder.push_str(rest);
    }
    if remainder.ends_with('\n') {
        remainder.pop();
    }
    (expected_exit, remainder)
}
