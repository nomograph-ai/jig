# jig adoption template

Drop these files into your tool's repo to adopt the agent-shape
methodology.

## What's here

- `agent-shape.toml`: starter rubric + runner + task config. Fill
  in the `REPLACE-ME` markers and your tasks.

## What you also need to write

`scripts/agent-shape-fixture.sh`: a script that wipes and rebuilds
your tool's working state to a known starting point. Idempotent.
Strips inherited env vars to avoid leaking the developer's session
into a trial.

See `examples/agent-shape.example.toml` in this repo for a worked
fixture/task layout you can mirror in your own repo.

## Adoption checklist

1. Copy `agent-shape.toml` to your repo root. Replace markers.
2. Write `scripts/agent-shape-fixture.sh` that seeds realistic state.
3. Add `[commands].top_level` entries that match `<your-tool> --help`.
4. `jig check agent-shape.toml --binary $(which your-tool)`,
   which should report 0 drift.
5. `jig run agent-shape.toml --tuning-only --n 5` for a smoke run
   (~$2-5 in API depending on agent model + task length).
6. Inspect the report. Iterate the rubric and tasks until trials
   are scoring meaningfully.
7. Run a real baseline at `--n 10` or `--n 20`. Check it in as
   `agent-shape-reports/baseline-<your-tool>-vX.Y.Z-nN.{json,md}`.

## What to watch for

- **Rubric staleness.** When a rubric misses real commands, the
  judge counts them as inventions and produces phantom regressions.
  `jig check --binary` flags it; `[commands].top_level` is the
  contract. Land subcommand changes and rubric updates in the same
  commit.
- **Judge variance.** Typical IRR delta is 0.05-0.30 per cell at
  n=5. Don't draw conclusions from sub-0.20 effects without n>=20
  or Cliff's delta significance testing.
- **Fixture leakage.** If your tool reads env vars (e.g.
  `<TOOL>_DIR`, `<TOOL>_SESSION`), strip them in the fixture
  script. Otherwise the caller's local state contaminates trials.
- **Hold-out corpus.** Tuning-only studies overfit to the designer's
  tasks. Plan for hold-out tasks authored by someone who hasn't seen
  the tuning data.
