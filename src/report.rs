//! Report emitter.
//!
//! Aggregates a collection of (TrialResult, JudgeResult) pairs into a
//! Report split by tuning vs holdout. Primary metric is mean judge
//! score; secondaries are completion rate, tokens, turns, invented
//! commands, and fallback-to-sql count. Emits JSON and Markdown.

use crate::judge::JudgeResult;
use crate::runner::TrialResult;
use crate::schema::AgentShape;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Which battery a task belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Section {
    Tuning,
    Holdout,
}

/// One (trial, verdict) pair with its section assignment.
pub struct ScoredTrial<'a> {
    pub section: Section,
    pub trial: &'a TrialResult,
    pub verdict: &'a JudgeResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub subject: String,
    pub version_pin: Option<String>,
    pub run_timestamp: String,
    pub judge_model: String,
    pub tuning: BatteryReport,
    pub holdout: BatteryReport,
    /// Per-(task, model) breakdown, flattened for easy table rendering.
    pub cells: Vec<CellReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BatteryReport {
    pub n_trials: usize,
    pub mean_score: Option<f64>,
    pub completion_rate: Option<f64>,
    pub mean_tokens: Option<f64>,
    pub mean_turns: Option<f64>,
    pub total_invented_commands: usize,
    pub total_fallback_to_sql: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellReport {
    pub section: Section,
    pub task_id: String,
    pub model: String,
    pub n: usize,
    pub mean_score: f64,
    pub score_stddev: f64,
    pub mean_tokens: f64,
    pub mean_turns: f64,
    pub invented_commands: Vec<String>,
    pub fallback_count: usize,
    pub mean_irr_delta: Option<f64>,
}

/// Aggregate a slice of scored trials into a Report. Pure function.
pub fn build_report(
    config: &AgentShape,
    scored: &[ScoredTrial<'_>],
    run_timestamp: String,
    judge_model: String,
) -> Report {
    let tuning: Vec<&ScoredTrial> = scored.iter().filter(|s| s.section == Section::Tuning).collect();
    let holdout: Vec<&ScoredTrial> = scored.iter().filter(|s| s.section == Section::Holdout).collect();

    Report {
        subject: config.subject.name.clone(),
        version_pin: config.subject.version_pin.clone(),
        run_timestamp,
        judge_model,
        tuning: battery_summary(&tuning),
        holdout: battery_summary(&holdout),
        cells: build_cells(scored),
    }
}

fn battery_summary(scored: &[&ScoredTrial]) -> BatteryReport {
    let n = scored.len();
    if n == 0 {
        return BatteryReport::default();
    }
    let scores: Vec<f64> = scored.iter().map(|s| s.verdict.first.score).collect();
    let completions: Vec<bool> = scored.iter().map(|s| s.verdict.first.completed).collect();
    let tokens: Vec<f64> = scored
        .iter()
        .map(|s| (s.trial.input_tokens + s.trial.output_tokens) as f64)
        .collect();
    let turns: Vec<f64> = scored.iter().map(|s| s.trial.num_turns as f64).collect();
    let invented: usize = scored
        .iter()
        .map(|s| s.verdict.first.invented_commands.len())
        .sum();
    let fallback: usize = scored
        .iter()
        .filter(|s| s.verdict.first.fallback_to_sql)
        .count();

    BatteryReport {
        n_trials: n,
        mean_score: Some(mean(&scores)),
        completion_rate: Some(
            completions.iter().filter(|c| **c).count() as f64 / n as f64,
        ),
        mean_tokens: Some(mean(&tokens)),
        mean_turns: Some(mean(&turns)),
        total_invented_commands: invented,
        total_fallback_to_sql: fallback,
    }
}

fn build_cells(scored: &[ScoredTrial<'_>]) -> Vec<CellReport> {
    // Group by (section, task_id, model).
    let mut groups: BTreeMap<(Section, String, String), Vec<&ScoredTrial>> = BTreeMap::new();
    for s in scored {
        groups
            .entry((s.section, s.trial.task_id.clone(), s.trial.model.clone()))
            .or_default()
            .push(s);
    }

    let mut out = Vec::with_capacity(groups.len());
    for ((section, task_id, model), items) in groups {
        let scores: Vec<f64> = items.iter().map(|s| s.verdict.first.score).collect();
        let tokens: Vec<f64> = items
            .iter()
            .map(|s| (s.trial.input_tokens + s.trial.output_tokens) as f64)
            .collect();
        let turns: Vec<f64> = items.iter().map(|s| s.trial.num_turns as f64).collect();
        let mut invented_acc: Vec<String> = Vec::new();
        let mut fallback_count = 0usize;
        let mut irr_deltas: Vec<f64> = Vec::new();
        for s in &items {
            invented_acc.extend(s.verdict.first.invented_commands.clone());
            if s.verdict.first.fallback_to_sql {
                fallback_count += 1;
            }
            if let Some(d) = s.verdict.irr_delta {
                irr_deltas.push(d);
            }
        }
        invented_acc.sort();
        invented_acc.dedup();

        out.push(CellReport {
            section,
            task_id,
            model,
            n: items.len(),
            mean_score: mean(&scores),
            score_stddev: stddev(&scores),
            mean_tokens: mean(&tokens),
            mean_turns: mean(&turns),
            invented_commands: invented_acc,
            fallback_count,
            mean_irr_delta: if irr_deltas.is_empty() {
                None
            } else {
                Some(mean(&irr_deltas))
            },
        });
    }
    out
}

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        0.0
    } else {
        xs.iter().sum::<f64>() / xs.len() as f64
    }
}

fn stddev(xs: &[f64]) -> f64 {
    let n = xs.len();
    if n < 2 {
        return 0.0;
    }
    let m = mean(xs);
    let var = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n as f64 - 1.0);
    var.sqrt()
}

