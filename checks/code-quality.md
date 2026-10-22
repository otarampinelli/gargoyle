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

Review the diff for code-quality problems. Fail the check if any of these are true:

- Debugging leftovers (`console.log`, `print`, `debugger`, commented-out blocks of code)
- Dead code: unused variables, imports, or functions introduced by this diff
- Functions that are clearly doing too much (very long, deeply nested)
- Magic numbers or strings that should be named constants
- Missing error handling at an obvious boundary (network call, file I/O, parsing)

If none of these issues are found, pass the check.
