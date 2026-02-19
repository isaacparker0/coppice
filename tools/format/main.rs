use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use clap::{Parser, Subcommand};
use runfiles::Runfiles;

#[derive(Parser)]
#[command(version)]
struct CommandLine {
    #[command(subcommand)]
    command: Option<Mode>,
}

#[derive(Copy, Clone, Subcommand)]
enum Mode {
    /// Check formatting without modifying files.
    Check,
    /// Format files in place.
    Fix,
}

#[derive(Copy, Clone)]
enum Tool {
    Deno,
    Gofmt,
    Rustfmt,
    Shfmt,
    Buildifier,
    Taplo,
    KeepSorted,
}

enum FileSelector {
    Extensions(&'static [&'static str]),
    AllFiles,
}

struct Formatter {
    name: &'static str,
    tool: Tool,
    check_args: &'static [&'static str],
    fix_args: &'static [&'static str],
    selector: FileSelector,
}

struct FormatterInvocation {
    name: &'static str,
    bin: PathBuf,
    args: Vec<String>,
    selector: FileSelector,
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum FormatterOutcome {
    Success,
    Failure,
}

const FORMATTERS: [Formatter; 7] = [
    Formatter {
        name: "JSON/Markdown/HTML/CSS/JS/YAML",
        tool: Tool::Deno,
        check_args: &["fmt", "--check"],
        fix_args: &["fmt"],
        selector: FileSelector::Extensions(&["json", "md", "html", "css", "js", "yaml"]),
    },
    Formatter {
        name: "Go",
        tool: Tool::Gofmt,
        check_args: &["-d"],
        fix_args: &["-w"],
        selector: FileSelector::Extensions(&["go"]),
    },
    Formatter {
        name: "Rust",
        tool: Tool::Rustfmt,
        check_args: &["--check"],
        fix_args: &[],
        selector: FileSelector::Extensions(&["rs"]),
    },
    Formatter {
        name: "Shell",
        tool: Tool::Shfmt,
        check_args: &["-d"],
        fix_args: &["-w"],
        selector: FileSelector::Extensions(&["sh"]),
    },
    Formatter {
        name: "Starlark",
        tool: Tool::Buildifier,
        check_args: &["-lint=off", "-mode=check"],
        fix_args: &["-lint=off", "-mode=fix"],
        selector: FileSelector::Extensions(&["bzl", "bazel"]),
    },
    Formatter {
        name: "TOML",
        tool: Tool::Taplo,
        check_args: &["fmt", "--check"],
        fix_args: &["fmt"],
        selector: FileSelector::Extensions(&["toml"]),
    },
    Formatter {
        name: "keep-sorted",
        tool: Tool::KeepSorted,
        check_args: &["--mode=lint"],
        fix_args: &["--mode=fix"],
        selector: FileSelector::AllFiles,
    },
];

fn main() -> ExitCode {
    let command_line = CommandLine::parse();
    let mode = command_line.command.unwrap_or(Mode::Fix);

    let workspace_directory = std::env::var("BUILD_WORKSPACE_DIRECTORY").unwrap_or_else(|_| {
        eprintln!("error: BUILD_WORKSPACE_DIRECTORY not set. Run via `bazel run`.");
        std::process::exit(1);
    });

    let tools = read_tools_from_build();
    let formatter_invocations: Vec<FormatterInvocation> = FORMATTERS
        .iter()
        .map(|formatter| FormatterInvocation {
            name: formatter.name,
            bin: match formatter.tool {
                Tool::Deno => tools.deno.clone(),
                Tool::Gofmt => tools.gofmt.clone(),
                Tool::Rustfmt => tools.rustfmt.clone(),
                Tool::Shfmt => tools.shfmt.clone(),
                Tool::Buildifier => tools.buildifier.clone(),
                Tool::Taplo => tools.taplo.clone(),
                Tool::KeepSorted => tools.keep_sorted.clone(),
            },
            args: match mode {
                Mode::Check => formatter.check_args,
                Mode::Fix => formatter.fix_args,
            }
            .iter()
            .map(|arg| (*arg).to_string())
            .collect(),
            selector: match formatter.selector {
                FileSelector::Extensions(extensions) => FileSelector::Extensions(extensions),
                FileSelector::AllFiles => FileSelector::AllFiles,
            },
        })
        .collect();

    // Build routing tables for extension-based and all-file formatters.
    let mut formatter_index_by_extension: HashMap<&str, usize> = HashMap::new();
    let mut formatter_indices_for_all_files: Vec<usize> = Vec::new();
    let mut formatter_indices_for_extension_files: Vec<usize> = Vec::new();
    for (index, invocation) in formatter_invocations.iter().enumerate() {
        match invocation.selector {
            FileSelector::Extensions(extensions) => {
                formatter_indices_for_extension_files.push(index);
                for extension in extensions {
                    if let Some(existing_index) =
                        formatter_index_by_extension.insert(extension, index)
                    {
                        eprintln!(
                            "error: extension '.{extension}' is configured for multiple formatters: {} and {}",
                            formatter_invocations[existing_index].name,
                            formatter_invocations[index].name
                        );
                        std::process::exit(1);
                    }
                }
            }
            FileSelector::AllFiles => formatter_indices_for_all_files.push(index),
        }
    }

    if formatter_index_by_extension.is_empty() && formatter_indices_for_all_files.is_empty() {
        return ExitCode::SUCCESS;
    }

    // Single `git ls-files` to discover all tracked + untracked, non-ignored
    // files.
    let git_output = Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--modified",
            "--other",
            "--exclude-standard",
        ])
        .current_dir(&workspace_directory)
        .stdout(Stdio::piped())
        .output()
        .expect("failed to run git ls-files");

    let deleted_output = Command::new("git")
        .args(["ls-files", "--deleted"])
        .current_dir(&workspace_directory)
        .stdout(Stdio::piped())
        .output()
        .expect("failed to run git ls-files --deleted");

    let deleted: std::collections::HashSet<String> =
        String::from_utf8_lossy(&deleted_output.stdout)
            .lines()
            .map(String::from)
            .collect();

    // Partition files into per-formatter file lists.
    let mut files_by_formatter_index: Vec<Vec<String>> = vec![vec![]; formatter_invocations.len()];

    for line in String::from_utf8_lossy(&git_output.stdout).lines() {
        if deleted.contains(line) {
            continue;
        }

        for &index in &formatter_indices_for_all_files {
            files_by_formatter_index[index].push(line.to_string());
        }

        let path = Path::new(line);
        if let Some(extension) = path.extension().and_then(|e| e.to_str())
            && let Some(&formatter_index) = formatter_index_by_extension.get(extension)
        {
            files_by_formatter_index[formatter_index].push(line.to_string());
        }
    }

    // Run all all-file formatters sequentially to avoid concurrent edits.
    let mut failed = false;
    for &index in &formatter_indices_for_all_files {
        let formatter_invocation = &formatter_invocations[index];
        let files = &files_by_formatter_index[index];
        if files.is_empty() {
            continue;
        }

        failed |= run_and_report_formatter(mode, formatter_invocation, files, &workspace_directory)
            == FormatterOutcome::Failure;
    }

    // Run extension-based formatters in parallel.
    let (sender, receiver) = mpsc::channel();

    thread::scope(|scope| {
        for &index in &formatter_indices_for_extension_files {
            let formatter_invocation = &formatter_invocations[index];
            let files = &files_by_formatter_index[index];
            if files.is_empty() {
                continue;
            }

            let sender = sender.clone();
            let workspace = &workspace_directory;

            scope.spawn(move || {
                sender
                    .send(run_and_report_formatter(
                        mode,
                        formatter_invocation,
                        files,
                        workspace,
                    ))
                    .unwrap();
            });
        }

        drop(sender);

        for formatter_outcome in receiver {
            failed |= formatter_outcome == FormatterOutcome::Failure;
        }

        if failed {
            if let Mode::Check = mode {
                eprintln!(
                    "Some formatters reported unformatted files. Run `bazel run //:format` to fix."
                );
            }
            std::process::exit(1);
        }
    });

    ExitCode::SUCCESS
}

