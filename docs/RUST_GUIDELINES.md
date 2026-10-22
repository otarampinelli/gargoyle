# Rust Guidelines

Conventions for the Gargoyle codebase. Lints enforce most of these; the rest are review rules.

## Workspace
- Dependency versions live only in the root `Cargo.toml` (`[workspace.dependencies]`). Crates use `foo.workspace = true`.
- Every crate has `[lints] workspace = true`. Don't relax lints per crate; if a lint is wrong, discuss changing it at the root.
- Add a crate only when a step needs it. No empty crates.

## Errors
- Identifiers get newtypes (`CheckId`, `Fingerprint`), not bare `String`.
- User-written formats (checks, config) use `#[serde(deny_unknown_fields)]` so typos fail loudly.
- Contract types derive `JsonSchema`; published schemas are generated, never hand-written.

## Determinism
- Anything that reaches output uses `BTreeMap`/`BTreeSet` or a sorted `Vec`. Never iterate a `HashMap` into output.
- The same inputs must produce byte-identical output. Snapshot tests guard this.

## Visibility
- Modules are private; the public API is chosen with `pub use` in `lib.rs`.
- Use `pub(crate)` for anything not needed by other crates.

## Tests
- Unit tests live next to the code in `#[cfg(test)] mod tests`.
- CLI behaviour and exit codes: `crates/gargoyle-cli/tests/` with `assert_cmd`.
- Rendered output (reports, prompts, SARIF): `insta` snapshots.
- Test names describe behaviour: `rejects_unknown_values`, not `test_severity_2`.

## Docs
- `//!` at the top of each crate explains what it owns and what it must not do.
- `///` on public items explains *why*, not what the signature already says.

## Before pushing
```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```