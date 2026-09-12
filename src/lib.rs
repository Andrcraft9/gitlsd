//! Core modules for the `gitlsd` application.
//!
//! [`config`] defines runtime policy and the shared input vocabulary, [`app`]
//! owns state and behavior, and [`git`] supplies repository data. [`ui`] and
//! [`debug`] are peer frontends over that shared core.

pub mod app;
pub mod cli;
pub mod config;
#[cfg(debug_assertions)]
pub mod debug;
pub mod git;
pub mod ui;
