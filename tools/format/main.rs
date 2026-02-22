use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
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
    Check,
    Fix,
}

#[derive(Copy, Clone)]
enum Selector {
    Extension(&'static str),
    ExactFirstLine(&'static str),
}

#[derive(Copy, Clone)]
enum Scope {
    AllFiles,
    Selectors(&'static [Selector]),
}

#[derive(Copy, Clone)]
struct ToolConfig {
    display_name: &'static str,
    runfile_path: &'static str,
    check_args: &'static [&'static str],
    fix_args: &'static [&'static str],
    scope: Scope,
}

struct RuntimeToolConfig {
    display_name: &'static str,
    bin: PathBuf,
    check_args: &'static [&'static str],
    fix_args: &'static [&'static str],
    scope: Scope,
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum FormatterOutcome {
    Success,
    Failure,
}

const TOOL_CONFIGS: [ToolConfig; 8] = [
    ToolConfig {
        display_name: "deno fmt",
        runfile_path: env!("DENO"),
        check_args: &["fmt", "--check"],
        fix_args: &["fmt"],
        scope: Scope::Selectors(&[
            Selector::Extension("json"),
            Selector::Extension("md"),
            Selector::Extension("html"),
            Selector::Extension("css"),
            Selector::Extension("js"),
            Selector::Extension("yaml"),
        ]),
    },
    ToolConfig {
        display_name: "gofmt",
        runfile_path: env!("GOFMT"),
        check_args: &["-d"],
        fix_args: &["-w"],
        scope: Scope::Selectors(&[Selector::Extension("go")]),
    },
    ToolConfig {
        display_name: "rustfmt",
        runfile_path: env!("RUSTFMT"),
        check_args: &["--check"],
        fix_args: &[],
        scope: Scope::Selectors(&[Selector::Extension("rs")]),
    },
    ToolConfig {
        display_name: "shfmt",
        runfile_path: env!("SHFMT"),
        check_args: &["-d"],
        fix_args: &["-w"],
        scope: Scope::Selectors(&[
            Selector::Extension("sh"),
            Selector::ExactFirstLine("#!/usr/bin/env bash"),
        ]),
    },
    ToolConfig {
        display_name: "buildifier",
        runfile_path: env!("BUILDIFIER"),
        check_args: &["-lint=off", "-mode=check"],
        fix_args: &["-lint=off", "-mode=fix"],
        scope: Scope::Selectors(&[Selector::Extension("bzl"), Selector::Extension("bazel")]),
    },
    ToolConfig {
        display_name: "taplo",
        runfile_path: env!("TAPLO"),
        check_args: &["fmt", "--check"],
        fix_args: &["fmt"],
        scope: Scope::Selectors(&[Selector::Extension("toml")]),
    },
    ToolConfig {
        display_name: "keep-sorted",
        runfile_path: env!("KEEP_SORTED"),
        check_args: &["--mode=lint"],
        fix_args: &["--mode=fix"],
        scope: Scope::AllFiles,
    },
    ToolConfig {
        display_name: "tofu fmt",
        runfile_path: env!("TOFU"),
        check_args: &["fmt", "-check", "-diff"],
        fix_args: &["fmt"],
        scope: Scope::Selectors(&[Selector::Extension("tf")]),
    },
];

