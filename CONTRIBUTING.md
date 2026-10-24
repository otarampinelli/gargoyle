# Contributing

Want to help? Great. There are two ways in: write a check, or work on the engine.

Gargoyle is a work in progress, so things move fast and formats can still change. If you're planning something big, open an issue first so we can agree on the approach.

## Writing a Check

Add a markdown file to `checks/`. The file name must match the check's `id`, so `performance` lives in `checks/performance.md`.

### Frontmatter

| Field | Required | Default | What it is |
|---|---|---|---|
| `schema` | Yes | | Always `gargoyle/check@1` |
| `id` | Yes | | Lowercase letters, digits and single hyphens |
| `version` | Yes | | Starts at `1`. Bump it when the instructions change |
| `name` | Yes | | Human-readable name |
| `description` | Yes | | One sentence on what the check catches |
| `severity` | No | `warning` | `error`, `warning` or `info` |
| `applies_to.globs` | No | `["**/*"]` | Files this check reviews |
| `applies_to.exclude` | No | | Files to skip, even if they match `globs` |
| `hints` | No | | Kinds of change this check matters most for. Not used yet, it will help rank checks |
| `context.mode` | No | `hunk` | `hunk`, `function` or `file`. For now `function` and `file` work like `hunk` |
| `min_confidence` | No | | Drops findings below this, from `0.0` to `1.0` |

Unknown fields are rejected, so typos fail loudly. The full format is in [schemas/check.v1.json](schemas/check.v1.json).

### Instructions

Everything after the frontmatter is what the reviewer reads. Tell it:

- What to report, with concrete examples.
- What not to report. Be explicit about this too.
- When to return no findings.

End with a `## Rules` list that gives each kind of problem a short, stable slug. Findings use it to stay consistent across runs.

Don't describe the output format. The engine adds it.

### Guidelines

- **Keep it focused.** One concern per check. If it catches five different things, split it into five checks.
- **Be opinionated, not pedantic.** A check that always passes is useless. A check that complains about minor style is noise.
- **Keep it portable.** Checks run on any repo, so don't rely on one project's structure.
- **Don't duplicate.** If Security Review already catches hardcoded secrets, don't add another check for it.
- **Explain the trade-offs.** If a check is strict on purpose, say why.

### Testing It

```bash
gargoyle checks validate
```

Then review a change that should pass and one with a real issue, running only your check:

```bash
gargoyle prepare --checks performance
```

Run the prompts with your agent, or use `/gargoyle performance`. You should get no findings for the first change and a finding for the second.

## Working on the Engine

You need Rust. The toolchain version is pinned in `rust-toolchain.toml`, so `rustup` picks it up for you.

Before pushing:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

- Conventions live in [docs/RUST_GUIDELINES.md](docs/RUST_GUIDELINES.md).
- If you change a type behind a file in `schemas/`, regenerate it with `UPDATE_SCHEMAS=1 cargo test -p gargoyle-core --test schemas`.

## Commits

Keep them short, lowercase and in the past tense, with a type prefix:

```
feat: added check spec parsing
feat: added checks and schema commands to cli
docs: added rust guidelines
refactor: migrated checks to check@1 format
```

One logical change per commit. Every commit should build and pass the tests.

That's it. Open a PR when you're ready, the template tells you what to include.
