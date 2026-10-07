# Gargoyle

A framework for running automated code review checks against your local changes before you push.

```
         .:::.
        /o   o\
       (   >   )
        \     /
        /|   |\
       / |   | \

      Standing watch over your code.
```

## What is Gargoyle?

Gargoyle is a tool that orchestrates parallel code review checks. Each check is a markdown file containing review instructions that get executed by an AI reviewer against your uncommitted changes. Checks are decoupled from the runner, so you can add, remove, or modify them without touching the core logic.

The intent is simple: catch issues before you push, get instant feedback, and decide what to fix. No CI waiting, no PR review slowdown.

How it works:

1. Shell script gathers your diff and discovers available checks
2. AI reviewers run checks in parallel against your changes
3. Findings are deduplicated and presented as a summary
4. You triage findings: fix it, skip it, or provide a custom instruction
5. Gargoyle applies your choices to your working tree

All checks are opinionated. You can skip any finding or modify any check.

## Quick Start

### 1. Install Gargoyle

**Using npx (recommended):**

```bash
npx skills add otarampinelli/gargoyle
```

This installs Gargoyle and makes the `/gargoyle` skill available in Claude Code.

**Or manually:**

```bash
cd your-repo
cp -r /path/to/gargoyle .gargoyle
```

Or clone if you have it in git:

```bash
git clone <gargoyle-repo> .gargoyle
```

### 2. Run Gargoyle

Simply invoke the skill from Claude Code:

```bash
/gargoyle
```

The skill automatically gathers your changes and runs all recommended checks. Optionally, specify specific checks:

```bash
/gargoyle security-review code-quality
```

### 3. Triage findings

For each issue found, you can fix it, skip it, or provide a custom instruction. Gargoyle applies your choices to your working tree.

## Available Checks

Gargoyle comes with 5 built-in checks. See the files in `checks/` for their specific opinions and criteria.

- `security-review.md`
- `code-quality.md`
- `anti-slop.md`
- `test-coverage.md`
- `simplicity.md`

Each check has a `hints` field that tells Gargoyle when to recommend it. Run `/gargoyle` with no arguments to see all available checks and their descriptions.

## Adding Your Own Checks

Each check is a markdown file in `.gargoyle/checks/`. To add one:

1. Create `.gargoyle/checks/my-check.md`
2. Add frontmatter with name, description, and hints (see CONTRIBUTING.md)
3. Write review instructions that explain what to look for
4. Add output format guidance

See CONTRIBUTING.md for detailed guidelines and format rules.

## How It Works

### Runner

`runner.sh` is the setup script:

- Creates a unique temp directory (avoids collisions with parallel runs)
- Gathers your diff (with fallback logic: main > HEAD > working tree)
- Discovers all checks in `.gargoyle/checks/`
- Extracts check metadata (name, description, hints)
- Outputs environment variables for the skill to use
- Cleans up temp files on exit

### Skill

The Gargoyle skill (`SKILL.md`) orchestrates the review:

1. Verifies environment is set up
2. Optionally infers intent from your diff to recommend checks (uses hints field)
3. Decides which checks to run (all, or only the ones you requested)
4. Spawns AI reviewer agents in parallel for each check
5. Collects results and deduplicates findings
6. Presents a summary table
7. Triages findings one at a time (you decide what to fix)

### Output Format

Each check must follow this format:

- Start with `RESULT: PASS` or `RESULT: FAIL`
- Findings: `Finding: "[title]" | file.ts | line | Severity | Type`
- Severity: Error, Warning, or Info
- Type: Local (single-line code fix) or Design (broader changes)

See CONTRIBUTING.md for details.

## Design Principles

- Keep checks small and focused (one concern per file)
- Put review policy in checks, not in the runner
- The runner is stable; most changes go in `checks/`
- Checks are opinionated and meant to be overridden (you can skip any finding)
- No false positives if possible; err on the side of catching issues over silence

## Extending Gargoyle

Most customization happens in the `checks/` directory:

- Edit an existing check to change its criteria
- Add a new check to enforce a team standard
- Remove a check if it doesn't fit your workflow
- Use hints to control when checks are recommended

The runner and skill rarely need changes. See the Design Notes in SKILL.md.

## Troubleshooting

### Runner exits with "No changes to review"

Your diff is empty. Stage or commit your changes first.

### A check doesn't run

Check that its name or filename matches one of the files in `.gargoyle/checks/`. Run `/gargoyle` with no args to see all available checks.

### A finding looks wrong

Look at the diff that was reviewed. Gargoyle only examines changed lines; pre-existing issues are ignored. If you disagree with a check, edit it or skip the finding.

### I want to modify a check

Edit `.gargoyle/checks/check-name.md`. Changes take effect on the next run.

## License

MIT
