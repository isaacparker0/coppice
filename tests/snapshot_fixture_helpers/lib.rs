use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub enum SnapshotFixtureRunMode {
    Check,
    Update { workspace_directory: PathBuf },
}

#[must_use]
pub fn snapshot_fixture_run_mode_from_environment() -> SnapshotFixtureRunMode {
    if env::var("UPDATE_SNAPSHOTS").is_ok() {
        let workspace_directory = env::var("BUILD_WORKSPACE_DIRECTORY").unwrap();
        SnapshotFixtureRunMode::Update {
            workspace_directory: PathBuf::from(workspace_directory),
        }
    } else {
        SnapshotFixtureRunMode::Check
    }
}

pub fn collect_snapshot_fixture_case_paths(
    root_directory: &Path,
    runfiles_directory: &Path,
    case_marker_file_name: &str,
    case_paths: &mut Vec<PathBuf>,
) {
    for entry in fs::read_dir(root_directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            if path.join(case_marker_file_name).is_file() {
                let case_path = path.strip_prefix(runfiles_directory).unwrap();
                case_paths.push(case_path.to_path_buf());
            } else {
                collect_snapshot_fixture_case_paths(
                    &path,
                    runfiles_directory,
                    case_marker_file_name,
                    case_paths,
                );
            }
        }
    }
}

#[must_use]
pub fn read_snapshot_fixture_file(path: &Path, case_path: &Path, file_name: &str) -> String {
    let raw_contents = fs::read_to_string(path).unwrap_or_else(|error| {
        panic!(
            "failed to read {} for case {}: {}",
            file_name,
            case_path.display(),
            error
        )
    });
    if raw_contents.is_empty() {
        return String::new();
    }
    assert!(
        raw_contents.ends_with('\n'),
        "{} must end with a trailing newline for case {}",
        file_name,
        case_path.display()
    );
    let contents = raw_contents.strip_suffix('\n').unwrap();
    assert!(
        !contents.is_empty(),
        "{} must be empty (no newline) or non-empty text ending with a trailing newline for case {}",
        file_name,
        case_path.display()
    );
    contents.to_string()
}

pub fn write_snapshot_fixture_file_if_changed(path: &Path, content: &str, case_path: &Path) {
    let canonical_content = if content.is_empty() {
        String::new()
    } else {
        format!("{content}\n")
    };
    let existing_contents = fs::read_to_string(path).unwrap_or_default();
    if existing_contents != canonical_content {
        fs::write(path, canonical_content).unwrap();
        println!("updated: {}", case_path.display());
    }
}

#[must_use]
pub fn normalize_snapshot_fixture_process_output(value: &str) -> String {
    value.strip_suffix('\n').unwrap_or(value).to_string()
}
