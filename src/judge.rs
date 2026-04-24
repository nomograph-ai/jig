//! LLM-as-judge scorer.
//!
//! Takes a `TrialResult` plus the task context and asks a judge model
//! (Haiku by default) to score the trial against the tool's rubric.
//! When `double_score` is true, the judge runs twice so we can measure
//! inter-rater reliability from the pair.

use crate::runner::TrialResult;
use crate::schema::{AgentShape, Task};
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Deserializer, Serialize};
use std::io::Write;
use std::process::{Command, Stdio};

fn nullable_bool<'de, D: Deserializer<'de>>(de: D) -> Result<bool, D::Error> {
    Ok(Option::<bool>::deserialize(de)?.unwrap_or(false))
}

fn nullable_string_vec<'de, D: Deserializer<'de>>(
    de: D,
) -> Result<Vec<String>, D::Error> {
    Ok(Option::<Vec<String>>::deserialize(de)?.unwrap_or_default())
}

/// One judge's verdict on one trial.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JudgeScore {
    pub score: f64,
    /// None when the agent never invoked Bash (the judge answers null).
    #[serde(default)]
    pub first_command: Option<String>,
    #[serde(default, deserialize_with = "nullable_bool")]
    pub first_command_existed: bool,
    #[serde(default, deserialize_with = "nullable_bool")]
    pub completed: bool,
    #[serde(default, deserialize_with = "nullable_string_vec")]
    pub invented_commands: Vec<String>,
    #[serde(default, deserialize_with = "nullable_bool")]
    pub fallback_to_sql: bool,
    pub reasoning: String,
}

/// The judge's full output for a trial. When double_score is off, `second` is None.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgeResult {
    pub task_id: String,
    pub model_under_test: String,
    pub judge_model: String,
    pub first: JudgeScore,
    pub second: Option<JudgeScore>,
    /// Absolute score delta between first and second. None when double_score off.
    pub irr_delta: Option<f64>,
}

