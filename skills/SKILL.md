---
name: gargoyle
description: Runs review checks from .gargoyle/checks against local changes, summarizes results, and triages findings. Use when the user says "/gargoyle", "run gargoyle", or "run the checks" before pushing.
---

# gargoyle - Check Runner

Run review checks from `.gargoyle/checks/*.md` against local changes.
By default, run all checks. If the user names one or more checks, run only those.

Before running this skill, execute `.gargoyle/runner.sh` to gather diff, log, and discover checks.

Keep the runner generic and keep repo-specific rules inside `.gargoyle/checks/*.md`.

## Workflow

### 1. Verify setup

Source environment from `runner.sh`:

```bash
export GARGOYLE_TMP=...  # temp dir from runner.sh
export GARGOYLE_DIFF=...
export GARGOYLE_LOG=...
export GARGOYLE_CHECKS_META=...
```

If any env var is missing, ask the user to run `.gargoyle/runner.sh` first.

Derive available checks from `GARGOYLE_CHECKS_META` (format: `path|name|description`, one per line).
Convert check names to display names (e.g., `security-review` → `Security Review`).

If the user requested specific checks, filter the list to matching checks.
Accept filename stem (e.g., `security-review`), display name (e.g., `Security Review`), or unambiguous partial match (e.g., `security`).

If a name matches multiple checks, ask which one. If a check doesn't exist, list available checks and stop.

### 2. Infer intent and recommend checks (optional)

Only do this step if the user did not explicitly name which checks to run.

Do a lightweight intent pass over the changed paths, diff, and commit log to infer the change type.
Produce:
1. a short summary of likely intent
2. a recommendation for which checks to run
3. a confidence judgment: narrow, mixed, or unclear

**Recommendation algorithm:**
- For each check, read its `name`, `description`, and `hints` from the metadata.
- Include the check if any hint matches the inferred intent category OR if the description contains keywords from changed paths.
- If fewer than 2 checks match, recommend running all checks instead (fallback threshold).
- If more than 5 files touched, recommend all checks (broad change fallback).
- If the inferred intent is unclear (mixed/sprawling change), recommend all checks.

Always print the recommendation before proceeding:

```text
Likely intent: fix security validation in auth handler.
Recommended checks: Security Review, Code Quality.
Confidence: narrow.
```

### 3. Decide scope

Use the recommended checks from step 2 if the user didn't specify.
Otherwise, run exactly the checks the user named.

Print the final list of checks that will run.

### 4. Run checks in parallel (background sub-agents)

For each check file, spawn a sub-agent with:

- `subagent_type: "general-purpose"`
- `run_in_background: true`

Use this prompt structure:

```
You are a code reviewer running an automated check on a set of changes.

## Read first
1. Read the check instructions from: {absolute path from GARGOYLE_CHECKS_META}
2. Read the diff from: $GARGOYLE_DIFF
3. Read the commit log from: $GARGOYLE_LOG

## Review rules
- Only review behavior introduced or changed in the diff
- Do not flag pre-existing problems in unchanged code
- Prefer working from the diff alone; read only the sections relevant to your check
- If the diff is not enough to make a reliable judgment, you may read a small
  amount of nearby context, but only when necessary

## Output contract

Return findings in compact markdown. Optimize for token efficiency while remaining parseable.

**If the check passes:**

```
RESULT: PASS
One sentence why it passed.
```

**If the check fails:**

```
RESULT: FAIL
One sentence summary of what failed.

---

Finding: "[title]" | file.ts | line | Severity | Type
One or two sentences explaining what is wrong and why it matters.
Fix: One or two sentences on how to solve it.

```diff
- old code
+ new code
```

---

Finding: "[another]" | file.ts | line | Severity | Type
Problem explanation.
Fix: Fix description.
```

Rules:
- Start with `RESULT: PASS` or `RESULT: FAIL` (no markdown headers)
- Titles must be quoted: `Finding: "[title]"` to handle special characters
- Format: `Finding: "[title]" | file | line | Severity | Type`
  - Severity: Error, Warning, or Info
  - Type: Local (single-line fix) or Design (broader changes)
- Local findings include a `\`\`\`diff` block; Design findings do not
- Separate findings with `---`
- Keep explanations concise and token-efficient
```

Launch ALL sub-agents in a single message (all Agent tool calls together).

### 5. Collect results & deduplicate

After all agents complete, parse each agent's response.

For each agent response:

1. Look for `RESULT: PASS` or `RESULT: FAIL`
2. Extract each finding by parsing `Finding: "[title]" | file | line | severity | type` lines
3. Extract the explanation (prose before "Fix:") and fix description (after "Fix:")
4. For Local findings, extract the diff block (between ` ```diff ` markers)
5. For Design findings, there is no code block

Parsing should be lenient — use regex to split on `|`, ignore whitespace, tolerate slight format variations.

**Deduplicate findings:** Merge findings that point to the same file + line + root issue into ONE finding.
Record which checks flagged it and keep a single code suggestion.
Do NOT merge findings that are genuinely different problems, even in the same file.

### 6. Summarize results (ALWAYS — before any triage)

Print this table before doing anything else. Do not rely on sub-agent completion status.
The only source of truth is the PASS/FAIL line in each agent's output.

List one row per deduplicated finding. A check that passed gets a single ✅ row.

```
🛡️  gargoyle — N checks

| Finding                                   | Flagged by                  |
|-------------------------------------------|-----------------------------|
| ❌ Hardcoded DB password — app.module.ts:12 | Security Review, Code Quality |
| ✅ Security Review — passed                | Security Review             |

X findings across Y checks.
```

If every check passed, say so explicitly and stop.

### 7. Triage local findings

Triage each deduplicated finding at a time. Each distinct problem gets its own
AskUserQuestion so the user decides independently.

**Render each finding as a card in your message FIRST (CommonMark), then ask via AskUserQuestion.**
The card uses colored diffs; the question widget renders as monochrome. Both together keep the flow clear.

Card structure:

```
### 🔴 Hardcoded DB password
`app.module.ts:925` · flagged by Security Review + Code Quality

Password is committed in source instead of read from the environment.

```diff
-     password: '1223456789',
+     password: process.env.PG_PASSWORD,
```
---
```

- Heading: `### <emoji> <short title>` (🔴 Error, 🟡 Warning, 🔵 Info)
- Context: `` `<file>:<line>` · flagged by <checks> ``
- Problem (one sentence)
- Diff block (for Local findings; Design findings skip this)
- Separator: `---`

AskUserQuestion structure: organize the question with findings grouped, file/line on the first line, then the suggestion:

```
1. Hardcoded DB password at app.module.ts:925

Switch to process.env.PG_PASSWORD?

-     password: '1223456789',
+     password: process.env.PG_PASSWORD,
```

Header: `<file> — flagged by <checks>`
Options:
- **Fix it** — apply the code suggestion
- **Skip** — leave it as is

If the user picks "Other" with a custom instruction (e.g., "use DB_PASS instead"), re-read the file, apply a best-effort fix based on the instruction, show the updated code, and ask for confirmation before applying.

Batch up to 4 findings per AskUserQuestion; use multiple calls if there are more.
Print all findings + diffs first, then ask. Execute each choice — fix, skip, or custom instruction.
Only edit changed files; never touch pre-existing issues in unchanged code.

## Design Notes

- Keep the runner stable — most changes go in `.gargoyle/checks/*.md`, not in SKILL.md.
- Keep checks small and focused.
- `runner.sh` handles all I/O and setup; this skill focuses on orchestration and judgment.
- Deduplication merges findings at the same file + line + issue; different issues at the same line stay separate.
