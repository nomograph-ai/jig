# Contributing

Thanks for your interest. `nomograph-jig` ships under the nomograph
estate and shares the common Rust contribution flow with `claim`,
`workflow`, `synthesist`, and `lattice`.

## Local checks

```sh
cargo test                                      # run the test suite
cargo fmt                                       # format the tree
cargo clippy --all-targets -- -D warnings       # lint (warnings are errors)
```

CI runs the same four stages (check, fmt, clippy, test) on every push
through the shared `nomograph/pipeline/rust-cli` component.

## House style

- No em dashes. They are an LLM tell, not nomograph house style.
- The crate root carries `#![deny(warnings, clippy::all)]`; do not
  add `#[allow(...)]` escape hatches without an inline justification
  comment.
- Source files split by concern. The current modules (`runner`,
  `judge`, `report`, `schema`, `checkpoint`) all sit at the top of
  `src/`. New surface earns a new module.

## Licensing

All contributions are accepted under the [MIT License](LICENSE). By
submitting a change you agree to license it under those terms.

## Architecture notes

`jig` is a library + binary crate. The library exposes the harness so
external callers can drive it programmatically; the binary is a thin
CLI on top.

The methodology and rubric anchors live in the wider estate:

- `keaton/research/synthesist-read-surface-audit.md`: end-to-end
  treatment study, including methodology lessons (rubric drift,
  judge IRR variance, fixture leakage).
- `synthesist/agent-shape.toml`: the production reference.
- `lever/canary/initial-results.md`: the precision-vs-brevity finding
  on judge prompts.
