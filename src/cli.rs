use std::path::PathBuf;

use clap::{Parser, Subcommand};

const HELP_FOOTER: &str = "\
Exit codes:
  0  Clean exit (Done, or update completed)
  1  Generic failure (file not found, render error, etc.)
  2  Configuration / parse error
  3  Port already in use (or other server bind failure)
  5  Interrupted (Ctrl+C)

Docs: https://github.com/chrisraethke/discuss-cli
LLM ref: https://github.com/chrisraethke/discuss-cli/blob/main/llms.txt";

#[derive(Debug, Parser)]
#[command(
    name = "discuss",
    version,
    about = "Launch a live bidirectional markdown review session.",
    arg_required_else_help = true,
    after_help = HELP_FOOTER,
    after_long_help = HELP_FOOTER
)]
pub struct Args {
    #[arg(value_name = "FILE", help = "Markdown file to review")]
    pub file: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    #[command(about = "Check for and install discuss updates.")]
    Update,
}

#[cfg(test)]
mod tests {
    use clap::{error::ErrorKind, CommandFactory, Parser};

    use super::*;

    #[test]
    fn bare_command_displays_help() {
        let error = Args::try_parse_from(["discuss"]).expect_err("bare command should show help");

        assert_eq!(
            error.kind(),
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn parses_markdown_file_argument() {
        let args = Args::try_parse_from(["discuss", "plan.md"]).expect("file arg should parse");

        assert_eq!(args.file, Some(PathBuf::from("plan.md")));
        assert!(args.command.is_none());
    }

    #[test]
    fn parses_update_subcommand() {
        let args = Args::try_parse_from(["discuss", "update"]).expect("update should parse");

        assert!(args.file.is_none());
        assert!(matches!(args.command, Some(Commands::Update)));
    }

    #[test]
    fn help_contains_exit_codes_and_references() {
        let help = Args::command().render_long_help().to_string();

        for expected in [
            "Exit codes:",
            "0  Clean exit",
            "1  Generic failure",
            "2  Configuration / parse error",
            "3  Port already in use",
            "5  Interrupted",
            "Docs:",
            "LLM ref:",
        ] {
            assert!(
                help.contains(expected),
                "expected help to contain {expected:?}\n{help}"
            );
        }
    }

    #[test]
    fn version_reports_package_version() {
        let error =
            Args::try_parse_from(["discuss", "--version"]).expect_err("--version should exit");

        assert_eq!(error.kind(), ErrorKind::DisplayVersion);
        assert!(error.to_string().contains(env!("CARGO_PKG_VERSION")));
    }
}
