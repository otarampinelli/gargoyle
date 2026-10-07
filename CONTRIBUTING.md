# Contributing Checks to Gargoyle

Want to add a new check? Great. Keep it focused, clear, and opinionated.

## Create a Check File

Add a markdown file to `.gargoyle/checks/my-check.md`.

### Frontmatter

Every check starts with YAML frontmatter:

```yaml
---
name: My Check Name
description: One sentence describing what this check catches
hints:
  - Use when a change touches [pattern]
  - specific pattern 1
  - specific pattern 2
---
```

The `name` field is how users reference your check (`/gargoyle my-check`).

The `hints` field helps the intent inference system recommend your check when it sees matching patterns. Write hints as concrete patterns (e.g., "auth logic changes", "new API endpoints"), not generic ones like "testing".

### Review Instructions

After the frontmatter, write clear instructions for a reviewer. Tell them:

- What to look for (concrete examples)
- When to fail the check (specific conditions)
- When to pass (be explicit about this too)

Be opinionated. A check that always passes is useless.

### Example

```yaml
---
name: Performance Review
description: Flag N+1 queries and unbounded loops
hints:
  - Use when a change touches database queries or loops
  - new loops
  - database queries
  - changes to data-access logic
---

Review the diff for performance issues introduced by this change.

Fail the check if you find:
- N+1 database queries (a loop that queries the database in each iteration)
- Unbounded loops over user-provided input
- Quadratic algorithms on large collections (nested loops where outer collection size is not bounded)

Pass the check if the code looks efficient for its use case.

Understand that some slowness is acceptable if it solves a correctness problem first.
```

## Output Format

Your check output must follow this format exactly. The runner and skill parse it strictly.

### If the check passes:

```
RESULT: PASS
One sentence explaining why it passed.
```

### If the check fails:

```
RESULT: FAIL
One sentence summary of what failed.

---

Finding: "[title here]" | file.ts | 42 | Error | Local
Explanation of the problem and why it matters.
Fix: How to solve it.

```diff
- old line
+ new line
```

---

Finding: "[another issue]" | file.ts | 50 | Warning | Design
Problem explanation.
Fix: Prose explanation of how to fix this.
```

### Format Rules

- Always start with `RESULT: PASS` or `RESULT: FAIL` (exactly this, no markdown headers)
- Each finding starts with `Finding: "[title]" | file | line | Severity | Type`
  - Titles must be quoted to handle special characters (pipes, quotes, etc.)
  - Severity: Error, Warning, or Info
  - Type: Local (single-line code fix) or Design (broader changes, new files, config)
- For Local findings, include a `\`\`\`diff` block with before/after
- For Design findings, just prose (no diff block)
- Separate findings with `---`
- Keep explanations concise and token-efficient

### Adding the Format Note

At the top of your check file (after frontmatter), add:

```
**Output format:** Start with `RESULT: PASS` or `RESULT: FAIL`. For findings, use: `Finding: "[title]" | file.ts | line | Severity | Type`. Titles must be quoted. Severity: Error/Warning/Info. Type: Local (code fix) or Design (broader change).
```

## Testing Your Check

Before submitting:

1. Make sure the check runs and produces output in the correct format
2. Verify it catches the issues you intend to catch
3. Test it on a small diff that should pass (it outputs `RESULT: PASS`)
4. Test it on a diff with a real issue (it outputs `RESULT: FAIL` with findings)

## Guidelines

- Keep checks focused. One concern per file. If your check catches 5 different things, split it into 5 checks.
- Be opinionated but not pedantic. A check that complains about minor style is noise.
- Avoid checks that require project-specific context. Gargoyle runs on any diff; checks should work without knowing the codebase structure.
- Don't duplicate existing checks. If Security Review already catches hardcoded secrets, don't add another check for it.
- Document the trade-offs. If your check is strict on purpose, explain why (e.g., "We err on the side of blocking untested code because we've had bugs").

## Adding to the Repo

1. Create your check in `.gargoyle/checks/your-check.md`
2. Test it locally
3. Commit with a message: "Add your-check review check"
4. Open a PR or add it to your project's gargoyle setup

That's it. The runner will auto-discover it on the next run.
