# jig

Agent-shape testing harness. Runs runtime-in-the-loop task batteries
against a tool's CLI to measure first-try command success, tokens per
task, turns to completion, and invented-command count.

Each subject tool ships an `agent-shape.toml` declaring a fixture, a
battery of tasks (tuning + holdout), success criteria, and an LLM judge
rubric. `jig` spawns the agent runtime against the fixture, records
transcripts, scores them with an LLM-as-judge, and emits a report.

The runtime today is `claude -p`; the framework is runtime-agnostic in
language and ready for other agents (GPT, Gemini, local models) once
the runner accepts a configurable spawn command.

## Status

End-to-end runnable: schema, runner, judge, report, CLI, checkpointing,
render. First baseline measured against synthesist v5.1.0.

## Build

```bash
cargo build --release
cargo test
cargo clippy -- -D warnings
```

## Install

```bash
cargo install nomograph-jig
```

## Why

Agents like Claude Opus and Sonnet reach for commands and arguments that
tools do not always provide. When an agent invents a non-existent
command or falls through to raw SQL, that is a signal about the tool's
surface, not the agent. `jig` measures where that happens so the tool
can be reshaped.

Anchored in nomograph's own prior findings: 5 worked examples are
sufficient to recover near-ceiling accuracy (gkg-bench Phase 5),
precise-and-brief errors beat vague-and-verbose (lever canary bench),
and removing one confusing tool can outweigh adding six (sysml-bench
O12).

## License

MIT. See [LICENSE](LICENSE).
