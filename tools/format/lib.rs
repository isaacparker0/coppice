use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use clap::Subcommand;
use globset::{Glob, GlobSet, GlobSetBuilder};
use runfiles::Runfiles;

mod built_in_tools;

#[derive(Copy, Clone, Subcommand)]
pub enum FormatMode {
    Check,
    Fix,
}

#[derive(Copy, Clone)]
enum Selector {
    Extension(&'static str),
    NoExtensionExactFirstLine(&'static str),
}

#[derive(Copy, Clone)]
enum Scope {
    AllFiles,
    Selectors(&'static [Selector]),
}

#[derive(Copy, Clone)]
struct ToolConfig {
    display_name: &'static str,
    scope: Scope,
    backend: ToolBackend,
}

struct RuntimeToolConfig {
    display_name: &'static str,
    scope: Scope,
    backend: RuntimeToolBackend,
}

#[derive(Copy, Clone)]
struct ExternalBinaryToolConfig {
    runfile_path: &'static str,
    check_args: &'static [&'static str],
    fix_args: &'static [&'static str],
}

#[derive(Copy, Clone)]
pub(crate) struct BuiltInToolConfig {
    pub(crate) implementation: BuiltInToolImplementation,
}

#[derive(Copy, Clone)]
pub(crate) enum ToolBackend {
    ExternalBinary(ExternalBinaryToolConfig),
    BuiltIn(BuiltInToolConfig),
}

struct RuntimeExternalBinaryToolConfig {
    bin: PathBuf,
    check_args: &'static [&'static str],
    fix_args: &'static [&'static str],
}

enum RuntimeToolBackend {
    ExternalBinary(RuntimeExternalBinaryToolConfig),
    BuiltIn(BuiltInToolConfig),
}

#[derive(Copy, Clone)]
pub(crate) struct BuiltInToolContext<'a> {
    pub(crate) mode: FormatMode,
    pub(crate) files: &'a [String],
    pub(crate) workspace_directory: &'a str,
}