struct Tools {
    deno: PathBuf,
    gofmt: PathBuf,
    rustfmt: PathBuf,
    shfmt: PathBuf,
    buildifier: PathBuf,
    taplo: PathBuf,
    keep_sorted: PathBuf,
}

fn read_tools_from_build() -> Tools {
    let runfiles = Runfiles::create().unwrap_or_else(|e| {
        eprintln!("error: failed to initialize runfiles: {e}");
        std::process::exit(1);
    });

    Tools {
        deno: rlocation_from(&runfiles, env!("DENO"), "DENO"),
        gofmt: rlocation_from(&runfiles, env!("GOFMT"), "GOFMT"),
        rustfmt: rlocation_from(&runfiles, env!("RUSTFMT"), "RUSTFMT"),
        shfmt: rlocation_from(&runfiles, env!("SHFMT"), "SHFMT"),
        buildifier: rlocation_from(&runfiles, env!("BUILDIFIER"), "BUILDIFIER"),
        taplo: rlocation_from(&runfiles, env!("TAPLO"), "TAPLO"),
        keep_sorted: rlocation_from(&runfiles, env!("KEEP_SORTED"), "KEEP_SORTED"),
    }
}

fn rlocation_from(runfiles: &Runfiles, path: &str, name: &str) -> PathBuf {
    runfiles
        .rlocation_from(path, env!("REPOSITORY_NAME"))
        .unwrap_or_else(|| {
            eprintln!("error: failed to resolve runfile for {name}: {path}");
            std::process::exit(1);
        })
}

fn run_and_report_formatter(
    mode: Mode,
    formatter_invocation: &FormatterInvocation,
    files: &[String],
    workspace_directory: &str,
) -> FormatterOutcome {
    let start = Instant::now();

    let output = Command::new(&formatter_invocation.bin)
        .args(&formatter_invocation.args)
        .args(files)
        .current_dir(workspace_directory)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn {}: {e}", formatter_invocation.name));

    let elapsed_ms = start.elapsed().as_millis();
    let formatter_name = formatter_invocation.name;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    if output.status.success() {
        match mode {
            Mode::Check => eprintln!("Checked {formatter_name} in {elapsed_ms}ms"),
            Mode::Fix => eprintln!("Formatted {formatter_name} in {elapsed_ms}ms"),
        }
        return FormatterOutcome::Success;
    }

    eprintln!("FAILED {formatter_name} in {elapsed_ms}ms");
    if !stderr.is_empty() {
        eprint!("{stderr}");
    }
    if !stdout.is_empty() {
        eprint!("{stdout}");
    }
    FormatterOutcome::Failure
}
