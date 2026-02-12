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

#[derive(Subcommand)]
enum Mode {
    /// Check formatting without modifying files.
    Check,
    /// Format files in place.
    Fix,
}

#[derive(Copy, Clone)]
enum Tool {
    Deno,
    Rustfmt,
    Shfmt,
    Buildifier,
    Taplo,
}

struct Formatter {
    name: &'static str,
    tool: Tool,
    check_args: &'static [&'static str],
    fix_args: &'static [&'static str],
    extensions: &'static [&'static str],
}

struct FormatterInvocation {
    name: &'static str,
    bin: PathBuf,
    args: Vec<String>,
    extensions: &'static [&'static str],
}

const FORMATTERS: [Formatter; 6] = [
    Formatter {
        name: "JSON",
        tool: Tool::Deno,
        check_args: &["fmt", "--check"],
        fix_args: &["fmt"],
        extensions: &["json"],
    },
    Formatter {
        name: "Markdown",
        tool: Tool::Deno,
        check_args: &["fmt", "--check"],
        fix_args: &["fmt"],
        extensions: &["md"],
    },
    Formatter {
        name: "Rust",
        tool: Tool::Rustfmt,
        check_args: &["--check"],
        fix_args: &[],
        extensions: &["rs"],
    },
    Formatter {
        name: "Shell",
        tool: Tool::Shfmt,
        check_args: &["-d"],
        fix_args: &["-w"],
        extensions: &["sh"],
    },
    Formatter {
        name: "Starlark",
        tool: Tool::Buildifier,
        check_args: &["-lint=off", "-mode=check"],
        fix_args: &["-lint=off", "-mode=fix"],
        extensions: &["bzl", "bazel"],
    },
    Formatter {
        name: "TOML",
        tool: Tool::Taplo,
        check_args: &["fmt", "--check"],
        fix_args: &["fmt"],
        extensions: &["toml"],
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
                Tool::Rustfmt => tools.rustfmt.clone(),
                Tool::Shfmt => tools.shfmt.clone(),
                Tool::Buildifier => tools.buildifier.clone(),
                Tool::Taplo => tools.taplo.clone(),
            },
            args: match mode {
                Mode::Check => formatter.check_args,
                Mode::Fix => formatter.fix_args,
            }
            .iter()
            .map(|arg| (*arg).to_string())
            .collect(),
            extensions: formatter.extensions,
        })
        .collect();

    // Build extension -> formatter index map.
    let mut extension_map: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, formatter_invocation) in formatter_invocations.iter().enumerate() {
        for extension in formatter_invocation.extensions {
            extension_map.entry(extension).or_default().push(index);
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

    let deleted: std::collections::HashSet<String> =
        String::from_utf8_lossy(&deleted_output.stdout)
            .lines()
            .map(String::from)
            .collect();

    // Partition files into per-formatter buckets.
    let mut buckets: Vec<Vec<String>> = vec![vec![]; formatter_invocations.len()];

    for line in String::from_utf8_lossy(&git_output.stdout).lines() {
        if deleted.contains(line) {
            continue;
        }

        let path = Path::new(line);
        if let Some(extension) = path.extension().and_then(|e| e.to_str())
            && let Some(indices) = extension_map.get(extension)
        {
            for &index in indices {
                buckets[index].push(line.to_string());
            }
        }
    }

    // Run all formatters in parallel.
    let (sender, receiver) = mpsc::channel();

    thread::scope(|scope| {
        for (index, formatter_invocation) in formatter_invocations.iter().enumerate() {
            let files = &buckets[index];
            if files.is_empty() {
                continue;
            }

            let sender = sender.clone();
            let workspace = &workspace_directory;

            scope.spawn(move || {
                let start = Instant::now();

                let output = Command::new(&formatter_invocation.bin)
                    .args(&formatter_invocation.args)
                    .args(files)
                    .current_dir(workspace)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output()
                    .unwrap_or_else(|e| {
                        panic!("failed to spawn {}: {e}", formatter_invocation.name)
                    });

                let elapsed = start.elapsed();

                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);

                sender
                    .send((
                        formatter_invocation.name,
                        output.status.success(),
                        elapsed,
                        stderr.to_string(),
                        stdout.to_string(),
                    ))
                    .unwrap();
            });
        }

        drop(sender);

        let mut failed = false;
        for (name, success, elapsed, stderr, stdout) in receiver {
            let ms = elapsed.as_millis();
            if success {
                match mode {
                    Mode::Check => eprintln!("Checked {name} in {ms}ms"),
                    Mode::Fix => eprintln!("Formatted {name} in {ms}ms"),
                }
            } else {
                eprintln!("FAILED {name} in {ms}ms");
                if !stderr.is_empty() {
                    eprint!("{stderr}");
                }
                if !stdout.is_empty() {
                    eprint!("{stdout}");
                }
                failed = true;
            }
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
    rustfmt: PathBuf,
    shfmt: PathBuf,
    buildifier: PathBuf,
    taplo: PathBuf,
}

fn read_tools_from_build() -> Tools {
    let runfiles = Runfiles::create().unwrap_or_else(|e| {
        eprintln!("error: failed to initialize runfiles: {e}");
        std::process::exit(1);
    });

    Tools {
        deno: rlocation_from(&runfiles, env!("DENO"), "DENO"),
        rustfmt: rlocation_from(&runfiles, env!("RUSTFMT"), "RUSTFMT"),
        shfmt: rlocation_from(&runfiles, env!("SHFMT"), "SHFMT"),
        buildifier: rlocation_from(&runfiles, env!("BUILDIFIER"), "BUILDIFIER"),
        taplo: rlocation_from(&runfiles, env!("TAPLO"), "TAPLO"),
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
