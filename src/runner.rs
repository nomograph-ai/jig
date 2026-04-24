//! Trial runner.
//!
//! Spawns `claude -p --output-format stream-json` in a fixture
//! workdir, captures the event stream, extracts bash commands,
//! aggregates the result summary.

use crate::schema::{AgentShape, Task};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// One trial outcome. The judge consumes this; the report aggregates it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrialResult {
    pub task_id: String,
    pub model: String,
    /// Every bash command the agent tried, in order.
    pub bash_commands: Vec<String>,
    /// Assistant messages (text content), in order.
    pub assistant_texts: Vec<String>,
    pub num_turns: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub duration_ms: u64,
    pub terminal_reason: String,
    pub is_error: bool,
    pub completed_under_turn_cap: bool,
    pub final_text: String,
    pub setup_failed: bool,
    pub timed_out: bool,
}

impl TrialResult {
    fn failed_setup(task_id: &str, model: &str) -> Self {
        Self {
            task_id: task_id.into(),
            model: model.into(),
            bash_commands: vec![],
            assistant_texts: vec![],
            num_turns: 0,
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: 0.0,
            duration_ms: 0,
            terminal_reason: "setup_failed".into(),
            is_error: true,
            completed_under_turn_cap: false,
            final_text: String::new(),
            setup_failed: true,
            timed_out: false,
        }
    }
}

