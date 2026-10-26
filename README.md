# Gargoyle

Provider-agnostic AI code review, before you push.

Gargoyle runs review checks against your local changes and hands you the findings. You decide what to fix. No CI waiting, no PR review slowdown.

> [!WARNING]
> **Work in progress.** Gargoyle is being rebuilt as a Rust CLI. Commands, check formats and schemas can still change without notice, so don't depend on it for anything important yet.

## How It Works

1. Gargoyle collects your diff, finds the code around it, and picks the checks that apply.
2. An AI reviewer runs each check and returns its findings as JSON.
3. Gargoyle validates the findings, merges duplicates, and shows you a report.
4. You fix it, skip it, or tell it what to do instead.

Everything except the review itself is deterministic. The model only judges the code, so Gargoyle isn't tied to any one AI provider.

Reviewers don't get a bare diff. They see every function you changed in full, a short outline of the rest of each file, and the code your change affects: where the changed functions are used, the tests that use them, and what your change calls. That's how a reviewer catches a caller that still passes the old arguments.

Want to know why it works this way? Every choice is explained in [docs/DECISIONS.md](docs/DECISIONS.md).

## Use It Today

Install the CLI (macOS and Linux):

```bash
curl -fsSL https://raw.githubusercontent.com/otarampinelli/gargoyle/main/install.sh | sh
```

Gargoyle isn't published yet, so this builds it from source and you need [Rust](https://rustup.rs). Prebuilt binaries are coming.

Then set up your repo:

```bash
cd your-repo
gargoyle init
```

This creates a `gargoyle.toml`, copies the built-in checks into `.gargoyle/checks` so you can edit them, and adds the `/gargoyle` skill to your agents. Run it again any time, it only fills in what's missing.

Now run `/gargoyle` in your agent. To run specific checks, name them: `/gargoyle security-review code-quality`.

### How Your Agent Runs It

Any agent that can run shell commands can run a review: Claude Code, Codex, Gemini CLI, Cursor, OpenCode or your own. It works in four steps:

1. `gargoyle prepare --json` writes one review prompt per task.
2. The agent runs each prompt, ideally in parallel sub-agents, and saves the JSON answer.
3. `gargoyle ingest <run> <task> <answer>` checks each answer. If it's invalid, the agent fixes it and tries again.
4. `gargoyle report <run>` shows the findings, and the agent helps you fix them.

`gargoyle guide` prints these steps, and the skill just tells your agent to follow it. Adding the skill needs Node.js. Without it, or if your agent has no skills support, add one line to its instructions, like `AGENTS.md`: "To review changes, run `gargoyle guide` and follow it."

Gargoyle reviews everything that differs from your base branch: commits, staged and unstaged changes, and new files. Lockfiles, minified files and binaries are skipped.

The first review indexes your repo, so it takes a bit longer. After that, only the files you changed are read again. The index lives in `.gargoyle/index` and stays out of git. It covers TypeScript, JavaScript, Python, Go, Rust, Java, C#, Ruby, PHP, C, C++, Lua, Swift and Elixir. Files in other languages still get reviewed, just without the code around them.

### Uninstall

Remove the CLI:

```bash
cargo uninstall gargoyle-cli
```

To remove gargoyle from a repo too, delete `gargoyle.toml` and `.gargoyle/`, and remove the skill:

```bash
npx skills remove gargoyle
```

## CLI

Gargoyle doesn't call a model itself yet. Your agent runs the review tasks, and the CLI does everything else.

| Command | What it does |
|---|---|
| `gargoyle init` | Sets up your repo: config, checks and the `/gargoyle` skill. `--no-skill` skips the skill |
| `gargoyle guide` | Prints the review steps for your agent |
| `gargoyle prepare` | Collects your changes and writes one review prompt per task |
| `gargoyle ingest <run> <task> <file>` | Checks a reviewer's answer and keeps it |
| `gargoyle report <run>` | Shows the findings. Exits with 1 when one fails the review |
| `gargoyle diff` | Prints the changes a review would cover |
| `gargoyle checks list` | Lists the available checks |
| `gargoyle checks validate` | Reports problems in your check files |
| `gargoyle schema <name>` | Prints a JSON schema: `review-output`, `check`, `config`, `plan` or `report` |

## Config

`gargoyle.toml` controls what gets reviewed and when a review fails. Every setting is optional.

```toml
[review]
base = "main"            # what to compare your changes against
fail_on = "error"        # "info", "warning", "error" or "never"
min_confidence = 0.6     # hide findings the reviewer isn't sure about
tasks = "grouped"        # all checks in one task, or "per-check" for one task per check

[files]
exclude = ["dist/**"]    # files never to review

[checks]
dir = ".gargoyle/checks" # where your checks live

[context]
callers = true           # show reviewers where your changed functions are used
tests = true             # and the tests that use them
guidelines = ["docs/STYLE.md"] # docs every reviewer follows, none by default

[[instructions]] # extra rules for some files, repeat for more
path = "src/api/**"
text = "Every endpoint must validate its input."
```

## Checks

Gargoyle ships with six: `correctness`, `security-review`, `code-quality`, `anti-slop`, `simplicity` and `test-coverage`. They're opinionated on purpose. Skip any finding, edit any check, or write your own.

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