/// Build the prompt handed to the judge. Pure function, deterministic
/// given inputs - exposed for testing.
pub fn build_judge_prompt(
    config: &AgentShape,
    task: &Task,
    trial: &TrialResult,
) -> String {
    let mut bash_list = String::new();
    for (i, cmd) in trial.bash_commands.iter().enumerate() {
        bash_list.push_str(&format!("{}. {}\n", i + 1, cmd));
    }
    if bash_list.is_empty() {
        bash_list.push_str("(none)\n");
    }

    let mut text_list = String::new();
    for (i, text) in trial.assistant_texts.iter().enumerate() {
        text_list.push_str(&format!("{}. {}\n", i + 1, text));
    }
    if text_list.is_empty() {
        text_list.push_str("(none)\n");
    }

    let criteria = if task.success_criteria.is_empty() {
        "(none specified)".into()
    } else {
        task.success_criteria
            .iter()
            .map(|c| format!("- {c}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let required_fields = config.judge.required_fields.join(", ");

    format!(
        "{rubric}\n\n\
         ## Subject tool\n\
         binary: {binary}\n\
         description: {description}\n\n\
         ## Task\n\
         {prompt}\n\n\
         ## Success criteria\n\
         {criteria}\n\n\
         ## Agent transcript\n\
         ### Bash commands (in order)\n\
         {bash_list}\n\
         ### Assistant text (in order)\n\
         {text_list}\n\
         ### Outcome summary\n\
         - num_turns: {num_turns}\n\
         - completed_under_turn_cap: {cap}\n\
         - terminal_reason: {reason}\n\
         - is_error: {err}\n\
         - final_text: {final_text}\n\n\
         Respond in strict JSON with exactly these fields: {required_fields}\n\
         Do not wrap the JSON in markdown fences. Do not include prose outside the JSON.",
        rubric = config.judge.rubric,
        binary = config.subject.binary,
        description = config.subject.description,
        prompt = task.prompt,
        num_turns = trial.num_turns,
        cap = trial.completed_under_turn_cap,
        reason = trial.terminal_reason,
        err = trial.is_error,
        final_text = trial.final_text,
    )
}

/// Parse the judge's JSON response into a JudgeScore. Tolerant of
/// leading/trailing whitespace and of a markdown fence if the judge
/// ignores instructions.
pub fn parse_judge_response(raw: &str) -> Result<JudgeScore> {
    let trimmed = raw.trim();
    let stripped = if trimmed.starts_with("```") {
        let without_first = trimmed
            .strip_prefix("```json")
            .or_else(|| trimmed.strip_prefix("```"))
            .unwrap_or(trimmed);
        without_first
            .trim_start_matches('\n')
            .trim_end_matches("```")
            .trim()
    } else {
        trimmed
    };

    let parsed: JudgeScore =
        serde_json::from_str(stripped).context("judge response is not valid JudgeScore JSON")?;

    if !(0.0..=1.0).contains(&parsed.score) {
        return Err(anyhow!(
            "judge score {} outside [0.0, 1.0]",
            parsed.score
        ));
    }
    Ok(parsed)
}

/// Invoke the judge subprocess once and return its verdict.
fn invoke_judge(prompt: &str, judge_model: &str) -> Result<JudgeScore> {
    let mut child = Command::new("claude")
        .args([
            "-p",
            "--print",
            "--output-format",
            "text",
            "--bare",
            "--dangerously-skip-permissions",
            "--model",
            judge_model,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("spawn judge subprocess")?;

    {
        let stdin = child.stdin.as_mut().context("judge stdin")?;
        stdin
            .write_all(prompt.as_bytes())
            .context("write judge prompt")?;
    }
    let output = child
        .wait_with_output()
        .context("wait judge subprocess")?;
    if !output.status.success() {
        return Err(anyhow!(
            "judge exited with status {}",
            output.status
        ));
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    parse_judge_response(&raw)
}

/// Score one trial. Runs the judge once, or twice when double_score is on.
pub fn score_trial(
    config: &AgentShape,
    task: &Task,
    trial: &TrialResult,
    judge_model: &str,
) -> Result<JudgeResult> {
    let prompt = build_judge_prompt(config, task, trial);
    let first = invoke_judge(&prompt, judge_model)?;
    let (second, irr_delta) = if config.judge.double_score {
        let s = invoke_judge(&prompt, judge_model)?;
        let delta = (first.score - s.score).abs();
        (Some(s), Some(delta))
    } else {
        (None, None)
    };
    Ok(JudgeResult {
        task_id: task.id.clone(),
        model_under_test: trial.model.clone(),
        judge_model: judge_model.into(),
        first,
        second,
        irr_delta,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_score() -> JudgeScore {
        JudgeScore {
            score: 0.5,
            first_command: Some("synthesist tree show".into()),
            first_command_existed: false,
            completed: true,
            invented_commands: vec!["synthesist tree show".into()],
            fallback_to_sql: false,
            reasoning: "reached for non-existent command first".into(),
        }
    }

    #[test]
    fn parses_strict_json() {
        let s = sample_score();
        let raw = serde_json::to_string(&s).unwrap();
        let parsed = parse_judge_response(&raw).unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn parses_markdown_fenced_json() {
        let s = sample_score();
        let inner = serde_json::to_string(&s).unwrap();
        let fenced = format!("```json\n{inner}\n```");
        let parsed = parse_judge_response(&fenced).unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn parses_fence_without_lang() {
        let s = sample_score();
        let inner = serde_json::to_string(&s).unwrap();
        let fenced = format!("```\n{inner}\n```");
        let parsed = parse_judge_response(&fenced).unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn rejects_score_out_of_range() {
        let raw = r#"{"score":1.5,"first_command":"x","first_command_existed":true,"completed":true,"invented_commands":[],"fallback_to_sql":false,"reasoning":"r"}"#;
        assert!(parse_judge_response(raw).is_err());
    }

    #[test]
    fn rejects_missing_fields() {
        let raw = r#"{"score":0.5}"#;
        assert!(parse_judge_response(raw).is_err());
    }

    #[test]
    fn accepts_null_first_command() {
        let raw = r#"{"score":0.0,"first_command":null,"first_command_existed":false,"completed":false,"invented_commands":[],"fallback_to_sql":false,"reasoning":"agent ran no commands"}"#;
        let parsed = parse_judge_response(raw).unwrap();
        assert!(parsed.first_command.is_none());
    }

    #[test]
    fn accepts_null_bool_and_vec_fields() {
        let raw = r#"{"score":0.0,"first_command":null,"first_command_existed":null,"completed":null,"invented_commands":null,"fallback_to_sql":null,"reasoning":"everything null"}"#;
        let parsed = parse_judge_response(raw).unwrap();
        assert!(!parsed.first_command_existed);
        assert!(!parsed.completed);
        assert!(parsed.invented_commands.is_empty());
        assert!(!parsed.fallback_to_sql);
    }

    #[test]
    fn prompt_includes_all_required_sections() {
        let config: AgentShape = toml::from_str(include_str!(
            "../examples/agent-shape.example.toml"
        ))
        .unwrap();
        let task = &config.tasks.tuning[0];
        let trial = TrialResult {
            task_id: task.id.clone(),
            model: "claude-sonnet-4-6".into(),
            bash_commands: vec!["synthesist status".into()],
            assistant_texts: vec!["Let me check.".into()],
            num_turns: 2,
            input_tokens: 100,
            output_tokens: 50,
            cost_usd: 0.01,
            duration_ms: 5000,
            terminal_reason: "completed".into(),
            is_error: false,
            completed_under_turn_cap: true,
            final_text: "done".into(),
            setup_failed: false,
            timed_out: false,
        };
        let prompt = build_judge_prompt(&config, task, &trial);
        assert!(prompt.contains("## Task"));
        assert!(prompt.contains("## Success criteria"));
        assert!(prompt.contains("## Agent transcript"));
        assert!(prompt.contains("synthesist status"));
        assert!(prompt.contains("Let me check."));
        assert!(prompt.contains("num_turns: 2"));
        assert!(prompt.contains("strict JSON"));
    }
}
