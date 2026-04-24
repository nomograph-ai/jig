#![deny(warnings, clippy::all)]

//! jig: agent-shape testing harness.
//!
//! Runs claude-in-the-loop task batteries against a tool's CLI to
//! measure first-try command success, tokens, turns, and
//! invented-command count.

pub mod runner;
pub mod schema;