fn main() -> ExitCode {
    let command_line = CommandLine::parse();
    let mode = command_line.command.unwrap_or(Mode::Fix);

    let workspace_directory = std::env::var("BUILD_WORKSPACE_DIRECTORY").unwrap_or_else(|_| {
        eprintln!("error: BUILD_WORKSPACE_DIRECTORY not set. Run via `bazel run`.");
        std::process::exit(1);
    });

    let runfiles = Runfiles::create().unwrap_or_else(|e| {
        eprintln!("error: failed to initialize runfiles: {e}");
        std::process::exit(1);
    });
    let mut runtime_tool_configs = Vec::with_capacity(TOOL_CONFIGS.len());
    for tool_config in &TOOL_CONFIGS {
        let bin = runfiles
            .rlocation_from(tool_config.runfile_path, env!("REPOSITORY_NAME"))
            .unwrap_or_else(|| {
                eprintln!(
                    "error: failed to resolve runfile for {}: {}",
                    tool_config.display_name, tool_config.runfile_path
                );
                std::process::exit(1);
            });
        runtime_tool_configs.push(RuntimeToolConfig {
            display_name: tool_config.display_name,
            check_args: tool_config.check_args,
            fix_args: tool_config.fix_args,
            scope: tool_config.scope,
            bin,
        });
    }

    // Build derived routing indexes from tool configs.
    let mut tool_indices_for_all_files: Vec<usize> = Vec::new();
    let mut tool_index_by_extension: HashMap<&str, usize> = HashMap::new();
    let mut tool_index_by_exact_first_line: HashMap<&str, usize> = HashMap::new();
    for (tool_index, runtime_tool_config) in runtime_tool_configs.iter().enumerate() {
        match runtime_tool_config.scope {
            Scope::AllFiles => tool_indices_for_all_files.push(tool_index),
            Scope::Selectors(selectors) => {
                for selector in selectors {
                    match selector {
                        Selector::Extension(extension) => {
                            if let Some(existing_tool_index) =
                                tool_index_by_extension.insert(extension, tool_index)
                            {
                                eprintln!(
                                    "error: extension '.{extension}' is configured for multiple tools: {} and {}",
                                    runtime_tool_configs[existing_tool_index].display_name,
                                    runtime_tool_configs[tool_index].display_name
                                );
                                std::process::exit(1);
                            }
                        }
                        Selector::ExactFirstLine(exact_first_line) => {
                            if let Some(existing_tool_index) =
                                tool_index_by_exact_first_line.insert(exact_first_line, tool_index)
                            {
                                eprintln!(
                                    "error: exact first line '{}' is configured for multiple tools: {} and {}",
                                    exact_first_line,
                                    runtime_tool_configs[existing_tool_index].display_name,
                                    runtime_tool_configs[tool_index].display_name
                                );
                                std::process::exit(1);
                            }
                        }
                    }
                }
            }
        }
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

    let deleted: HashSet<String> = String::from_utf8_lossy(&deleted_output.stdout)
        .lines()
        .map(String::from)
        .collect();

    // Partition files into per-tool file lists.
    let mut files_by_tool_index: Vec<Vec<String>> = vec![Vec::new(); runtime_tool_configs.len()];
    let mut first_line_by_file: HashMap<String, Option<String>> = HashMap::new();

    for line in String::from_utf8_lossy(&git_output.stdout).lines() {
        if deleted.contains(line) {
            continue;
        }

        for &tool_index in &tool_indices_for_all_files {
            files_by_tool_index[tool_index].push(line.to_string());
        }

        let path = Path::new(line);
        if let Some(extension) = path.extension().and_then(|value| value.to_str())
            && let Some(&tool_index) = tool_index_by_extension.get(extension)
        {
            files_by_tool_index[tool_index].push(line.to_string());
        }

        if path.extension().is_none()
            && let Some(first_line) =
                first_line_from_file(line, &workspace_directory, &mut first_line_by_file)
        {
            let exact_first_line = first_line.trim_end_matches(['\r', '\n']);
            if let Some(&tool_index) = tool_index_by_exact_first_line.get(exact_first_line) {
                files_by_tool_index[tool_index].push(line.to_string());
            }
        }
    }

    // Run all-file tools sequentially to avoid concurrent edits.
    let mut failed = false;
    for &tool_index in &tool_indices_for_all_files {
        let files = &files_by_tool_index[tool_index];
        if files.is_empty() {
            continue;
        }

        failed |= run_and_report_tool(
            mode,
            &runtime_tool_configs[tool_index],
            files,
            &workspace_directory,
        ) == FormatterOutcome::Failure;
    }

    // Run tools with selectors in parallel.
    let (sender, receiver) = mpsc::channel();
    thread::scope(|scope| {
        for (tool_index, files) in files_by_tool_index.iter().enumerate() {
            if files.is_empty() {
                continue;
            }
            if matches!(runtime_tool_configs[tool_index].scope, Scope::AllFiles) {
                continue;
            }
            let sender = sender.clone();
            let workspace = &workspace_directory;
            let runtime_tool_config = &runtime_tool_configs[tool_index];
            scope.spawn(move || {
                sender
                    .send(run_and_report_tool(
                        mode,
                        runtime_tool_config,
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

fn run_and_report_tool(
    mode: Mode,
    runtime_tool_config: &RuntimeToolConfig,
    files: &[String],
    workspace_directory: &str,
) -> FormatterOutcome {
    let args = match mode {
        Mode::Check => runtime_tool_config.check_args,
        Mode::Fix => runtime_tool_config.fix_args,
    };

    let start = Instant::now();

    let output = Command::new(&runtime_tool_config.bin)
        .args(args)
        .args(files)
        .current_dir(workspace_directory)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn {}: {e}", runtime_tool_config.display_name));

    let elapsed_ms = start.elapsed().as_millis();
    let tool_name = runtime_tool_config.display_name;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    if output.status.success() {
        eprintln!("Ran {tool_name} in {elapsed_ms}ms");
        return FormatterOutcome::Success;
    }

    eprintln!("FAILED {tool_name} in {elapsed_ms}ms");
    if !stderr.is_empty() {
        eprint!("{stderr}");
    }
    if !stdout.is_empty() {
        eprint!("{stdout}");
    }
    FormatterOutcome::Failure
}

fn first_line_from_file(
    relative_path: &str,
    workspace_directory: &str,
    first_line_by_file: &mut HashMap<String, Option<String>>,
) -> Option<String> {
    if let Some(first_line) = first_line_by_file.get(relative_path) {
        return first_line.clone();
    }

    let full_path = Path::new(workspace_directory).join(relative_path);
    let first_line = File::open(full_path).ok().and_then(|file| {
        let mut first_line = String::new();
        let mut reader = BufReader::new(file);
        reader.read_line(&mut first_line).ok()?;
        Some(first_line)
    });
    first_line_by_file.insert(relative_path.to_string(), first_line.clone());
    first_line
}
