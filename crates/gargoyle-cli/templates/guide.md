# Reviewing Changes With Gargoyle

The `gargoyle` CLI does the predictable work: it collects the changes, picks the checks, writes one prompt per review task, validates every answer and builds the report. Your job is to run the tasks and help the user triage.

## 1. Prepare

Run `gargoyle prepare --json`. If the user named checks, add `--checks <id>,<id>`.

- If it fails, show the error and stop. `gargoyle init` sets a repo up.
- If `tasks` is empty, tell the user there's nothing to review and stop.

## 2. Review

Run each task with this prompt. If you can spawn sub-agents, give each task its own and start them all at once. Otherwise do the tasks yourself, one by one.

> Read `<prompt_path>` and do the review it describes. Write only the JSON answer it asks for to `<answer_path>`, then run `gargoyle ingest <run_id> <task id> <answer_path>`. If it prints errors, fix the file and run it again until it prints `ok`. Reply with its last line only.

Wait for every task to finish before going on.

## 3. Report

Run `gargoyle report <run_id>` and show its output as is. Exit code 1 only means there are findings. If every check passed, say so and stop.

## 4. Triage

Run `gargoyle report <run_id> --format json` and go through `findings` in order. Show each one as a short card:

- Heading: 🔴 error, 🟡 warning or 🔵 info, then the title
- `path:start_line` and the checks that flagged it
- The message, then `fix.patch` in a `diff` code block, or `fix.description` for design findings

Then ask the user, a few findings at a time: **Fix it** or **Skip**. Use your question tool if you have one. If the user writes their own instruction instead, follow it and show the result before moving on.

To fix, re-read the file and apply the change with your edit tools, lines may have moved since the review. Only touch files that were part of the review.
