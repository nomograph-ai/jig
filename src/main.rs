#![deny(warnings, clippy::all)]

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand, ValueEnum};
use jig::checkpoint::{self, CheckpointEntry};
use jig::judge::{JudgeResult, score_trial};
use jig::report::{
    CellReport, Report, ScoredTrial, Section, build_report, emit_json, emit_markdown,
};
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
    /// Pass `--binary <path>` to also cross-reference the rubric's
    /// expected commands with the binary's `--help` output.
    Check {
        #[arg(default_value = "agent-shape.toml")]
        path: PathBuf,
        /// Path to the subject binary. When set, runs `<binary> --help`
        /// and reports drift between the binary's advertised
        /// subcommands and the rubric's `commands.top_level` list.
        #[arg(long)]
        binary: Option<PathBuf>,
    },
    /// Render a previously-emitted JSON report as markdown.
    Render {
        /// Path to the JSON report.
        path: PathBuf,
        /// Write markdown here instead of stdout.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Compare two reports (e.g. baseline vs treated) and emit a
    /// per-cell delta table. No API calls; pure JSON-in, markdown-out.
    Compare {
        /// Path to the baseline JSON report.
        before: PathBuf,
        /// Path to the treated JSON report.
        after: PathBuf,
        /// Where to write the comparison report. Prints to stdout if omitted.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Re-score the trial transcripts in a checkpoint against the
    /// (now-updated) rubric in the TOML, without re-running the
    /// expensive agent trials. Writes a new checkpoint and report.
    Rejudge {
        /// Path to the agent-shape.toml whose rubric to use.
        toml: PathBuf,
        /// Path to the input checkpoint JSONL.
        #[arg(long)]
        from: PathBuf,
        /// Path to the output (rejudged) checkpoint JSONL.
        #[arg(long)]
        to: PathBuf,
        /// Override the judge model.
        #[arg(long)]
        judge_model: Option<String>,
        /// Where to write the corrected report.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Json)]
        format: Format,
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

    /// Append each completed (trial, verdict) pair to this JSONL.
    /// On restart, already-recorded cells are skipped, so a killed
    /// run resumes without losing prior work.
    #[arg(long)]
    checkpoint: Option<PathBuf>,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum Format {
    Json,
    Markdown,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Command::Check { path, binary } => cmd_check(&path, binary.as_deref()),
        Command::Run(args) => cmd_run(args),
        Command::Render { path, output } => cmd_render(&path, output.as_deref()),
        Command::Compare {
            before,
            after,
            output,
        } => cmd_compare(&before, &after, output.as_deref()),
        Command::Rejudge {
            toml,
            from,
            to,
            judge_model,
            output,
            format,
        } => cmd_rejudge(&toml, &from, &to, judge_model, output.as_deref(), format),
    }
}

fn cmd_rejudge(
    toml_path: &Path,
    from: &Path,
    to: &Path,
    judge_model_override: Option<String>,
    output: Option<&Path>,
    format: Format,
) -> Result<()> {
    let config = load_config(toml_path)?;
    let judge_model = judge_model_override.unwrap_or_else(|| config.judge.model.clone());

    let prior =
        checkpoint::load(from).with_context(|| format!("load checkpoint {}", from.display()))?;
    if prior.is_empty() {
        return Err(anyhow!(
            "checkpoint at {} is empty; nothing to rejudge",
            from.display()
        ));
    }

    // Resume: load any entries already in the output checkpoint.
    let already =
        checkpoint::load(to).with_context(|| format!("load resume-checkpoint {}", to.display()))?;
    if !already.is_empty() {
        eprintln!(
            "[jig] rejudge resuming: {} entries already in {}",
            already.len(),
            to.display()
        );
    }

    eprintln!(
        "[jig] rejudging {} entries from {} -> {} (judge: {})",
        prior.len(),
        from.display(),
        to.display(),
        judge_model
    );

    let mut scored_owned: Vec<(Section, TrialResult, JudgeResult)> = Vec::new();
    let mut skipped: Vec<(String, String, u32, String)> = Vec::new();

    for (i, entry) in prior.iter().enumerate() {
        if let Some(existing) = checkpoint::has_entry(
            &already,
            entry.section,
            &entry.task_id,
            &entry.model,
            entry.trial_index,
        ) {
            scored_owned.push((
                entry.section,
                existing.trial.clone(),
                existing.verdict.clone(),
            ));
            continue;
        }

        let task = config
            .tasks
            .tuning
            .iter()
            .chain(config.tasks.holdout.iter())
            .find(|t| t.id == entry.task_id)
            .ok_or_else(|| {
                anyhow!(
                    "checkpoint references task '{}' not present in current TOML",
                    entry.task_id
                )
            })?;

        eprintln!(
            "[jig] rejudge {}/{} {}/{}/{} i={} ...",
            i + 1,
            prior.len(),
            section_label(entry.section),
            entry.task_id,
            entry.model,
            entry.trial_index
        );

        let verdict = match score_trial(&config, task, &entry.trial, &judge_model) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "[jig] rejudge FAILED (skipping): {}/{} i={}: {e}",
                    entry.task_id, entry.model, entry.trial_index
                );
                skipped.push((
                    entry.task_id.clone(),
                    entry.model.clone(),
                    entry.trial_index,
                    format!("{e}"),
                ));
                continue;
            }
        };

        eprintln!(
            "[jig] rejudge done: was={:.2} now={:.2}",
            entry.verdict.first.score, verdict.first.score
        );

        let new_entry = CheckpointEntry {
            section: entry.section,
            task_id: entry.task_id.clone(),
            model: entry.model.clone(),
            trial_index: entry.trial_index,
            trial: entry.trial.clone(),
            verdict: verdict.clone(),
        };
        checkpoint::append(to, &new_entry)
            .with_context(|| format!("append rejudged checkpoint {}", to.display()))?;
        scored_owned.push((entry.section, entry.trial.clone(), verdict));
    }

    if !skipped.is_empty() {
        eprintln!(
            "[jig] rejudge summary: {} rejudged, {} skipped (judge errors)",
            scored_owned.len(),
            skipped.len()
        );
        for (t, m, i, err) in &skipped {
            eprintln!("  skipped {}/{} i={}: {err}", t, m, i);
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
    emit(&report, format, output)
}

fn cmd_compare(before_path: &Path, after_path: &Path, output: Option<&Path>) -> Result<()> {
    let before: Report = serde_json::from_str(&std::fs::read_to_string(before_path)?)
        .with_context(|| format!("parse {}", before_path.display()))?;
    let after: Report = serde_json::from_str(&std::fs::read_to_string(after_path)?)
        .with_context(|| format!("parse {}", after_path.display()))?;
    let md = render_comparison(&before, &after, before_path, after_path);
    match output {
        Some(p) => std::fs::write(p, md).with_context(|| format!("write {}", p.display()))?,
        None => println!("{md}"),
    }
    Ok(())
}

fn render_comparison(
    before: &Report,
    after: &Report,
    before_path: &Path,
    after_path: &Path,
) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "# agent-shape comparison: {} vs {}",
        before.subject, after.subject
    );
    let _ = writeln!(s, "\nbefore: `{}`", before_path.display());
    let _ = writeln!(s, "after:  `{}`", after_path.display());

    let _ = writeln!(s, "\n## Aggregate (tuning battery)\n");
    let _ = writeln!(s, "| metric | before | after | delta |");
    let _ = writeln!(s, "|---|---:|---:|---:|");
    let bm = before.tuning.mean_score.unwrap_or(0.0);
    let am = after.tuning.mean_score.unwrap_or(0.0);
    let _ = writeln!(s, "| mean_score | {bm:.3} | {am:.3} | {:+.3} |", am - bm);
    let bc = before.tuning.completion_rate.unwrap_or(0.0);
    let ac = after.tuning.completion_rate.unwrap_or(0.0);
    let _ = writeln!(
        s,
        "| completion_rate | {:.1}% | {:.1}% | {:+.1}pp |",
        bc * 100.0,
        ac * 100.0,
        (ac - bc) * 100.0
    );
    let bt = before.tuning.mean_tokens.unwrap_or(0.0);
    let at = after.tuning.mean_tokens.unwrap_or(0.0);
    let _ = writeln!(s, "| mean_tokens | {bt:.0} | {at:.0} | {:+.0} |", at - bt);
    let btr = before.tuning.mean_turns.unwrap_or(0.0);
    let atr = after.tuning.mean_turns.unwrap_or(0.0);
    let _ = writeln!(
        s,
        "| mean_turns | {btr:.2} | {atr:.2} | {:+.2} |",
        atr - btr
    );
    let _ = writeln!(
        s,
        "| total_invented | {} | {} | {:+} |",
        before.tuning.total_invented_commands,
        after.tuning.total_invented_commands,
        after.tuning.total_invented_commands as i64 - before.tuning.total_invented_commands as i64
    );
    let _ = writeln!(
        s,
        "| total_fallback_to_sql | {} | {} | {:+} |",
        before.tuning.total_fallback_to_sql,
        after.tuning.total_fallback_to_sql,
        after.tuning.total_fallback_to_sql as i64 - before.tuning.total_fallback_to_sql as i64
    );

    let _ = writeln!(s, "\n## Per-cell deltas\n");
    let _ = writeln!(
        s,
        "| section | task | model | before | after | delta | invented Δ |"
    );
    let _ = writeln!(s, "|---|---|---|---:|---:|---:|---:|");
    use std::collections::BTreeMap;
    let key = |c: &CellReport| -> (Section, String, String) {
        (c.section, c.task_id.clone(), c.model.clone())
    };
    let before_by: BTreeMap<_, _> = before.cells.iter().map(|c| (key(c), c)).collect();
    let after_by: BTreeMap<_, _> = after.cells.iter().map(|c| (key(c), c)).collect();
    let mut all_keys: Vec<_> = before_by.keys().chain(after_by.keys()).cloned().collect();
    all_keys.sort();
    all_keys.dedup();
    for k in all_keys {
        let b = before_by.get(&k);
        let a = after_by.get(&k);
        let bs = b.map(|c| c.mean_score).unwrap_or(0.0);
        let as_ = a.map(|c| c.mean_score).unwrap_or(0.0);
        let bi = b.map(|c| c.invented_commands.len()).unwrap_or(0);
        let ai = a.map(|c| c.invented_commands.len()).unwrap_or(0);
        let sec = match k.0 {
            Section::Tuning => "tuning",
            Section::Holdout => "holdout",
        };
        let _ = writeln!(
            s,
            "| {sec} | {} | {} | {bs:.2} | {as_:.2} | {:+.2} | {:+} |",
            k.1,
            k.2,
            as_ - bs,
            ai as i64 - bi as i64
        );
    }
    s
}

