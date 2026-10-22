---
schema: gargoyle/check@1
id: test-coverage
version: 1
name: Test Coverage
description: Flag new logic that ships without tests
severity: warning
applies_to:
  exclude:
    - "**/*.md"
hints:
  - Use when a change adds meaningful behavior, fixes a bug, or changes logic that should be exercised by tests.
  - new logic
  - bug fixes
  - branching behavior
  - changed handlers or workflows
  - missing nearby test updates
---

Review the diff for meaningful logic that was added or changed without accompanying
tests. The goal is to catch untested behavior, not to demand tests for everything.
Flag meaningful coverage gaps even when they are not explicitly listed below.

Scope is strictly limited to code touched by the current diff:

- Only evaluate behavior introduced or changed in this PR
- Ignore pre-existing test gaps that are not part of this diff
- Do not ask for tests in unrelated files, modules, or historical code paths
- If a touched file has adjacent legacy logic, only comment on the changed behavior

Different parts of a codebase may use different test stacks.
Treat coverage as satisfied when the change is covered by the most appropriate
test type for that code path.
Do not require every change to include both unit and e2e tests.

Report a finding when meaningful behavior introduced or changed by the diff is not
adequately exercised by tests.

Common examples of inadequate coverage (non-exhaustive):

- New or changed meaningful behavior has no test coverage for the affected path
- A bug fix has no regression test that would have caught the original bug
- Coverage only exercises happy paths while meaningful failure, error, or edge-case
  behavior introduced by the change remains untested

No action is needed when:

- The change is pure config, types, comments, formatting, or renames
- The change is trivial pass-through with no logic to test
- A test file _was_ added/updated alongside the change and covers the new behavior
- The touched area does not have an established automated test setup; in this
  case, report it with severity `info` instead

When you flag something, name the specific function or behavior that lacks a test and
describe the case that should be covered (e.g. "`processPayment()` has no test for the
declined-card path").

If new logic is adequately covered, return no findings.

When suggesting or evaluating tests, prefer:

- Minimal mocking of internal implementation details when behavior can be tested through public interfaces
- Assertions that verify observable behavior and outcomes, not implementation trivia
- Test descriptions that clearly state scenario and expected result
- Coverage for both success and non-happy paths when the changed logic introduces both

In each finding, call out exactly which behavior is untested and name one
concrete test that should be added in the existing test style for that area.

## Rules

Set each finding's `rule` to one of these:

- `untested-behavior`: new or changed behavior with no test
- `missing-regression-test`: a bug fix with no test that would have caught the bug
- `untested-failure-path`: only the happy path is tested; failure or edge cases are not
- `other`: any other meaningful coverage gap
