# jig adoption template

Drop these files into your tool's repo to adopt the agent-shape
methodology.

## What's here

- `agent-shape.toml` — starter rubric + runner + task config. Fill
  in the `REPLACE-ME` markers and your tasks.

## What you also need to write

`scripts/agent-shape-fixture.sh` — a script that wipes and rebuilds
your tool's working state to a known starting point. Idempotent.
Strips inherited env vars to avoid leaking the developer's session
into a trial.

See `synthesist/scripts/agent-shape-fixture.sh` in the nomograph
estate for a worked example (creates trees, specs, sessions in a
synthesist instance under `fixtures/agent-shape-realistic/`).

## Adoption checklist

1. Copy `agent-shape.toml` to your repo root. Replace markers.
2. Write `scripts/agent-shape-fixture.sh` that seeds realistic state.
3. Add `[commands].top_level` entries that match `<your-tool> --help`.
4. `jig check agent-shape.toml --binary $(which your-tool)` —
   should report 0 drift.
5. `jig run agent-shape.toml --tuning-only --n 5` for a smoke run
   (~$2-5 in API depending on agent model + task length).
6. Inspect the report. Iterate the rubric and tasks until trials
   are scoring meaningfully.
7. Run a real baseline at `--n 10` or `--n 20`. Check it in as
   `agent-shape-reports/baseline-<your-tool>-vX.Y.Z-nN.{json,md}`.

## What to watch for

- **Rubric staleness.** Twice in the synthesist study the rubric
  missed real commands and the judge counted them as inventions.
  `jig check --binary` flags it; `[commands].top_level` is the
  contract. Land subcommand changes and rubric updates in the same
  commit.
- **Judge variance.** Typical IRR delta is 0.05-0.30 per cell at
  n=5. Don't draw conclusions from sub-0.20 effects without n≥20
  or Cliff's delta significance testing.
- **Fixture leakage.** If your tool reads env vars (e.g.
  `<TOOL>_DIR`, `<TOOL>_SESSION`), strip them in the fixture
  script AND in the runner (jig already does this for synthesist).
  Otherwise the developer's local state contaminates trials.
- **Hold-out corpus.** v1 tuning-only studies overfit to the
  designer's tasks. Plan for v2 hold-out tasks authored by someone
  who hasn't seen the tuning data.

## Background reading

- `keaton/research/synthesist-read-surface-audit.md` — full study
  end-to-end, including the corrected-baseline-vs-treated comparison
  and methodology lessons.
- `synthesist/agent-shape.toml` — production reference.
- `lever/canary/initial-results.md` — the precision-vs-brevity finding
  on judge prompts (precise short rubrics outperform vague verbose
  ones).
