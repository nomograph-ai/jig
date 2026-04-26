//! Integration tests for the `jig` CLI.
//!
//! Each test runs the release binary as a subprocess via assert_cmd.
//! Pure offline subcommands (`--version`, `check`, `render`, `compare`)
//! are exercised end-to-end. `run` and `rejudge` need `claude` in the
//! loop and are out of scope for the test suite; their wiring is
//! covered by unit tests in the lib modules.

use std::fs;

use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;
use tempfile::TempDir;

fn jig() -> Command {
    Command::cargo_bin("jig").expect("cargo bin for jig")
}

fn write(path: &std::path::Path, body: &str) {
    fs::write(path, body).expect("write fixture file");
}

const MIN_TOML: &str = r#"
[subject]
name = "demo"
binary = "demo"
description = "Demo tool used in jig integration tests."

[fixture]
setup = "true"
workdir = "/tmp"

[run]
n = 1
models = ["claude-sonnet-4-6"]
turn_cap = 3
timeout_seconds = 30

[judge]
model = "claude-haiku-4-5"
double_score = false
rubric = "Score on 0..1; respond strict JSON."
required_fields = ["score", "first_command", "first_command_existed", "completed", "invented_commands", "fallback_to_sql", "reasoning"]

[[tasks.tuning]]
id = "t1"
summary = "exploration"
prompt = "ask the tool what is going on"
success_criteria = ["agent uses real commands"]
author = "test@example.com"
created_at = "2026-04-25"
sealed_against_tag = "demo-v0.0.0"

[commands]
top_level = ["alpha", "beta"]
"#;

#[test]
fn version_flag_prints_a_version() {
    jig()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("jig "));
}

#[test]
fn help_lists_every_subcommand() {
    let assert = jig().arg("--help").assert().success();
    let out = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    for cmd in &["run", "check", "render", "compare", "rejudge"] {
        assert!(out.contains(cmd), "--help missing subcommand {cmd}: {out}");
    }
}

#[test]
fn check_accepts_minimal_config() {
    let dir = TempDir::new().unwrap();
    let toml = dir.path().join("agent-shape.toml");
    write(&toml, MIN_TOML);
    jig()
        .args(["check", toml.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("OK: demo"))
        .stdout(predicate::str::contains("1 tuning"));
}

#[test]
fn check_rejects_missing_file() {
    jig()
        .args(["check", "/tmp/jig-does-not-exist-9d1f.toml"])
        .assert()
        .failure();
}

#[test]
fn check_with_binary_warns_on_drift() {
    // Use /bin/echo as a stand-in binary. `echo --help` does not have
    // a `Commands:` block, so the rubric's two top_level entries
    // become extra-in-rubric drift.
    let dir = TempDir::new().unwrap();
    let toml = dir.path().join("agent-shape.toml");
    write(&toml, MIN_TOML);
    let assert = jig()
        .args(["check", toml.to_str().unwrap(), "--binary", "/bin/echo"])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        stderr.contains("rubric lists") || stderr.contains("subcommand(s)"),
        "expected drift warning on stderr, got: {stderr}"
    );
}

const SAMPLE_REPORT_JSON: &str = r#"{
  "subject": "demo",
  "version_pin": null,
  "run_timestamp": "unix:0",
  "judge_model": "claude-haiku-4-5",
  "tuning": {
    "n_trials": 2,
    "mean_score": 0.75,
    "completion_rate": 1.0,
    "mean_tokens": 150.0,
    "mean_turns": 2.0,
    "total_invented_commands": 0,
    "total_fallback_to_sql": 0
  },
  "holdout": {
    "n_trials": 0,
    "mean_score": null,
    "completion_rate": null,
    "mean_tokens": null,
    "mean_turns": null,
    "total_invented_commands": 0,
    "total_fallback_to_sql": 0
  },
  "cells": [
    {
      "section": "tuning",
      "task_id": "t1",
      "model": "claude-sonnet-4-6",
      "n": 2,
      "mean_score": 0.75,
      "score_stddev": 0.0,
      "mean_tokens": 150.0,
      "mean_turns": 2.0,
      "invented_commands": [],
      "fallback_count": 0,
      "mean_irr_delta": null
    }
  ]
}"#;

const TREATED_REPORT_JSON: &str = r#"{
  "subject": "demo",
  "version_pin": null,
  "run_timestamp": "unix:1",
  "judge_model": "claude-haiku-4-5",
  "tuning": {
    "n_trials": 2,
    "mean_score": 1.0,
    "completion_rate": 1.0,
    "mean_tokens": 120.0,
    "mean_turns": 1.5,
    "total_invented_commands": 0,
    "total_fallback_to_sql": 0
  },
  "holdout": {
    "n_trials": 0,
    "mean_score": null,
    "completion_rate": null,
    "mean_tokens": null,
    "mean_turns": null,
    "total_invented_commands": 0,
    "total_fallback_to_sql": 0
  },
  "cells": [
    {
      "section": "tuning",
      "task_id": "t1",
      "model": "claude-sonnet-4-6",
      "n": 2,
      "mean_score": 1.0,
      "score_stddev": 0.0,
      "mean_tokens": 120.0,
      "mean_turns": 1.5,
      "invented_commands": [],
      "fallback_count": 0,
      "mean_irr_delta": null
    }
  ]
}"#;

#[test]
fn render_emits_markdown_for_a_report() {
    let dir = TempDir::new().unwrap();
    let json = dir.path().join("report.json");
    write(&json, SAMPLE_REPORT_JSON);
    let assert = jig()
        .args(["render", json.to_str().unwrap()])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    assert!(stdout.contains("# agent-shape report: demo"));
    assert!(stdout.contains("## Tuning battery"));
    assert!(stdout.contains("mean_score: 0.750"));
}

#[test]
fn render_writes_to_output_path() {
    let dir = TempDir::new().unwrap();
    let json = dir.path().join("report.json");
    let md = dir.path().join("report.md");
    write(&json, SAMPLE_REPORT_JSON);
    jig()
        .args([
            "render",
            json.to_str().unwrap(),
            "--output",
            md.to_str().unwrap(),
        ])
        .assert()
        .success();
    let body = fs::read_to_string(&md).expect("read rendered md");
    assert!(body.contains("# agent-shape report: demo"));
}

#[test]
fn compare_emits_per_cell_delta() {
    let dir = TempDir::new().unwrap();
    let before = dir.path().join("before.json");
    let after = dir.path().join("after.json");
    write(&before, SAMPLE_REPORT_JSON);
    write(&after, TREATED_REPORT_JSON);
    let assert = jig()
        .args(["compare", before.to_str().unwrap(), after.to_str().unwrap()])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    assert!(stdout.contains("agent-shape comparison: demo vs demo"));
    assert!(stdout.contains("mean_score | 0.750 | 1.000 | +0.250"));
    assert!(stdout.contains("Per-cell deltas"));
    assert!(stdout.contains("| tuning | t1 | claude-sonnet-4-6 |"));
}

#[test]
fn run_rejects_subject_mismatch_without_calling_claude() {
    // run will fail at fixture setup if it gets that far; the
    // subject-mismatch guard fires before any spawn so this test does
    // not need claude installed.
    let dir = TempDir::new().unwrap();
    let toml = dir.path().join("agent-shape.toml");
    write(&toml, MIN_TOML);
    jig()
        .args([
            "run",
            toml.to_str().unwrap(),
            "--subject",
            "not-the-real-subject",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("subject mismatch"));
}
