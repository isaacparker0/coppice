use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use runfiles::{Runfiles, rlocation};

#[test]
fn diagnostics_cases() {
    let runfiles = Runfiles::create().unwrap();
    let compiler = rlocation!(runfiles, "_main/compiler/cli/main").unwrap();
    let runfiles_root = runfiles::find_runfiles_dir().unwrap();
    let cases_root = runfiles_root.join("_main/tests/diagnostics");

    let mut cases = Vec::new();
    collect_cases(&cases_root, &mut cases);
    assert!(!cases.is_empty(), "no diagnostics cases found");

    for case_root in cases {
        run_case(&compiler, &case_root);
    }
}

fn collect_cases(dir: &Path, cases: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            if path.join("expect.txt").is_file() {
                cases.push(path);
            } else {
                collect_cases(&path, cases);
            }
        }
    }
}

fn run_case(compiler: &Path, case_root: &Path) {
    let expect_path = case_root.join("expect.txt");
    let expect_contents = fs::read_to_string(&expect_path).unwrap();
    let (expected_exit, expected_output) = parse_expect(&expect_contents);

    let output = Command::new(compiler)
        .arg("check")
        // Currently all fixture tests are single file, so we hardcode the
        // assumption of `input/main.lang0`. In the future when we support
        // multi-file fixtures this will need to change.
        .arg("input/main.lang0")
        .current_dir(case_root)
        .output()
        .unwrap();

    let mut combined = String::new();
    combined.push_str(&String::from_utf8_lossy(&output.stdout));
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    if combined.ends_with('\n') {
        combined.pop();
    }

    let actual_exit = output.status.code().unwrap_or(1);
    assert_eq!(
        expected_exit,
        actual_exit,
        "exit code mismatch for {}",
        case_root.display()
    );
    assert_eq!(
        expected_output,
        combined,
        "output mismatch for {}",
        case_root.display()
    );
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