pub(crate) type BuiltInToolImplementation = fn(BuiltInToolContext<'_>) -> FormatterOutcome;

#[derive(Copy, Clone, Eq, PartialEq)]
enum FormatterOutcome {
    Success,
    Failure,
}

const TOOL_CONFIGS: [ToolConfig; 10] = [
    ToolConfig {
        display_name: "deno fmt",
        scope: Scope::Selectors(&[
            Selector::Extension("css"),
            Selector::Extension("html"),
            Selector::Extension("js"),
            Selector::Extension("json"),
            Selector::Extension("md"),
            Selector::Extension("ts"),
            Selector::Extension("tsx"),
            Selector::Extension("yaml"),
        ]),
        backend: ToolBackend::ExternalBinary(ExternalBinaryToolConfig {
            runfile_path: env!("DENO"),
            check_args: &["fmt", "--check"],
            fix_args: &["fmt"],
        }),
    },
    ToolConfig {
        display_name: "gofmt",
        scope: Scope::Selectors(&[Selector::Extension("go")]),
        backend: ToolBackend::ExternalBinary(ExternalBinaryToolConfig {
            runfile_path: env!("GOFMT"),
            check_args: &["-d"],
            fix_args: &["-w"],
        }),
    },
    ToolConfig {
        display_name: "rustfmt",
        scope: Scope::Selectors(&[Selector::Extension("rs")]),
        backend: ToolBackend::ExternalBinary(ExternalBinaryToolConfig {
            runfile_path: env!("RUSTFMT"),
            check_args: &["--check"],
            fix_args: &[],
        }),
    },
    ToolConfig {
        display_name: "shfmt",
        scope: Scope::Selectors(&[
            Selector::Extension("sh"),
            Selector::NoExtensionExactFirstLine("#!/usr/bin/env bash"),
        ]),
        backend: ToolBackend::ExternalBinary(ExternalBinaryToolConfig {
            runfile_path: env!("SHFMT"),
            check_args: &["-d"],
            fix_args: &["-w"],
        }),
    },
    ToolConfig {
        display_name: "buildifier",
        scope: Scope::Selectors(&[Selector::Extension("bzl"), Selector::Extension("bazel")]),
        backend: ToolBackend::ExternalBinary(ExternalBinaryToolConfig {
            runfile_path: env!("BUILDIFIER"),
            check_args: &["-lint=off", "-mode=check"],
            fix_args: &["-lint=off", "-mode=fix"],
        }),
    },
    ToolConfig {
        display_name: "taplo",
        scope: Scope::Selectors(&[Selector::Extension("toml")]),
        backend: ToolBackend::ExternalBinary(ExternalBinaryToolConfig {
            runfile_path: env!("TAPLO"),
            check_args: &["fmt", "--check"],
            fix_args: &["fmt"],
        }),
    },
    ToolConfig {
        display_name: "keep-sorted",
        scope: Scope::AllFiles,
        backend: ToolBackend::ExternalBinary(ExternalBinaryToolConfig {
            runfile_path: env!("KEEP_SORTED"),
            check_args: &["--mode=lint"],
            fix_args: &["--mode=fix"],
        }),
    },
    ToolConfig {
        display_name: "tofu fmt",
        scope: Scope::Selectors(&[Selector::Extension("tf")]),
        backend: ToolBackend::ExternalBinary(ExternalBinaryToolConfig {
            runfile_path: env!("TOFU"),
            check_args: &["fmt", "-check", "-diff"],
            fix_args: &["fmt"],
        }),
    },
    ToolConfig {
        display_name: "normalize line endings",
        scope: Scope::AllFiles,
        backend: ToolBackend::BuiltIn(BuiltInToolConfig {
            implementation: built_in_tools::normalize_line_endings,
        }),
    },
    ToolConfig {
        display_name: "normalize trailing newlines",
        scope: Scope::AllFiles,
        backend: ToolBackend::BuiltIn(BuiltInToolConfig {
            implementation: built_in_tools::normalize_trailing_newlines,
        }),
    },
];

pub fn run_workspace_formatter(format_mode: FormatMode) -> ExitCode {
    let Ok(workspace_directory) = std::env::var("BUILD_WORKSPACE_DIRECTORY") else {
        eprintln!("error: BUILD_WORKSPACE_DIRECTORY not set. Run via `bazel run`.");
        return ExitCode::FAILURE;
    };

    let runfiles = match Runfiles::create() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("error: failed to initialize runfiles: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut runtime_tool_configs = Vec::with_capacity(TOOL_CONFIGS.len());
    for tool_config in &TOOL_CONFIGS {
        let runtime_backend = match tool_config.backend {
            ToolBackend::ExternalBinary(external_binary_tool_config) => {
                let Some(bin) = runfiles.rlocation_from(
                    external_binary_tool_config.runfile_path,
                    env!("REPOSITORY_NAME"),
                ) else {
                    eprintln!(
                        "error: failed to resolve runfile for {}: {}",
                        tool_config.display_name, external_binary_tool_config.runfile_path
                    );
                    return ExitCode::FAILURE;
                };
                RuntimeToolBackend::ExternalBinary(RuntimeExternalBinaryToolConfig {
                    bin,
                    check_args: external_binary_tool_config.check_args,
                    fix_args: external_binary_tool_config.fix_args,
                })
            }
            ToolBackend::BuiltIn(built_in_tool_config) => {
                RuntimeToolBackend::BuiltIn(built_in_tool_config)
            }
        };
        runtime_tool_configs.push(RuntimeToolConfig {
            display_name: tool_config.display_name,
            scope: tool_config.scope,
            backend: runtime_backend,
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
                                return ExitCode::FAILURE;
                            }
                        }
                        Selector::NoExtensionExactFirstLine(exact_first_line) => {
                            if let Some(existing_tool_index) =
                                tool_index_by_exact_first_line.insert(exact_first_line, tool_index)
                            {
                                eprintln!(
                                    "error: exact first line '{}' is configured for multiple tools: {} and {}",
                                    exact_first_line,
                                    runtime_tool_configs[existing_tool_index].display_name,
                                    runtime_tool_configs[tool_index].display_name
                                );
                                return ExitCode::FAILURE;
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

    // Build one canonical workspace file list: non-deleted + deduplicated.
    let mut workspace_files = Vec::new();
    let mut seen_workspace_files: HashSet<String> = HashSet::new();
    for line in String::from_utf8_lossy(&git_output.stdout).lines() {
        if deleted.contains(line) || !seen_workspace_files.insert(line.to_string()) {
            continue;
        }
        workspace_files.push(line.to_string());
    }

    let format_ignore_glob_set = load_format_ignore_glob_set(&workspace_directory);
    let workspace_files: Vec<String> = workspace_files
        .into_iter()
        .filter(|path| !format_ignore_glob_set.is_match(path))
        .collect();

    // Partition files into per-tool file lists.
    let mut files_by_tool_index: Vec<Vec<String>> = vec![Vec::new(); runtime_tool_configs.len()];
    let mut first_line_by_file: HashMap<String, Option<String>> = HashMap::new();

    for file in &workspace_files {
        for &tool_index in &tool_indices_for_all_files {
            files_by_tool_index[tool_index].push(file.clone());
        }

        let path = Path::new(file);
        if let Some(extension) = path.extension().and_then(|value| value.to_str())
            && let Some(&tool_index) = tool_index_by_extension.get(extension)
        {
            files_by_tool_index[tool_index].push(file.clone());
        }

        if path.extension().is_none()
            && let Some(first_line) =
                first_line_from_file(file, &workspace_directory, &mut first_line_by_file)
        {
            let exact_first_line = first_line.trim_end_matches('\n');
            if let Some(&tool_index) = tool_index_by_exact_first_line.get(exact_first_line) {
                files_by_tool_index[tool_index].push(file.clone());
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
            format_mode,
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
                        format_mode,
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
    });

    if failed {
        if let FormatMode::Check = format_mode {
            eprintln!(
                "Some formatters reported unformatted files. Run `bazel run //:format` to fix."
            );
        }
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

fn run_and_report_tool(
    mode: FormatMode,
    runtime_tool_config: &RuntimeToolConfig,
    files: &[String],
    workspace_directory: &str,
) -> FormatterOutcome {
    let start = Instant::now();
    let tool_name = runtime_tool_config.display_name;
    let formatter_outcome = match &runtime_tool_config.backend {
        RuntimeToolBackend::ExternalBinary(runtime_external_binary_tool_config) => {
            let args = match mode {
                FormatMode::Check => runtime_external_binary_tool_config.check_args,
                FormatMode::Fix => runtime_external_binary_tool_config.fix_args,
            };
            let output = Command::new(&runtime_external_binary_tool_config.bin)
                .args(args)
                .args(files)
                .current_dir(workspace_directory)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .unwrap_or_else(|e| panic!("failed to spawn {tool_name}: {e}"));
            if output.status.success() {
                FormatterOutcome::Success
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                if !stderr.is_empty() {
                    eprint!("{stderr}");
                }
                if !stdout.is_empty() {
                    eprint!("{stdout}");
                }
                FormatterOutcome::Failure
            }
        }
        RuntimeToolBackend::BuiltIn(built_in_tool_config) => {
            (built_in_tool_config.implementation)(BuiltInToolContext {
                mode,
                files,
                workspace_directory,
            })
        }
    };
    let elapsed_ms = start.elapsed().as_millis();
    match formatter_outcome {
        FormatterOutcome::Success => eprintln!("Ran {tool_name} in {elapsed_ms}ms"),
        FormatterOutcome::Failure => eprintln!("FAILED {tool_name} in {elapsed_ms}ms"),
    }
    formatter_outcome
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

fn load_format_ignore_glob_set(workspace_directory: &str) -> GlobSet {
    let format_ignore_path = Path::new(workspace_directory).join(".formatignore");
    let file = match File::open(&format_ignore_path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return GlobSetBuilder::new()
                .build()
                .expect("empty glob set must compile");
        }
        Err(error) => {
            panic!("failed to read {}: {error}", format_ignore_path.display(),);
        }
    };

    let mut glob_set_builder = GlobSetBuilder::new();
    for (line_number_zero_based, line_result) in BufReader::new(file).lines().enumerate() {
        let line = line_result.unwrap_or_else(|error| {
            panic!(
                "failed to read {} at line {}: {error}",
                format_ignore_path.display(),
                line_number_zero_based + 1,
            )
        });
        let trimmed_line = line.trim();
        if trimmed_line.is_empty() || trimmed_line.starts_with('#') {
            continue;
        }
        let glob = Glob::new(trimmed_line).unwrap_or_else(|error| {
            panic!(
                "invalid glob pattern in {} at line {}: {error}",
                format_ignore_path.display(),
                line_number_zero_based + 1,
            )
        });
        glob_set_builder.add(glob);
    }

    glob_set_builder.build().unwrap_or_else(|error| {
        panic!(
            "failed to compile glob patterns from {}: {error}",
            format_ignore_path.display(),
        )
    })
}
