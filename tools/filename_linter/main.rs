//! Lint filenames to ensure they follow repo naming conventions.

use std::collections::BTreeMap;
use std::env;
use std::path::Path;
use std::process::{Command, ExitCode};

use globset::{Glob, GlobSet, GlobSetBuilder};
use tools__filename_linter::{is_lower_or_upper_snake_case, is_lower_snake_case};

struct Rule {
    message: &'static str,
    check: fn(&str) -> bool,
    exclude_patterns: Option<&'static [&'static str]>,
}

struct CompiledRule {
    message: &'static str,
    check: fn(&str) -> bool,
    exclusions: GlobSet,
}

const RULES: &[Rule] = &[
    Rule {
        message: "Files must use snake_case naming",
        check: |path| {
            let components: Vec<_> = Path::new(path)
                .components()
                .filter_map(|component| component.as_os_str().to_str())
                .collect();
            if components.is_empty() {
                return false;
            }
            let (directories, filenames) = components.split_at(components.len() - 1);

            // Directory segments must be lower snake_case.
            for directory in directories {
                // Strip leading '.' for hidden directories.
                let name = directory.strip_prefix('.').unwrap_or(directory);
                if !is_lower_snake_case(name) {
                    return true;
                }
            }

            // Filenames can be lower or UPPER snake_case.
            for filename in filenames {
                // Strip leading '.' for hidden files, then take base name without extension.
                let without_dot = filename.strip_prefix('.').unwrap_or(filename);
                let base = without_dot.split('.').next().unwrap_or(without_dot);
                if !is_lower_or_upper_snake_case(base) {
                    return true;
                }
            }

            false
        },
        exclude_patterns: Some(&[
            // keep-sorted start
            ".github/workflows/**",
            ".pre-commit-config.yaml",
            "Cargo.lock",
            "Cargo.toml",
            "bin/**",
            // keep-sorted end
        ]),
    },
    Rule {
        message: "Bazel build files must be named BUILD.bazel",
        check: |path| {
            let filename = Path::new(path).file_name().and_then(|name| name.to_str());
            filename == Some("BUILD")
        },
        exclude_patterns: None,
    },
    Rule {
        message: "YAML files must use .yaml extension (not .yml)",
        check: |path| {
            Path::new(path)
                .extension()
                .and_then(|extension| extension.to_str())
                == Some("yml")
        },
        exclude_patterns: None,
    },
];

fn main() -> ExitCode {
    let workspace_directory = env::var("BUILD_WORKING_DIRECTORY").expect("must run under Bazel");

    let compiled_rules: Vec<CompiledRule> = RULES
        .iter()
        .map(|rule| {
            let mut builder = GlobSetBuilder::new();
            if let Some(patterns) = rule.exclude_patterns {
                for pattern in patterns {
                    builder.add(Glob::new(pattern).expect("invalid glob pattern"));
                }
            }
            CompiledRule {
                message: rule.message,
                check: rule.check,
                exclusions: builder.build().expect("failed to build glob set"),
            }
        })
        .collect();

    let output = match Command::new("git")
        .args([
            "-C",
            &workspace_directory,
            "ls-files",
            "--cached",
            "--modified",
            "--other",
            "--exclude-standard",
            "-z",
        ])
        .output()
    {
        Ok(output) => output,
        Err(err) => {
            eprintln!("failed to run git ls-files: {err}");
            return ExitCode::FAILURE;
        }
    };

    if !output.status.success() {
        eprintln!("git ls-files failed");
        return ExitCode::FAILURE;
    }

    let mut violations: BTreeMap<usize, Vec<String>> = BTreeMap::new();

    for path_bytes in output.stdout.split(|&b| b == 0) {
        let path = match std::str::from_utf8(path_bytes) {
            Ok(s) if !s.is_empty() => s,
            _ => continue,
        };

        for (index, rule) in compiled_rules.iter().enumerate() {
            if rule.exclusions.is_match(path) {
                continue;
            }
            if (rule.check)(path) {
                violations.entry(index).or_default().push(path.to_string());
            }
        }
    }

    if violations.is_empty() {
        return ExitCode::SUCCESS;
    }

    let total: usize = violations.values().map(Vec::len).sum();
    eprintln!("filename_linter: {total} violation(s)\n");

    for (index, paths) in &violations {
        let rule = &compiled_rules[*index];
        eprintln!("{}:", rule.message);
        for path in paths {
            eprintln!("  {path}");
        }
        eprintln!();
    }

    eprintln!(
        "If a file MUST diverge from convention, add a glob pattern to \
         that rule's exclude_patterns in tools/filename_linter/main.rs."
    );

    ExitCode::FAILURE
}
