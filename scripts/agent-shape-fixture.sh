#!/bin/sh
# Idempotent fixture for the jig self-test agent-shape battery.
#
# Stands up a tiny demo subject under fixtures/jig-self-test/ that the
# agent can poke at: a TOML, a sample report, and a sample treated
# report. The fixture wipes and rebuilds on every trial so state is
# isolated across runs.
#
# Strips inherited env vars that could leak the developer's session
# context into a trial.

set -eu

unset SYNTHESIST_SESSION
unset SYNTHESIST_DIR

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FIXTURE="$ROOT/fixtures/jig-self-test"

rm -rf "$FIXTURE"
mkdir -p "$FIXTURE"

# Minimal agent-shape.toml the agent can validate or compare against.
cat > "$FIXTURE/agent-shape.toml" <<'TOML'
[subject]
name = "demo"
binary = "demo"
description = "Demo subject for jig self-test."

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
author = "self-test"
created_at = "2026-04-25"
sealed_against_tag = "demo-v0.0.0"

[commands]
top_level = ["alpha", "beta"]
TOML

# A baseline JSON report the agent can render or compare.
cat > "$FIXTURE/baseline.json" <<'JSON'
{
  "subject": "demo",
  "version_pin": null,
  "run_timestamp": "unix:0",
  "judge_model": "claude-haiku-4-5",
  "tuning": {
    "n_trials": 1, "mean_score": 0.5, "completion_rate": 1.0,
    "mean_tokens": 100.0, "mean_turns": 2.0,
    "total_invented_commands": 0, "total_fallback_to_sql": 0
  },
  "holdout": {
    "n_trials": 0, "mean_score": null, "completion_rate": null,
    "mean_tokens": null, "mean_turns": null,
    "total_invented_commands": 0, "total_fallback_to_sql": 0
  },
  "cells": [
    {
      "section": "tuning", "task_id": "t1", "model": "claude-sonnet-4-6",
      "n": 1, "mean_score": 0.5, "score_stddev": 0.0,
      "mean_tokens": 100.0, "mean_turns": 2.0,
      "invented_commands": [], "fallback_count": 0,
      "mean_irr_delta": null
    }
  ]
}
JSON

# A treated JSON report so `jig compare` has two ends to subtract.
cat > "$FIXTURE/treated.json" <<'JSON'
{
  "subject": "demo",
  "version_pin": null,
  "run_timestamp": "unix:1",
  "judge_model": "claude-haiku-4-5",
  "tuning": {
    "n_trials": 1, "mean_score": 1.0, "completion_rate": 1.0,
    "mean_tokens": 80.0, "mean_turns": 1.0,
    "total_invented_commands": 0, "total_fallback_to_sql": 0
  },
  "holdout": {
    "n_trials": 0, "mean_score": null, "completion_rate": null,
    "mean_tokens": null, "mean_turns": null,
    "total_invented_commands": 0, "total_fallback_to_sql": 0
  },
  "cells": [
    {
      "section": "tuning", "task_id": "t1", "model": "claude-sonnet-4-6",
      "n": 1, "mean_score": 1.0, "score_stddev": 0.0,
      "mean_tokens": 80.0, "mean_turns": 1.0,
      "invented_commands": [], "fallback_count": 0,
      "mean_irr_delta": null
    }
  ]
}
JSON
