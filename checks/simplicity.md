---
schema: gargoyle/check@1
id: simplicity
version: 1
name: Simplicity
description: Flag code structured more heavily than the problem warrants — wrong-fit data structures and over-engineering
severity: warning
applies_to:
  exclude:
    - "**/*.md"
hints:
  - Use when a change introduces new data structures, abstractions, or control flow and you want to check it is right-sized for the problem.
  - new data structures (maps, sets, classes, nested objects)
  - generic type params or options/config objects
  - elaborate control flow
  - speculative generality
---

Review the diff for solutions that are more complex than the problem they solve. Unlike
Anti-Slop (which targets low-signal filler like narrating comments and pass-throughs),
this check targets the *shape* of the solution: a data structure or abstraction that is
heavier or wrong for what the code actually does, even when every line carries signal.

Report a finding for each of these you find:

- **Wrong-fit data structure**: the container does not match how the data is used.
  Examples: a `Map`/keyed object that is only ever read with one known key or is built
  and then immediately iterated back into a list; a `Set` whose membership is never
  tested (only iterated); a wrapper object holding a single field that is always
  destructured at the point of use; parallel arrays kept in sync where one array of
  objects fits (or an array repeatedly scanned by id where a `Map`/record fits); an
  enum or discriminated union with only one variant.
- **Speculative generality**: generic type params, options/config objects, or function
  parameters that take exactly one value at every call site in the diff, with no second
  caller in sight. Recommend inlining the single concrete case.
- **Single-implementation abstraction with no state**: a class that holds no state and
  only groups functions, or a class instantiated once and used as a one-shot call —
  plain functions fit.
- **Over-elaborate control flow**: nested conditionals or loops that reduce to a direct
  expression, an early return, or a single `map`/`filter`/`find`. This includes
  hand-rolled logic that duplicates a standard built-in (a manual accumulation loop that
  is really a `map`, a hand-written clone, etc.).
- **Verbose value construction**: a multi-statement sequence with throwaway intermediate
  variables that builds a value a single clear expression (object literal, ternary,
  chained array method) would express more directly.

Do not report:

- Complexity that is genuinely proportional to a complex problem.
- Data structures chosen for a real, stated performance reason (e.g. a `Map`/`Set` for
  hot lookups over a large collection).
- Structures or abstractions that match an established pattern in the surrounding code,
  even if a lone instance would look heavy.
- A single-use intermediate variable that names a non-obvious value and aids readability.
- An abstraction with a second real caller or a concrete, imminent second use visible in
  the diff.

When you flag something, name the changed file and symbol, state which structure or
construct is oversized and how the code actually uses it, and describe the simpler shape
in prose (e.g. "replace the `Map<string, X>` keyed only by `id` with a plain array and
`find`").

If the solution's shape and data structures match the problem's actual complexity, return
no findings.

Most of these need more than a small patch, so report them as `design` findings unless the
fix is a small change to the changed lines.

## Rules

Set each finding's `rule` to one of these:

- `wrong-fit-structure`: a container that doesn't match how the data is used
- `speculative-generality`: generics, options or parameters with only one real value
- `stateless-class`: a class that only groups functions or is used once
- `elaborate-control-flow`: nested logic that reduces to something direct, or re-implements a built-in
- `verbose-construction`: many statements building a value one expression could express
- `other`: any other solution that is heavier than the problem
