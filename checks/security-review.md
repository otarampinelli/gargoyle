---
schema: gargoyle/check@1
id: security-review
version: 1
name: Security Review
description: Flag common security issues in the diff
severity: error
hints:
  - Use when a change touches auth, permissions, validation, external input handling, secrets, or sensitive data.
  - auth or permission changes
  - new endpoints
  - validation logic
  - external input handling
  - secrets or sensitive logging
---

Review the diff for security issues introduced by these changes. Report a finding for
any security risk you find, even if it is not listed below.

At minimum, check for issues like:

- Hardcoded API keys, tokens, passwords, or secrets in source files
- New API endpoints without input validation
- SQL queries built with string concatenation instead of parameterized queries
- Sensitive data (passwords, tokens, PII) logged to stdout or logs
- User-controlled input passed to shell commands, `eval`, or file paths without sanitization

This list is not exhaustive.

If you find no security issues, return no findings.

## Rules

Set each finding's `rule` to one of these:

- `hardcoded-secret`: API keys, tokens, passwords or other secrets in source
- `missing-input-validation`: external input used without validation
- `sql-injection`: queries built from strings instead of parameters
- `sensitive-logging`: secrets or personal data written to logs or output
- `command-injection`: user input reaching a shell, `eval` or similar
- `path-traversal`: user input used in file paths without sanitization
- `other`: any other security risk
