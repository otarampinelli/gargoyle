---
name: Code Quality
description: Flag obvious code-quality problems in the diff
hints:
  - Use when a code change adds or reshapes implementation logic and you want a general quality pass.
  - refactors
  - new helpers or utilities
  - control-flow changes
  - debugging leftovers
  - dead code
---

**Output format:** Start with `RESULT: PASS` or `RESULT: FAIL`. For findings, use: `Finding: "[title]" | file.ts | line | Severity | Type`. Titles must be quoted. Severity: Error/Warning/Info. Type: Local (code fix) or Design (broader change). Quote special characters in titles.

Review the diff for code-quality problems. Fail the check if any of these are true:

- Debugging leftovers (`console.log`, `print`, `debugger`, commented-out blocks of code)
- Dead code: unused variables, imports, or functions introduced by this diff
- Functions that are clearly doing too much (very long, deeply nested)
- Magic numbers or strings that should be named constants
- Missing error handling at an obvious boundary (network call, file I/O, parsing)

If none of these issues are found, pass the check.
