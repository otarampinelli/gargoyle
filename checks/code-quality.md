---
schema: gargoyle/check@1
id: code-quality
version: 1
name: Code Quality
description: Flag obvious code-quality problems in the diff
severity: warning
applies_to:
  exclude:
    - "**/*.md"
hints:
  - Use when a code change adds or reshapes implementation logic and you want a general quality pass.
  - refactors
  - new helpers or utilities
  - control-flow changes
  - debugging leftovers
  - dead code
---

Review the diff for code-quality problems. Report a finding for each of these you find:

- Debugging leftovers (`console.log`, `print`, `debugger`, commented-out blocks of code)
- Dead code: unused variables, imports, or functions introduced by this diff
- Functions that are clearly doing too much (very long, deeply nested)
- Magic numbers or strings that should be named constants
- Missing error handling at an obvious boundary (network call, file I/O, parsing)

If you find none of these issues, return no findings.

## Rules

Set each finding's `rule` to one of these:

- `debug-leftover`: `console.log`, `print`, `debugger`, commented-out code
- `dead-code`: unused variables, imports or functions added by the diff
- `oversized-function`: a function that is very long or deeply nested
- `magic-value`: a number or string that should be a named constant
- `missing-error-handling`: no error handling at a network, file or parsing boundary
- `other`: any other clear code-quality problem
