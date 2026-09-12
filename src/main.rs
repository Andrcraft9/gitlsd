//! Process entry point and composition root.
//!
//! Startup arguments and configuration select and construct the Git adapter,
//! application state, and interactive or debug frontend. Application behavior
//! belongs in the library modules rather than here.

use std::process::ExitCode;

use clap::Parser;
use gitlsd::app::App;
use gitlsd::cli::Cli;
use gitlsd::config::Config;
use gitlsd::git::{GitHistory, current_directory};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("gitlsd: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let config = Config::load(cli.config.as_deref())?;
    let directory = current_directory()?;
    let mut source = GitHistory::new(
        directory,
        config.log_command.clone(),
        config.preview_command.clone(),
        config.show_commit_command.clone(),
        config.show_command.clone(),
    );
    let mut app = App::new(config);
    app.initialize(&mut source)?;

    #[cfg(debug_assertions)]
    if let Some(script) = cli.debug {
        let snapshot = gitlsd::debug::run_script(&mut app, &mut source, &script)?;
        print!("{snapshot}");
        return Ok(());
    }

    gitlsd::ui::run(&mut app, &mut source)?;
    Ok(())
}