pub fn emit_json(report: &Report) -> String {
    serde_json::to_string_pretty(report).expect("report always serializes")
}

pub fn emit_markdown(report: &Report) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# agent-shape report: {}", report.subject);
    if let Some(pin) = &report.version_pin {
        let _ = writeln!(s, "\nversion_pin: `{pin}`");
    }
    let _ = writeln!(s, "\nrun_timestamp: `{}`", report.run_timestamp);
    let _ = writeln!(s, "judge_model: `{}`", report.judge_model);

    let _ = writeln!(s, "\n## Tuning battery\n");
    write_battery(&mut s, &report.tuning);

    let _ = writeln!(s, "\n## Holdout battery\n");
    if report.holdout.n_trials == 0 {
        let _ = writeln!(
            s,
            "_empty in v1 (schema supports it; corpus deferred)_"
        );
    } else {
        write_battery(&mut s, &report.holdout);
    }

    let _ = writeln!(s, "\n## Per-cell breakdown\n");
    let _ = writeln!(
        s,
        "| section | task | model | n | score | stddev | tokens | turns | invented | fallback | irr_delta |"
    );
    let _ = writeln!(
        s,
        "|---------|------|-------|---|-------|--------|--------|-------|----------|----------|-----------|"
    );
    for c in &report.cells {
        let sec = match c.section {
            Section::Tuning => "tuning",
            Section::Holdout => "holdout",
        };
        let irr = c
            .mean_irr_delta
            .map(|d| format!("{d:.3}"))
            .unwrap_or_else(|| "n/a".into());
        let invented_preview = if c.invented_commands.is_empty() {
            "—".into()
        } else {
            c.invented_commands.join("; ")
        };
        let _ = writeln!(
            s,
            "| {sec} | {task} | {model} | {n} | {score:.3} | {sd:.3} | {tok:.0} | {tu:.2} | {inv} | {fb} | {irr} |",
            task = c.task_id,
            model = c.model,
            n = c.n,
            score = c.mean_score,
            sd = c.score_stddev,
            tok = c.mean_tokens,
            tu = c.mean_turns,
            inv = invented_preview,
            fb = c.fallback_count,
        );
    }
    s
}

