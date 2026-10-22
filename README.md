# Gargoyle

Provider-agnostic AI code review, before you push.

Gargoyle runs review checks against your local changes and hands you the findings. You decide what to fix. No CI waiting, no PR review slowdown.

> [!WARNING]
> **Work in progress.** Gargoyle is being rebuilt as a Rust CLI, and most of it doesn't exist yet. Commands, check formats and schemas can still change without notice, so don't depend on it for anything important yet.

## How It Works

1. Gargoyle collects your diff and picks the checks that apply to it.
2. An AI reviewer runs each check and returns its findings as JSON.
3. Gargoyle validates the findings, merges duplicates, and shows you a report.
4. You fix it, skip it, or tell it what to do instead.

Everything except the review itself is deterministic. The model only judges the code, so Gargoyle isn't tied to any one AI provider.

## Use It Today

With Claude Code:

```bash
npx skills add otarampinelli/gargoyle
```

Then run `/gargoyle` in your repo. To run specific checks, name them: `/gargoyle security-review code-quality`.

## CLI

The CLI isn't published yet and only covers the basics so far. Reviewing a diff from the CLI is coming next. For now, build it from source:

```bash
cargo build --release
./target/release/gargoyle --help
```

| Command | What it does |
|---|---|
| `gargoyle checks list` | Lists the available checks |
| `gargoyle checks validate` | Reports every problem in your check files |
| `gargoyle schema review-output` | Prints the JSON schema reviewers must return |
| `gargoyle schema check` | Prints the JSON schema for a check's frontmatter |

Checks are read from `.gargoyle/checks` by default. Use `--dir` to point somewhere else.

## Checks

Gargoyle ships with five: `security-review`, `code-quality`, `anti-slop`, `simplicity` and `test-coverage`. They're opinionated on purpose. Skip any finding, edit any check, or write your own.

A check is a markdown file named after its id:

```markdown
---
schema: gargoyle/check@1
id: performance
version: 1
name: Performance
description: Flag N+1 queries and unbounded loops
severity: warning
applies_to:
  globs: ["src/**"]
hints:
  - database queries
  - new loops
---

Report a finding for each N+1 query or loop over unbounded user input.
Some slowness is fine if it fixes a correctness problem first.

## Rules

- `n-plus-one`: a query inside a loop
- `unbounded-loop`: a loop over user input with no limit
```

Run `gargoyle checks validate` after editing. See [CONTRIBUTING.md](CONTRIBUTING.md) for the full format.

## Development

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

Conventions live in [docs/RUST_GUIDELINES.md](docs/RUST_GUIDELINES.md).

## License

MIT
