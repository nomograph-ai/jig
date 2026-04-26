# jig

Agent-shape testing harness. Rust + clap + TOML + LLM-as-judge over
`claude -p`.

## Build

```
make build    # compile release binary, copy to ./jig
make test     # build + run all tests
make lint     # cargo clippy --all-targets -- -D warnings
make fmt      # cargo fmt
make check    # build + smoke test --help and check on the example
cargo build   # dev build
cargo test    # unit + integration tests
```

## Architecture

Library + binary crate. `src/lib.rs` re-exports the modules so callers
can drive the harness programmatically; `src/main.rs` is a thin clap
shell on top.

Modules, one concern each:

- `schema`: `agent-shape.toml` deserialization.
- `runner`: spawn `claude -p`, capture stream-json, build `TrialResult`.
- `judge`: build the judge prompt, invoke the judge subprocess, parse
  the JSON verdict, optional double-score for inter-rater reliability.
- `report`: aggregate `(trial, verdict)` pairs into a `Report`, emit
  JSON or Markdown.
- `checkpoint`: append-only JSONL of completed cells so killed runs
  resume.

Methodology and rubric anchors live outside this repo:

- `keaton/research/synthesist-read-surface-audit.md`: end-to-end
  treatment study with the corrected baseline-vs-treated comparison
  and methodology lessons.
- `synthesist/agent-shape.toml`: production reference TOML.
- `lever/canary/initial-results.md`: precision-vs-brevity finding on
  judge prompts.

## Conventions

- **No em dashes**, anywhere. Source comments, help text, README,
  CHANGELOG, commit messages, docstrings. They are an LLM tell.
- **Lever compliance**: `#![deny(warnings, clippy::all)]` at the
  crate root. No `#[allow(...)]` escape hatches without an inline
  justification comment.
- **File size**: keep source files focused on one concern. Modules
  split before files grow into `cmd_*` collections.
- **Tests**: unit tests inline in each module; integration tests in
  `tests/integration.rs` exercise the release binary as a subprocess.
- **Output**: `--format json` is the canonical machine output;
  Markdown is for human review and review-rendering. JSON round-trips
  through `jig render` with no API calls.
- **Single verification**: `make build && make test && make lint`
  before committing.

## Subject tools

A tool is a subject when it ships an `agent-shape.toml` at its repo
root. First subject: synthesist. New adopters use
`templates/agent-shape.toml` as a starter; the worked example lives at
`examples/agent-shape.example.toml`.

## Study methodology

Retrospective across tagged versions of a subject tool. `jig` runs the
current battery against each tag and plots the score trajectory.
Treatment changes prove themselves by lifting scores vs historical
baselines.

Primary metric: first-try command success rate. Secondaries: tokens
to completion, turns to completion, invented-command count. Cliff's
delta for significance.

`n` is configurable (default 10 for iteration, 20+ for CI advisory).
Judge model is configurable (default Haiku 4.5). Hold-out task
support is in the schema from v1 (`tasks.holdout`); corpus populates
in v2 once independent authors write tasks against the same surface.

## Release Checklist

Before tagging a release:

1. `make build && make test && make lint` (all pass locally).
2. Push to main; CI pipeline must pass.
3. README.md, CHANGELOG.md, CLAUDE.md all reflect the release content.
4. `git tag -a vX.Y.Z -m "release notes"` (annotated, with notes).
5. `git push --tags`; wait for tag CI to pass.
6. `glab release create vX.Y.Z --notes "release notes"`.

Never tag before CI passes. Never tag with stale documentation.
Never skip release notes; both the annotated tag and the GitLab
release must have them.
