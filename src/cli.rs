use std::path::PathBuf;

use clap::Parser;

/// Browse Git history in a fast terminal interface.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    /// Read configuration from this file instead of the default location.
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Run semicolon-separated scripted keys and print the final state.
    #[cfg(debug_assertions)]
    #[arg(long, value_name = "KEYS")]
    pub debug: Option<String>,
}