fn cmd_render(json_path: &Path, output: Option<&Path>) -> Result<()> {
    let raw =
        fs::read_to_string(json_path).with_context(|| format!("read {}", json_path.display()))?;
    let report: Report =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", json_path.display()))?;
    let md = emit_markdown(&report);
    match output {
        Some(p) => fs::write(p, md).with_context(|| format!("write {}", p.display()))?,
        None => println!("{md}"),
    }
    Ok(())
}

fn cmd_check(path: &Path, binary: Option<&Path>) -> Result<()> {
    let config = load_config(path)?;
    println!(
        "OK: {} ({} tuning, {} holdout)",
        config.subject.name,
        config.tasks.tuning.len(),
        config.tasks.holdout.len()
    );
    if let Some(bin) = binary {
        let drift = check_command_drift(&config, bin)?;
        if drift.missing_in_rubric.is_empty() && drift.extra_in_rubric.is_empty() {
            println!(
                "rubric ↔ binary OK: {} top-level subcommands match",
                drift.advertised.len()
            );
        } else {
            if !drift.missing_in_rubric.is_empty() {
                eprintln!(
                    "warning: binary advertises {} subcommand(s) the rubric omits:",
                    drift.missing_in_rubric.len()
                );
                for c in &drift.missing_in_rubric {
                    eprintln!("  + {c}");
                }
                eprintln!("  -> add to [commands].top_level so the judge knows they're real");
            }
            if !drift.extra_in_rubric.is_empty() {
                eprintln!(
                    "warning: rubric lists {} subcommand(s) the binary doesn't expose:",
                    drift.extra_in_rubric.len()
                );
                for c in &drift.extra_in_rubric {
                    eprintln!("  - {c}");
                }
                eprintln!("  -> remove from [commands].top_level so the judge doesn't expect them");
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
struct CommandDrift {
    advertised: Vec<String>,
    missing_in_rubric: Vec<String>,
    extra_in_rubric: Vec<String>,
}

fn check_command_drift(config: &AgentShape, binary: &Path) -> Result<CommandDrift> {
    let output = std::process::Command::new(binary)
        .arg("--help")
        .output()
        .with_context(|| format!("run {} --help", binary.display()))?;
    let help_text = String::from_utf8_lossy(&output.stdout);
    let advertised = parse_clap_subcommands(&help_text);
    let listed: std::collections::HashSet<String> = config
        .commands
        .as_ref()
        .map(|c| c.top_level.iter().cloned().collect())
        .unwrap_or_default();
    let advertised_set: std::collections::HashSet<String> = advertised.iter().cloned().collect();

    let missing_in_rubric: Vec<String> = advertised
        .iter()
        .filter(|c| !listed.contains(*c))
        .cloned()
        .collect();
    let extra_in_rubric: Vec<String> = listed
        .iter()
        .filter(|c| !advertised_set.contains(*c))
        .cloned()
        .collect();
    Ok(CommandDrift {
        advertised,
        missing_in_rubric,
        extra_in_rubric,
    })
}

/// Parse top-level subcommand names out of a clap-style `--help`
/// output. Looks for the `Commands:` (or `Subcommands:`) block and
/// extracts the first identifier from each subsequent indented line
/// until a blank line or another section header.
fn parse_clap_subcommands(help: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in help.lines() {
        let trimmed = line.trim_end();
        if !in_block {
            let lower = trimmed.trim().to_ascii_lowercase();
            if lower == "commands:" || lower == "subcommands:" {
                in_block = true;
            }
            continue;
        }
        if trimmed.is_empty() {
            in_block = false;
            continue;
        }
        // Indented lines start the subcommand. Format: "  name   description"
        if !trimmed.starts_with(' ') && !trimmed.starts_with('\t') {
            in_block = false;
            continue;
        }
        let stripped = trimmed.trim_start();
        let first = stripped.split_whitespace().next().unwrap_or("").to_string();
        if first.is_empty()
            || first == "help"
            || !first
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            continue;
        }
        out.push(first);
    }
    out
}

fn cmd_run(args: RunArgs) -> Result<()> {
    let mut config = load_config(&args.path)?;
    let base_dir = args
        .path
        .parent()
        .map(|p| {
            if p.as_os_str().is_empty() {
                Path::new(".")
            } else {
                p
            }
        })
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

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

    let prior: Vec<CheckpointEntry> = match &args.checkpoint {
        Some(p) => {
            let loaded =
                checkpoint::load(p).with_context(|| format!("load checkpoint {}", p.display()))?;
            if !loaded.is_empty() {
                eprintln!(
                    "[jig] resuming from checkpoint: {} prior entries in {}",
                    loaded.len(),
                    p.display()
                );
            }
            loaded
        }
        None => Vec::new(),
    };

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
                for i in 0..config.run.n {
                    if let Some(e) = checkpoint::has_entry(&prior, *section, &task.id, model, i) {
                        scored_owned.push((*section, e.trial.clone(), e.verdict.clone()));
                        continue;
                    }
                    eprintln!(
                        "[jig] trial {}/{} ({}/{}) i={} ...",
                        section_label(*section),
                        task.id,
                        model,
                        config.run.n,
                        i
                    );
                    let trial = run_trial(&config, task, model, &base_dir)
                        .with_context(|| format!("run_trial for {}/{}", task.id, model))?;
                    let verdict = score_trial(&config, task, &trial, &judge_model)
                        .with_context(|| format!("score_trial for {}/{}", task.id, model))?;
                    eprintln!(
                        "[jig] trial done: score={:.2} turns={} tokens={} invented={}",
                        verdict.first.score,
                        trial.num_turns,
                        trial.input_tokens + trial.output_tokens,
                        verdict.first.invented_commands.len()
                    );
                    if let Some(p) = &args.checkpoint {
                        let entry = CheckpointEntry {
                            section: *section,
                            task_id: task.id.clone(),
                            model: model.clone(),
                            trial_index: i,
                            trial: trial.clone(),
                            verdict: verdict.clone(),
                        };
                        checkpoint::append(p, &entry)
                            .with_context(|| format!("append checkpoint {}", p.display()))?;
                    }
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

fn section_label(s: Section) -> &'static str {
    match s {
        Section::Tuning => "tuning",
        Section::Holdout => "holdout",
    }
}

fn select_sections(tuning_only: bool, holdout_only: bool) -> Vec<Section> {
    match (tuning_only, holdout_only) {
        (true, false) => vec![Section::Tuning],
        (false, true) => vec![Section::Holdout],
        _ => vec![Section::Tuning, Section::Holdout],
    }
}

fn load_config(path: &Path) -> Result<AgentShape> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let cfg: AgentShape =
        toml::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    Ok(cfg)
}

fn emit(report: &Report, format: Format, output: Option<&Path>) -> Result<()> {
    let payload = match format {
        Format::Json => emit_json(report),
        Format::Markdown => emit_markdown(report),
    };
    match output {
        Some(path) => {
            fs::write(path, payload).with_context(|| format!("write {}", path.display()))?
        }
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

    #[test]
    fn parse_clap_subcommands_extracts_first_token() {
        let help = "
Usage: tool [OPTIONS] <COMMAND>

Commands:
  init     Initialize the thing
  status   Show status
  tree     Manage trees
  task     Manage tasks
  help     Print this help

Options:
  -h, --help     Print help
";
        let cmds = parse_clap_subcommands(help);
        assert_eq!(cmds, vec!["init", "status", "tree", "task"]);
    }

    #[test]
    fn parse_clap_subcommands_skips_help() {
        let help = "Commands:\n  foo  do foo\n  help  print help\n  bar  do bar\n";
        assert_eq!(parse_clap_subcommands(help), vec!["foo", "bar"]);
    }

    #[test]
    fn parse_clap_subcommands_handles_no_block() {
        let help = "Usage: tool [OPTIONS]\n\nOptions:\n  --help\n";
        assert!(parse_clap_subcommands(help).is_empty());
    }

    #[test]
    fn parse_clap_subcommands_stops_at_section_break() {
        let help = "Commands:\n  one  ok\n  two  ok\n\nOptions:\n  --foo  not a subcommand\n";
        assert_eq!(parse_clap_subcommands(help), vec!["one", "two"]);
    }
}
