# Contributing

Thanks for your interest. `nomograph-jig` follows a standard Rust
contribution flow.

## Local checks

```sh
cargo test                                      # run the test suite
cargo fmt                                       # format the tree
cargo clippy --all-targets -- -D warnings       # lint (warnings are errors)
```

CI runs the same four stages (check, fmt, clippy, test) on every
push.

## House style

- No em dashes. They are an LLM tell.
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
