//! `agent-shape.toml` schema.
//!
//! Each subject tool ships one of these files at its repo root. `jig`
//! deserializes it to drive the runner, judge, and report emitter.

use serde::{Deserialize, Serialize};

/// Root of the `agent-shape.toml` file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentShape {
    pub subject: Subject,
    pub fixture: Fixture,
    pub run: RunConfig,
    pub judge: JudgeConfig,
    pub tasks: Tasks,
    /// Optional: commands the rubric claims exist. `jig check --binary`
    /// runs the binary's `--help` and warns about drift in either
    /// direction (binary advertising commands the rubric omits, or
    /// rubric listing commands the binary doesn't expose).
    ///
    /// Twice in the synthesist study the rubric was missing real
    /// commands and the judge counted them as inventions, producing
    /// phantom regressions. This list closes that loop.
    #[serde(default)]
    pub commands: Option<ExpectedCommands>,
}

/// Subcommands the rubric claims exist. `jig check --binary` cross-
/// references this with the binary's `--help` output.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ExpectedCommands {
    /// Top-level subcommands (e.g. `init`, `tree`, `spec`).
    /// Compared with the first column of `<binary> --help`.
    #[serde(default)]
    pub top_level: Vec<String>,
}

/// Identity of the tool under test.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Subject {
    pub name: String,
    pub binary: String,
    pub description: String,
    /// Optional version pin for retrospective runs.
    #[serde(default)]
    pub version_pin: Option<String>,
}

/// How to stand up and tear down a realistic fixture per trial.
///
/// The runner executes `setup` before every trial so state is isolated.
/// `cleanup` runs after, best-effort.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Fixture {
    /// Shell command (path or inline) that prepares `workdir`.
    pub setup: String,
    /// Optional teardown command.
    #[serde(default)]
    pub cleanup: Option<String>,
    /// Working directory the agent operates in.
    pub workdir: String,
}

/// Default run parameters. CLI flags override these.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunConfig {
    /// Trials per (model, task) cell.
    pub n: u32,
    /// Agent models under test.
    pub models: Vec<String>,
    /// Max agent turns before the trial is judged unfinished.
    /// Anchored on lever canary-bench finding: correction loops decay
    /// exponentially after 2-3 turns.
    pub turn_cap: u32,
    /// Per-trial wall-clock timeout.
    pub timeout_seconds: u64,
}

/// LLM-as-judge configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JudgeConfig {
    /// Default judge model. Overridable via `--judge-model`.
    pub model: String,
    /// Run the judge twice per transcript for inter-rater reliability.
    #[serde(default = "default_true")]
    pub double_score: bool,
    /// Rubric prompt. The judge receives this plus the transcript.
    pub rubric: String,
    /// Fields the judge MUST populate in its JSON response.
    pub required_fields: Vec<String>,
}

fn default_true() -> bool {
    true
}

/// Task batteries. `tuning` is what the tool designer iterates against.
/// `holdout` is sealed against contamination; empty in v1.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Tasks {
    #[serde(default)]
    pub tuning: Vec<Task>,
    #[serde(default)]
    pub holdout: Vec<Task>,
}

/// A single task the agent must complete.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Task {
    /// Stable identifier, referenced in reports.
    pub id: String,
    /// One-line human summary.
    pub summary: String,
    /// Prompt fed verbatim to the agent.
    pub prompt: String,
    /// Human-readable criteria appended to the judge's rubric.
    #[serde(default)]
    pub success_criteria: Vec<String>,
    /// Provenance: who authored this task.
    pub author: String,
    /// Provenance: ISO date (YYYY-MM-DD) of authorship.
    pub created_at: String,
    /// Provenance: subject tool tag the task was authored against.
    /// For hold-out tasks, this proves the task predates any treatment.
    pub sealed_against_tag: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../examples/agent-shape.example.toml");

    #[test]
    fn deserializes_example() {
        let parsed: AgentShape = toml::from_str(EXAMPLE).expect("parse example");
        assert_eq!(parsed.subject.name, "synthesist");
        assert_eq!(parsed.subject.binary, "synthesist");
        assert!(parsed.run.n >= 1);
        assert!(parsed.run.turn_cap >= 1 && parsed.run.turn_cap <= 5);
        assert!(!parsed.tasks.tuning.is_empty());
        assert!(parsed.tasks.holdout.is_empty(), "holdout empty in v1");
        assert!(!parsed.judge.rubric.is_empty());
        assert!(parsed.judge.required_fields.contains(&"score".to_string()));
    }

    #[test]
    fn task_provenance_is_complete() {
        let parsed: AgentShape = toml::from_str(EXAMPLE).expect("parse example");
        for task in &parsed.tasks.tuning {
            assert!(!task.author.is_empty(), "task {} missing author", task.id);
            assert!(!task.created_at.is_empty(), "task {} missing created_at", task.id);
            assert!(
                !task.sealed_against_tag.is_empty(),
                "task {} missing sealed_against_tag",
                task.id
            );
        }
    }

    #[test]
    fn holdout_uses_same_shape() {
        // Build a minimal config with one hold-out task; must round-trip.
        let cfg = AgentShape {
            subject: Subject {
                name: "x".into(),
                binary: "x".into(),
                description: "x".into(),
                version_pin: None,
            },
            fixture: Fixture {
                setup: "true".into(),
                cleanup: None,
                workdir: "/tmp".into(),
            },
            run: RunConfig {
                n: 1,
                models: vec!["claude-sonnet-4-6".into()],
                turn_cap: 3,
                timeout_seconds: 60,
            },
            judge: JudgeConfig {
                model: "claude-haiku-4-5".into(),
                double_score: false,
                rubric: "r".into(),
                required_fields: vec!["score".into()],
            },
            tasks: Tasks {
                tuning: vec![],
                holdout: vec![Task {
                    id: "h1".into(),
                    summary: "s".into(),
                    prompt: "p".into(),
                    success_criteria: vec![],
                    author: "a".into(),
                    created_at: "2026-04-24".into(),
                    sealed_against_tag: "v0.1.0".into(),
                }],
            },
            commands: None,
        };
        let ser = toml::to_string(&cfg).expect("serialize");
        let back: AgentShape = toml::from_str(&ser).expect("round-trip");
        assert_eq!(back.tasks.holdout.len(), 1);
        assert_eq!(back.tasks.holdout[0].id, "h1");
    }
}
