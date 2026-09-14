//! Process startup arguments.
//!
//! This module only describes command-line input. Loading configuration and
//! selecting a frontend remain responsibilities of the composition root.

use std::path::PathBuf;

use clap::Parser;

/// Browse Git history in a fast terminal interface.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    /// Browse commits reachable from this branch.
    #[arg(value_name = "BRANCH", conflicts_with = "create_config")]
    pub branch: Option<String>,

    /// Read configuration from this file instead of the default location.
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Create a default configuration file at the default location.
    #[arg(long, conflicts_with = "config")]
    pub create_config: bool,

    /// Run semicolon-separated scripted keys and print the final state.
    #[cfg(debug_assertions)]
    #[arg(long, value_name = "KEYS")]
    pub debug: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_optional_branch() {
        let cli = Cli::try_parse_from(["gitlsd", "feature/topic"]).unwrap();
        assert_eq!(cli.branch.as_deref(), Some("feature/topic"));

        let cli = Cli::try_parse_from(["gitlsd"]).unwrap();
        assert_eq!(cli.branch, None);
    }

    #[test]
    fn branch_conflicts_with_config_creation() {
        assert!(Cli::try_parse_from(["gitlsd", "main", "--create-config"]).is_err());
    }
}
