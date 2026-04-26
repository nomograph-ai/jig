# Changelog

All notable changes to `nomograph-jig` are recorded here. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
the project follows semantic versioning.

## [Unreleased]

### Added

- Project tidy pass before the first non-bootstrap tag: shared
  `rustfmt.toml` (edition 2024, max width 100) and `clippy.toml`
  (MSRV 1.88) matching the wider nomograph estate; integration test
  suite covering `--version`, `--help`, `check`, `render`, and
  `compare` against the release binary; `Makefile` mirroring synthesist
  verbs (`build`, `test`, `lint`, `check`, `install`); `CONTRIBUTING.md`
  short-form contributor guide; `deny.toml` with the standard nomograph
  license allow-list; self-hosted `agent-shape.toml` so jig itself can
  be measured under the methodology it implements.
- Cargo metadata: `rust-version = "1.88"` pin and an `exclude` list so
  the published tarball drops CI config, local audit notes, and
  generated artifacts.

### Changed

- README rewritten with the synthesist density target: badges,
  install, quickstart, command reference, methodology pointer.
- CLAUDE.md rewritten to match the rest of the estate (build verbs,
  release checklist, no em dashes one-liner, methodology pointer).
- Em dashes purged from every shipped artifact: source comments, help
  text, templates, README, CLAUDE.md, and the Markdown report's
  empty-list placeholder.

## [0.1.0] (2026-04-24)

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
- Strips `SYNTHESIST_*` environment variables before spawning the
  agent or fixture commands, preventing the developer's session
  context from contaminating trials.
- `examples/agent-shape.example.toml`: worked example targeting
  `synthesist`. `templates/agent-shape.toml`: starter template with
  REPLACE-ME markers for new adopters.
- Library crate: `nomograph-jig` exposes `runner`, `judge`, `report`,
  `schema`, and `checkpoint` modules so callers can drive the harness
  programmatically.

### Methodology

- Inter-rater reliability via optional `double_score`; the judge runs
  twice and the per-trial absolute delta is reported.
- Tuning vs holdout battery split is in the schema from v1; the
  holdout corpus is intentionally empty until independent authors
  populate it.

[Unreleased]: https://gitlab.com/nomograph/jig/-/compare/v0.1.0...main
[0.1.0]: https://gitlab.com/nomograph/jig/-/tags/v0.1.0