fn write_battery(s: &mut String, b: &BatteryReport) {
    let _ = writeln!(s, "- n_trials: {}", b.n_trials);
    if let Some(x) = b.mean_score {
        let _ = writeln!(s, "- mean_score: {x:.3}");
    }
    if let Some(x) = b.completion_rate {
        let _ = writeln!(s, "- completion_rate: {:.1}%", x * 100.0);
    }
    if let Some(x) = b.mean_tokens {
        let _ = writeln!(s, "- mean_tokens: {x:.0}");
    }
    if let Some(x) = b.mean_turns {
        let _ = writeln!(s, "- mean_turns: {x:.2}");
    }
    let _ = writeln!(
        s,
        "- total_invented_commands: {}",
        b.total_invented_commands
    );
    let _ = writeln!(
        s,
        "- total_fallback_to_sql: {}",
        b.total_fallback_to_sql
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::judge::{JudgeResult, JudgeScore};
    use crate::runner::TrialResult;

    fn trial(task_id: &str, model: &str, tokens: u64, turns: u32) -> TrialResult {
        TrialResult {
            task_id: task_id.into(),
            model: model.into(),
            bash_commands: vec![],
            assistant_texts: vec![],
            num_turns: turns,
            input_tokens: tokens / 2,
            output_tokens: tokens / 2,
            cost_usd: 0.01,
            duration_ms: 1000,
            terminal_reason: "completed".into(),
            is_error: false,
            completed_under_turn_cap: true,
            final_text: String::new(),
            setup_failed: false,
            timed_out: false,
        }
    }

    fn verdict(
        task_id: &str,
        model: &str,
        score: f64,
        completed: bool,
        invented: Vec<&str>,
        fallback: bool,
        irr: Option<f64>,
    ) -> JudgeResult {
        let s = JudgeScore {
            score,
            first_command: Some("x".into()),
            first_command_existed: true,
            completed,
            invented_commands: invented.into_iter().map(String::from).collect(),
            fallback_to_sql: fallback,
            reasoning: "r".into(),
        };
        JudgeResult {
            task_id: task_id.into(),
            model_under_test: model.into(),
            judge_model: "claude-haiku-4-5".into(),
            first: s,
            second: None,
            irr_delta: irr,
        }
    }

    fn sample_config() -> AgentShape {
        toml::from_str(include_str!("../examples/agent-shape.example.toml"))
            .expect("parse example")
    }

    #[test]
    fn empty_inputs_produce_empty_batteries() {
        let cfg = sample_config();
        let report = build_report(&cfg, &[], "t".into(), "claude-haiku-4-5".into());
        assert_eq!(report.tuning.n_trials, 0);
        assert_eq!(report.holdout.n_trials, 0);
        assert!(report.tuning.mean_score.is_none());
        assert!(report.holdout.mean_score.is_none());
        assert!(report.cells.is_empty());
    }

    #[test]
    fn aggregates_by_task_and_model() {
        let cfg = sample_config();
        let t1a = trial("t1", "m1", 100, 2);
        let t1b = trial("t1", "m1", 200, 3);
        let t2 = trial("t2", "m2", 150, 2);
        let v1a = verdict("t1", "m1", 1.0, true, vec![], false, Some(0.0));
        let v1b = verdict("t1", "m1", 0.5, true, vec!["synthesist tree show"], false, Some(0.1));
        let v2 = verdict("t2", "m2", 0.0, false, vec![], true, None);

        let scored = vec![
            ScoredTrial {
                section: Section::Tuning,
                trial: &t1a,
                verdict: &v1a,
            },
            ScoredTrial {
                section: Section::Tuning,
                trial: &t1b,
                verdict: &v1b,
            },
            ScoredTrial {
                section: Section::Tuning,
                trial: &t2,
                verdict: &v2,
            },
        ];

        let r = build_report(&cfg, &scored, "t".into(), "judge".into());
        assert_eq!(r.tuning.n_trials, 3);
        assert!((r.tuning.mean_score.unwrap() - 0.5).abs() < 1e-9);
        assert!((r.tuning.completion_rate.unwrap() - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(r.tuning.total_invented_commands, 1);
        assert_eq!(r.tuning.total_fallback_to_sql, 1);

        assert_eq!(r.cells.len(), 2);
        let t1m1 = r
            .cells
            .iter()
            .find(|c| c.task_id == "t1" && c.model == "m1")
            .unwrap();
        assert_eq!(t1m1.n, 2);
        assert!((t1m1.mean_score - 0.75).abs() < 1e-9);
        assert!(t1m1.score_stddev > 0.0);
        assert_eq!(t1m1.invented_commands, vec!["synthesist tree show".to_string()]);
        assert!((t1m1.mean_irr_delta.unwrap() - 0.05).abs() < 1e-9);
    }

    #[test]
    fn holdout_section_isolated_from_tuning() {
        let cfg = sample_config();
        let t = trial("h1", "m", 100, 1);
        let v = verdict("h1", "m", 0.9, true, vec![], false, None);
        let scored = vec![ScoredTrial {
            section: Section::Holdout,
            trial: &t,
            verdict: &v,
        }];
        let r = build_report(&cfg, &scored, "t".into(), "j".into());
        assert_eq!(r.tuning.n_trials, 0);
        assert_eq!(r.holdout.n_trials, 1);
        assert!((r.holdout.mean_score.unwrap() - 0.9).abs() < 1e-9);
    }

    #[test]
    fn markdown_notes_empty_holdout() {
        let cfg = sample_config();
        let t = trial("t1", "m", 10, 1);
        let v = verdict("t1", "m", 1.0, true, vec![], false, None);
        let scored = vec![ScoredTrial {
            section: Section::Tuning,
            trial: &t,
            verdict: &v,
        }];
        let r = build_report(&cfg, &scored, "t".into(), "j".into());
        let md = emit_markdown(&r);
        assert!(md.contains("## Holdout battery"));
        assert!(md.contains("empty in v1"));
        assert!(md.contains("# agent-shape report: synthesist"));
    }

    #[test]
    fn json_roundtrips() {
        let cfg = sample_config();
        let t = trial("t1", "m", 10, 1);
        let v = verdict("t1", "m", 1.0, true, vec![], false, None);
        let scored = vec![ScoredTrial {
            section: Section::Tuning,
            trial: &t,
            verdict: &v,
        }];
        let r = build_report(&cfg, &scored, "t".into(), "j".into());
        let j = emit_json(&r);
        let back: Report = serde_json::from_str(&j).expect("roundtrip");
        assert_eq!(back.subject, "synthesist");
        assert_eq!(back.cells.len(), 1);
    }
}
