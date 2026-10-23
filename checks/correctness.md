---
schema: gargoyle/check@1
id: correctness
version: 1
name: Correctness
description: Flag logic bugs the diff introduces
severity: error
applies_to:
  exclude:
    - "**/*.md"
hints:
  - Use when a change touches conditions, calculations, parsing, data handling or async code.
  - new or changed conditions
  - arithmetic or rounding
  - parsing and data conversion
  - async code
  - changes to existing behavior
---

Review the diff for logic bugs introduced by these changes: code that runs but does the wrong thing.
Report a finding when you can name a concrete input or situation where the new code gives a wrong
result, crashes, or behaves differently than the change clearly means to.

Look for issues like:

- Conditions that are inverted, too broad, too narrow, or miss a case
- Off-by-one errors in loops, ranges, slices or indexes
- Arithmetic that loses precision, overflows, or rounds where it shouldn't
- `null`, `undefined`, empty or missing values that aren't handled
- Async code that isn't awaited, or that can race
- Values converted to the wrong type, unit or format
- Behavior that changes for existing callers when the change doesn't seem to mean it
- Code that contradicts its own comment, name or tests

For each finding, say what breaks: the input, what the code does, and what it should do instead.

Don't report:

- Style, naming, structure, security or missing tests. Other checks cover those.
- Anything you can't tie to a concrete failing case. A guess is not a finding.
- Bugs in code the diff didn't change.

If you find no logic bugs, return no findings.

## Rules

Set each finding's `rule` to one of these:

- `wrong-condition`: a condition that is inverted, too broad, too narrow or misses a case
- `off-by-one`: a loop, range, slice or index that is one off
- `precision-loss`: arithmetic that loses precision, overflows or rounds wrongly
- `missing-null-check`: a `null`, empty or missing value that isn't handled
- `async-misuse`: a missing `await`, an unhandled promise or a race
- `wrong-conversion`: a value converted to the wrong type, unit or format
- `unintended-change`: behavior that changes for existing callers without meaning to
- `contradicts-intent`: code that disagrees with its own comment, name or tests
- `other`: any other logic bug with a concrete failing case
