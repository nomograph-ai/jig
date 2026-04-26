# Changelog

All notable changes to `nomograph-jig` are recorded here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
the project follows semantic versioning.

## [0.1.0] (2026-04-26)

Initial public release. Bootstrapped fast to enable the agent-shape
methodology research; no breaking-change history before this point.

### Added

- `agent-shape.toml` schema (`subject`, `fixture`, `run`, `judge`,
  `tasks.tuning`, `tasks.holdout`, optional `commands.top_level`).
- `jig run`: spawns `claude -p --output-format stream-json` per
  `(task, model, trial_index)` cell, captures the transcript, scores
  it with an LLM-as-judge, and emits a JSON or Markdown report.
- `jig check`: validates an `agent-shape.toml` and, with `--binary`,
  cross-references the rubric's `commands.top_level` against the
  binary's `--help` output to catch rubric drift before it produces
  phantom inventions.
- `jig render`: rerenders a previously-emitted JSON report as
  Markdown with no API calls.
- `jig compare`: per-cell delta between baseline and treated reports,
  pure JSON-in / Markdown-out.
- `jig rejudge`: re-scores transcripts in a checkpoint against an
  updated rubric without re-running the agent trials. Supports
  resume from output checkpoint and skips judge-error cells with a
  summary.
- Trial checkpointing: every completed `(trial, verdict)` pair is
  appended as one JSON line so a killed run resumes without losing
  prior work.
- Subject-mismatch guard on `run`: `--subject <name>` aborts when the
  TOML's `subject.name` does not match, preventing the wrong rubric
  from being applied to a fixture.
- `[fixture].strip_env` field in `agent-shape.toml`: list of
  caller-side environment variables to remove before spawning the
  agent or fixture. Replaces an earlier hardcoded subject-specific
  strip list, so the runner is genuinely subject-agnostic. The
  default behavior strips known subject-tool environment variables
  before spawning so trials start from a clean slate.
- `examples/agent-shape.example.toml`: worked example.
  `templates/agent-shape.toml`: starter template with REPLACE-ME
  markers for new adopters.
- Library crate: `nomograph-jig` exposes `runner`, `judge`, `report`,
  `schema`, and `checkpoint` modules so callers can drive the harness
  programmatically.
- Project hygiene for the first tag: shared `rustfmt.toml` (edition
  2024, max width 100) and `clippy.toml` (MSRV 1.88); integration
  test suite covering `--version`, `--help`, `check`, `render`, and
  `compare` against the release binary; `Makefile` with `build` /
  `test` / `lint` / `check` / `install` verbs; `CONTRIBUTING.md`
  short-form contributor guide; `deny.toml` with a standard license
  allow-list; self-hosted `agent-shape.toml` so jig itself can be
  measured under the methodology it implements.
- Cargo metadata: `rust-version = "1.88"` pin and an `exclude` list
  so the published tarball drops CI config, local audit notes, and
  generated artifacts.

### Methodology

- Inter-rater reliability via optional `double_score`; the judge runs
  twice and the per-trial absolute delta is reported.
- Tuning vs holdout battery split is in the schema from v1; the
  holdout corpus is intentionally empty until independent authors
  populate it.

### Documentation

- README written to a higher information density: badges, install,
  quickstart, command reference, methodology pointer.
- CLAUDE.md with build verbs, release checklist, house style, and
  architecture notes.
- Em dashes purged from every shipped artifact: source comments,
  help text, templates, README, CLAUDE.md, and the Markdown report's
  empty-list placeholder.

[0.1.0]: https://gitlab.com/nomograph/jig/-/tags/v0.1.0
