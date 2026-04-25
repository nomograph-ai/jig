#![deny(warnings, clippy::all)]

//! jig: agent-shape testing harness.
//!
//! Runs runtime-in-the-loop task batteries against a tool's CLI to
//! measure first-try command success, tokens, turns, and
//! invented-command count. The runtime today is `claude -p`; the
//! framework is runtime-agnostic in language and ready to slot in
//! other agents (GPT, Gemini, local models) once the runner accepts
//! a configurable spawn command.

pub mod checkpoint;
pub mod judge;
pub mod report;
pub mod runner;
pub mod schema;
