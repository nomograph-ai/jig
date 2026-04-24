#![deny(warnings, clippy::all)]

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand, ValueEnum};
use jig::judge::{JudgeResult, score_trial};
use jig::report::{Report, ScoredTrial, Section, build_report, emit_json, emit_markdown};
use jig::runner::{TrialResult, run_trial};
use jig::schema::AgentShape;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Parser, Debug)]
#[command(name = "jig", version, about = "Agent-shape testing harness")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run the battery described in an agent-shape.toml and emit a report.
    Run(RunArgs),
    /// Validate an agent-shape.toml without running the battery.
    Check {
        #[arg(default_value = "agent-shape.toml")]
        path: PathBuf,
    },
}

#[derive(Parser, Debug)]
struct RunArgs {
    /// Path to the agent-shape.toml.
    #[arg(default_value = "agent-shape.toml")]
    path: PathBuf,

    /// Only run tuning-set tasks.
    #[arg(long, conflicts_with = "holdout_only")]
    tuning_only: bool,

    /// Only run holdout tasks (v2+).
    #[arg(long)]
    holdout_only: bool,

    /// Override trials per (task, model) cell.
    #[arg(long)]
    n: Option<u32>,

    /// Override the judge model.
    #[arg(long)]
    judge_model: Option<String>,

    /// Assert the TOML's subject.name matches this (safety guard).
    #[arg(long)]
    subject: Option<String>,

    /// Where to write the report. Prints to stdout if omitted.
    #[arg(long)]
    output: Option<PathBuf>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Markdown)]
    format: Format,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum Format {
    Json,
    Markdown,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Command::Check { path } => cmd_check(&path),
        Command::Run(args) => cmd_run(args),
    }
}

fn cmd_check(path: &Path) -> Result<()> {
    let config = load_config(path)?;
    println!(
        "OK: {} ({} tuning, {} holdout)",
        config.subject.name,
        config.tasks.tuning.len(),
        config.tasks.holdout.len()
    );
    Ok(())
}

fn cmd_run(args: RunArgs) -> Result<()> {
    let mut config = load_config(&args.path)?;

    if let Some(expected) = &args.subject
        && expected != &config.subject.name
    {
        return Err(anyhow!(
            "subject mismatch: TOML declares '{}' but --subject={} was passed. Either fix the flag or point at a different agent-shape.toml.",
            config.subject.name,
            expected
        ));
    }

    if let Some(n) = args.n {
        config.run.n = n;
    }
    let judge_model = args
        .judge_model
        .unwrap_or_else(|| config.judge.model.clone());

    let sections = select_sections(args.tuning_only, args.holdout_only);

    let mut scored_owned: Vec<(Section, TrialResult, JudgeResult)> = Vec::new();

    for section in &sections {
        let tasks = match section {
            Section::Tuning => &config.tasks.tuning,
            Section::Holdout => &config.tasks.holdout,
        };
        if tasks.is_empty() {
            continue;
        }
        for task in tasks {
            for model in &config.run.models {
                for _ in 0..config.run.n {
                    let trial = run_trial(&config, task, model)
                        .with_context(|| format!("run_trial for {}/{}", task.id, model))?;
                    let verdict = score_trial(&config, task, &trial, &judge_model)
                        .with_context(|| format!("score_trial for {}/{}", task.id, model))?;
                    scored_owned.push((*section, trial, verdict));
                }
            }
        }
    }

    let scored_view: Vec<ScoredTrial> = scored_owned
        .iter()
        .map(|(section, trial, verdict)| ScoredTrial {
            section: *section,
            trial,
            verdict,
        })
        .collect();

    let ts = current_iso_timestamp();
    let report = build_report(&config, &scored_view, ts, judge_model);
    emit(&report, args.format, args.output.as_deref())
}

fn select_sections(tuning_only: bool, holdout_only: bool) -> Vec<Section> {
    match (tuning_only, holdout_only) {
        (true, false) => vec![Section::Tuning],
        (false, true) => vec![Section::Holdout],
        _ => vec![Section::Tuning, Section::Holdout],
    }
}

fn load_config(path: &Path) -> Result<AgentShape> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?;
    let cfg: AgentShape = toml::from_str(&raw)
        .with_context(|| format!("parse {}", path.display()))?;
    Ok(cfg)
}

fn emit(report: &Report, format: Format, output: Option<&Path>) -> Result<()> {
    let payload = match format {
        Format::Json => emit_json(report),
        Format::Markdown => emit_markdown(report),
    };
    match output {
        Some(path) => fs::write(path, payload)
            .with_context(|| format!("write {}", path.display()))?,
        None => println!("{payload}"),
    }
    Ok(())
}

fn current_iso_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("unix:{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_sections_defaults_to_both() {
        assert_eq!(
            select_sections(false, false),
            vec![Section::Tuning, Section::Holdout]
        );
    }

    #[test]
    fn select_sections_honors_tuning_only() {
        assert_eq!(select_sections(true, false), vec![Section::Tuning]);
    }

    #[test]
    fn select_sections_honors_holdout_only() {
        assert_eq!(select_sections(false, true), vec![Section::Holdout]);
    }
}
