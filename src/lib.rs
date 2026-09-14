//! Core modules for the `gitlsd` application.
//!
//! [`config`] defines runtime policy and the shared input vocabulary, [`app`]
//! owns state and behavior, [`git`] supplies repository data, and [`editor`]
//! prepares and launches configured editor commands. [`ui`] and [`debug`] are
//! peer frontends over that shared core.

pub mod app;
pub mod cli;
pub mod config;
#[cfg(debug_assertions)]
pub mod debug;
pub mod editor;
pub mod git;
pub mod ui;
