# jig — Agent-Shape Testing Harness

## What this is

`jig` measures whether a CLI tool is shaped so that LLM agents reach
for the correct commands by default. The pattern is *runtime in the
loop*: a real agent runtime is spawned against a fixture and we
measure what it actually does.

The runtime today is `claude -p`; the framework is runtime-agnostic in
language and ready for other agents (GPT, Gemini, local models) once
the runner accepts a configurable spawn command.

It runs the agent subprocess against a fixture, feeding each one a
task from the subject tool's `agent-shape.toml` battery. It captures
the transcript, scores it with an LLM-as-judge, and reports first-try
command success, tokens per task, turns to completion, and
invented-command count.

## Build + test

```bash
cargo build --release
cargo test
cargo clippy -- -D warnings
```

CI: `nomograph/pipeline/rust-cli` component. `audit_allow_failure: false`.
`#![deny(warnings, clippy::all)]` at crate level.

## Architecture

Lib + binary crate. `lib.rs` re-exports modules for testing.

Design documents that drive this tool:

- `keaton/agent-shape-jig` spec (synthesist) — task plan, study design
- `keaton/synthesist-read-surface` spec (synthesist) — first subject

## Conventions

- No em dashes anywhere.
- Registry push (where applicable) is authoritative, not plain git
  commits. See `keaton/CLAUDE.md` for the shared discipline.
- Annotated tags + GitLab releases with notes on every release.
- Prescriptive errors: when `jig` rejects input, the error names the
  next action the user should take.

## Subject tools

A tool is a subject when it ships an `agent-shape.toml` at its repo
root. First subject: synthesist.

## Study methodology

Retrospective across tagged versions of a subject tool. `jig` runs the
current battery against each tag and plots score trajectory. Treatment
changes prove themselves by lifting scores vs historical baselines.

Primary metric: first-try command success rate.
Secondary: tokens to completion, turns to completion, invented-command
count. Cliff's delta for significance.

N is configurable (default 10 for iteration, 20+ for CI advisory).
Judge model is configurable (default Haiku 4.5).

Hold-out task support is in the schema from v1 (`tasks.holdout`);
corpus populates in v2.