/// Parse a stream-json output into a TrialResult. Pure function so
/// it's testable against recorded fixtures.
pub fn parse_event_stream(
    lines: impl Iterator<Item = String>,
    task_id: &str,
    model: &str,
    turn_cap: u32,
) -> TrialResult {
    let mut bash_commands = Vec::new();
    let mut assistant_texts = Vec::new();
    let mut final_result: Option<serde_json::Value> = None;

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let event: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if event.get("type").and_then(|v| v.as_str()) == Some("assistant")
            && let Some(content) = event
                .pointer("/message/content")
                .and_then(|v| v.as_array())
        {
            for item in content {
                match item.get("type").and_then(|v| v.as_str()) {
                    Some("tool_use") => {
                        if item.get("name").and_then(|v| v.as_str()) == Some("Bash")
                            && let Some(cmd) =
                                item.pointer("/input/command").and_then(|v| v.as_str())
                        {
                            bash_commands.push(cmd.to_string());
                        }
                    }
                    Some("text") => {
                        if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                            assistant_texts.push(text.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }

        if event.get("type").and_then(|v| v.as_str()) == Some("result") {
            final_result = Some(event);
        }
    }

    let r = final_result.as_ref();
    let num_turns = r
        .and_then(|v| v.get("num_turns"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let input_tokens = r
        .and_then(|v| v.pointer("/usage/input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = r
        .and_then(|v| v.pointer("/usage/output_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cost_usd = r
        .and_then(|v| v.get("total_cost_usd"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let terminal_reason = r
        .and_then(|v| v.get("terminal_reason"))
        .and_then(|v| v.as_str())
        .unwrap_or("no_result")
        .to_string();
    let is_error = r
        .and_then(|v| v.get("is_error"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let final_text = r
        .and_then(|v| v.get("result"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    TrialResult {
        task_id: task_id.into(),
        model: model.into(),
        bash_commands,
        assistant_texts,
        num_turns,
        input_tokens,
        output_tokens,
        cost_usd,
        duration_ms: 0,
        terminal_reason,
        is_error,
        completed_under_turn_cap: num_turns <= turn_cap && !is_error,
        final_text,
        setup_failed: false,
        timed_out: false,
    }
}

/// Spawn claude and capture the trial. Errors only on IO / spawn
/// failures; agent failures surface inside `TrialResult`.
///
/// `base_dir` anchors relative fixture and workdir paths (typically
/// the directory containing the `agent-shape.toml`). Absolute paths
/// in the config are respected as-is.
pub fn run_trial(
    config: &AgentShape,
    task: &Task,
    model: &str,
    base_dir: &Path,
) -> Result<TrialResult> {
    let workdir = resolve_path(&config.fixture.workdir, base_dir);
    let setup = resolve_path(&config.fixture.setup, base_dir);

    if !run_fixture_cmd(setup.to_string_lossy().as_ref(), base_dir) {
        return Ok(TrialResult::failed_setup(&task.id, model));
    }

    let start = Instant::now();

    let mut child = Command::new("claude")
        .args([
            "-p",
            "--print",
            "--output-format",
            "stream-json",
            "--verbose",
            "--bare",
            "--dangerously-skip-permissions",
            "--model",
            model,
            "--add-dir",
            workdir.to_string_lossy().as_ref(),
        ])
        // `--add-dir` is variadic; the separator stops clap from
        // consuming the prompt as another dir.
        .arg("--")
        .arg(&task.prompt)
        .current_dir(&workdir)
        // Prevent session/instance leakage: any SYNTHESIST_* var from
        // the parent shell would make the agent see the wrong DB.
        .env_remove("SYNTHESIST_SESSION")
        .env_remove("SYNTHESIST_DIR")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("spawn claude subprocess")?;

    let stdout = child.stdout.take().context("take stdout")?;
    let reader = BufReader::new(stdout);
    let timeout = Duration::from_secs(config.run.timeout_seconds);

    let mut collected = Vec::new();
    let mut timed_out = false;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        collected.push(line);
        if start.elapsed() > timeout {
            timed_out = true;
            let _ = child.kill();
            break;
        }
    }

    let _ = child.wait();
    let duration_ms = start.elapsed().as_millis() as u64;

    if let Some(cleanup) = &config.fixture.cleanup {
        let resolved = resolve_path(cleanup, base_dir);
        let _ = run_fixture_cmd(resolved.to_string_lossy().as_ref(), base_dir);
    }

    let mut result = parse_event_stream(collected.into_iter(), &task.id, model, config.run.turn_cap);
    result.duration_ms = duration_ms;
    result.timed_out = timed_out;
    if timed_out {
        result.terminal_reason = "timeout".into();
        result.is_error = true;
    }
    Ok(result)
}

fn run_fixture_cmd(cmd: &str, cwd: &Path) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .env_remove("SYNTHESIST_SESSION")
        .env_remove("SYNTHESIST_DIR")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Resolve a possibly-relative path against a base directory.
/// Absolute paths and anything starting with `~` or `$` is returned
/// unchanged (the shell will handle the latter when the path is fed
/// to `sh -c`).
fn resolve_path(path: &str, base: &Path) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() || path.starts_with('~') || path.starts_with('$') {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/stream-two-bash.jsonl");

    #[test]
    fn extracts_bash_commands_in_order() {
        let lines = FIXTURE.lines().map(|s| s.to_string());
        let result = parse_event_stream(lines, "t1", "claude-sonnet-4-6", 3);
        assert_eq!(
            result.bash_commands,
            vec!["ls".to_string(), "pwd".to_string()]
        );
    }

    #[test]
    fn extracts_final_metrics() {
        let lines = FIXTURE.lines().map(|s| s.to_string());
        let result = parse_event_stream(lines, "t1", "claude-sonnet-4-6", 3);
        assert_eq!(result.num_turns, 3);
        assert_eq!(result.input_tokens, 42);
        assert_eq!(result.output_tokens, 100);
        assert!((result.cost_usd - 0.0125).abs() < 1e-9);
        assert_eq!(result.terminal_reason, "completed");
        assert!(!result.is_error);
        assert_eq!(result.final_text, "done");
    }

    #[test]
    fn turn_cap_flag_reflects_comparison() {
        let lines = FIXTURE.lines().map(|s| s.to_string());
        let under = parse_event_stream(lines, "t1", "m", 3);
        assert!(under.completed_under_turn_cap);

        let lines2 = FIXTURE.lines().map(|s| s.to_string());
        let over = parse_event_stream(lines2, "t1", "m", 2);
        assert!(!over.completed_under_turn_cap);
    }

    #[test]
    fn empty_stream_yields_error_state() {
        let lines = std::iter::empty::<String>();
        let result = parse_event_stream(lines, "t1", "m", 3);
        assert!(result.is_error);
        assert_eq!(result.terminal_reason, "no_result");
        assert!(result.bash_commands.is_empty());
    }

    #[test]
    fn resolve_path_absolute_is_passthrough() {
        let r = resolve_path("/tmp/fx", Path::new("/repo"));
        assert_eq!(r, PathBuf::from("/tmp/fx"));
    }

    #[test]
    fn resolve_path_relative_anchors_to_base() {
        let r = resolve_path("fixtures/fx", Path::new("/repo"));
        assert_eq!(r, PathBuf::from("/repo/fixtures/fx"));
    }

    #[test]
    fn resolve_path_env_var_is_passthrough() {
        let r = resolve_path("$HOME/fx", Path::new("/repo"));
        assert_eq!(r, PathBuf::from("$HOME/fx"));
    }

    #[test]
    fn ignores_malformed_json_lines() {
        let lines = [
            r#"{"type":"system","subtype":"init"}"#.to_string(),
            "not json".to_string(),
            "".to_string(),
            r#"{"type":"result","subtype":"success","is_error":false,"num_turns":1,"result":"ok","terminal_reason":"completed","total_cost_usd":0.001,"usage":{"input_tokens":1,"output_tokens":1}}"#.to_string(),
        ];
        let result = parse_event_stream(lines.into_iter(), "t1", "m", 3);
        assert_eq!(result.num_turns, 1);
        assert_eq!(result.final_text, "ok");
    }
}
