---
name: Security Review
description: Flag common security issues in the diff
hints:
  - Use when a change touches auth, permissions, validation, external input handling, secrets, or sensitive data.
  - auth or permission changes
  - new endpoints
  - validation logic
  - external input handling
  - secrets or sensitive logging
---

**Output format:** Start with `RESULT: PASS` or `RESULT: FAIL`. For findings, use: `Finding: "[title]" | file.ts | line | Severity | Type`. Titles must be quoted. Severity: Error/Warning/Info. Type: Local (code fix) or Design (broader change). Quote special characters in titles.

Review the diff for security issues introduced by these changes. Fail the check if
you find any security risk, even if it is not listed below.

At minimum, check for issues like:

- Hardcoded API keys, tokens, passwords, or secrets in source files
- New API endpoints without input validation
- SQL queries built with string concatenation instead of parameterized queries
- Sensitive data (passwords, tokens, PII) logged to stdout or logs
- User-controlled input passed to shell commands, `eval`, or file paths without sanitization

This list is not exhaustive.

If no security issues are found, pass the check.
