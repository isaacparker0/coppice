use std::process::ExitCode;

use clap::Parser;

use tools__format::{FormatMode, run_workspace_formatter};

#[derive(Parser)]
#[command(version)]
struct CommandLine {
    #[command(subcommand)]
    command: Option<FormatMode>,
}

fn main() -> ExitCode {
    let command_line = CommandLine::parse();
    let format_mode = command_line.command.unwrap_or(FormatMode::Fix);
    run_workspace_formatter(format_mode)
}
