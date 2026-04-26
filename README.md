# jig

[![pipeline](https://gitlab.com/nomograph/jig/badges/main/pipeline.svg)](https://gitlab.com/nomograph/jig/-/pipelines)
[![crates.io](https://img.shields.io/crates/v/nomograph-jig.svg)](https://crates.io/crates/nomograph-jig)
[![license](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![built with GitLab](https://img.shields.io/badge/built_with-GitLab-FC6D26?logo=gitlab)](https://gitlab.com/nomograph/jig)

Agent-shape testing harness. Runs runtime-in-the-loop task batteries
against a tool's CLI to measure first-try command success, tokens per
task, turns to completion, and invented-command count.

Each subject tool ships an `agent-shape.toml` declaring a fixture, a
battery of tasks (tuning + holdout), success criteria, and an LLM
judge rubric. `jig` spawns the agent runtime against the fixture,
records transcripts, scores them with an LLM-as-judge, and emits a
report. The runtime today is `claude -p`; the framework is
runtime-agnostic in language and ready for other agents (GPT, Gemini,
local models) once the runner accepts a configurable spawn command.

## Why this exists

Agents like Claude Opus and Sonnet reach for commands and arguments
that tools do not always provide. When an agent invents a non-existent
command or falls through to raw SQL or grep, that is a signal about
the tool's surface, not the agent. `jig` measures where that happens
so the tool can be reshaped.

The methodology is anchored in three nomograph findings:

- 5 worked examples are sufficient to recover near-ceiling accuracy
  (gkg-bench Phase 5).
- Precise-and-brief errors beat vague-and-verbose (lever canary
  bench).
- Removing one confusing tool can outweigh adding six (sysml-bench
  O12).

## Install

```bash
cargo install nomograph-jig
```

Or from source:

```bash
git clone https://gitlab.com/nomograph/jig.git
cd jig && make build
```

Requires Rust 1.88+ and `claude` on `$PATH` for any command that
spawns an agent (`run`, `rejudge`). `check`, `render`, and `compare`
are pure offline operations.

## Quickstart

```bash
# 1. Drop a starter agent-shape.toml into your tool's repo.
cp /path/to/jig/templates/agent-shape.toml ./agent-shape.toml
# Fill in the REPLACE-ME markers and add tasks.

# 2. Validate against your binary's --help so the rubric and the CLI
#    agree about which subcommands exist.
jig check agent-shape.toml --binary $(which your-tool)

# 3. Run a small smoke battery (writes a markdown report to stdout).
jig run agent-shape.toml --tuning-only --n 3

# 4. Run a real baseline. Checkpoint so a killed run resumes.
jig run agent-shape.toml --n 10 \
  --output baseline.json --format json \
  --checkpoint baseline.checkpoint.jsonl

# 5. Render the JSON as Markdown without re-spending API.
jig render baseline.json --output baseline.md

# 6. After a treatment, compare the two reports.
jig compare baseline.json treated.json --output delta.md
```

## Command Reference

| Command | What it does |
|---------|--------------|
| `jig run [path]` | Spawn the agent against the fixture, score every trial, emit a report. Supports `--tuning-only`, `--holdout-only`, `--n`, `--judge-model`, `--subject`, `--output`, `--format {json,markdown}`, `--checkpoint`. |
| `jig check [path] [--binary <bin>]` | Parse the TOML and (optionally) cross-reference `[commands].top_level` with `<binary> --help`. Reports drift in either direction. |
| `jig render <json>` | Re-emit a previously-saved JSON report as Markdown. No API calls. |
| `jig compare <before.json> <after.json>` | Per-cell delta table (mean score, completion rate, tokens, turns, invented commands). No API calls. |
| `jig rejudge <toml> --from <ckpt> --to <ckpt>` | Re-score the trial transcripts in a checkpoint against an updated rubric. Costs judge tokens, not agent tokens. Supports resume. |

`jig --help` and `jig <subcommand> --help` are the authoritative
reference; this table tracks the surface as of v0.1.0.

## How a study runs

The `agent-shape.toml` declares everything the harness needs:

- `[subject]`: tool name, binary, description, optional `version_pin`
  for retrospective runs against tagged versions.
- `[fixture]`: idempotent setup script, optional cleanup, working
  directory the agent operates in. Setup runs before every trial so
  state is isolated.
- `[run]`: trials per cell (`n`), agent models under test, turn cap,
  per-trial wall-clock timeout.
- `[judge]`: judge model (default Haiku 4.5), `double_score` for IRR,
  rubric prose, required JSON fields.
- `[tasks.tuning]` and `[tasks.holdout]`: task IDs, prompts, success
  criteria, and provenance (`author`, `created_at`,
  `sealed_against_tag`).
- `[commands].top_level` (optional): subcommands the rubric claims
  exist; `jig check --binary` cross-references this with the CLI.

`examples/agent-shape.example.toml` is the worked example targeting
synthesist; `templates/agent-shape.toml` is the starter for new
adopters. The schema lives in [`src/schema.rs`](src/schema.rs).

## Methodology notes

- **Rubric drift** is the dominant source of measurement error.
  Twice in the synthesist study the rubric missed real commands and
  the judge counted them as inventions, producing phantom regressions.
  `jig check --binary` mechanically catches the binary side; rubric
  prose still has to be hand-maintained. Land subcommand changes and
  rubric updates in the same commit.
- **Judge variance**: typical IRR delta is 0.05 to 0.30 per cell at
  n=5. Don't draw conclusions from sub-0.20 effects without n>=20 or
  Cliff's delta significance testing.
- **Fixture leakage**: the runner strips `SYNTHESIST_*` environment
  variables before spawning, but every subject tool needs to do the
  same in its own fixture script.
- **Holdout corpus**: tuning-only studies overfit to the designer's
  tasks. The schema supports `tasks.holdout` from v1; the corpus
  populates in v2 once independent authors who haven't seen the
  tuning data write tasks against the same surface.

## Building

```bash
make build    # release binary, copied to ./jig
make test     # build + run all tests
make lint     # cargo clippy --all-targets -- -D warnings
make fmt      # cargo fmt
make check    # build + smoke test --help and check on the example
```

## Library use

`nomograph-jig` is a library crate as well as a binary. The
`runner`, `judge`, `report`, `schema`, and `checkpoint` modules are
public so callers can drive the harness programmatically without
shelling out to the CLI.

## License

MIT. See [LICENSE](LICENSE).
