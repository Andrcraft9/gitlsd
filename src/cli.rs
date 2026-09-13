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
