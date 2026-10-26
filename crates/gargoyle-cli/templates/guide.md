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

Run `gargoyle report <run_id> --format markdown`. Exit code 1 only means there are findings.

The user can't see command output, most agents fold it away. Copy the whole output into your reply, word for word, as markdown, not inside a code block. Never write "the report is above". Don't rewrite it, shorten it or add your own summary. If there are no findings, stop here.

## 4. Triage

The user decides what to fix, not you. Don't recommend, rank or group findings, and don't say which matters most. The report already explains each one.

Ask which to fix with one option per finding, named by its number and title, like "1. Status and sort are spliced into SQL". Let the user pick several at once. Use your question tool if it has multiple choice, and don't mark any option as recommended. Without one, ask in plain text: "Which should I fix? Reply with the numbers, like 1 and 3, or none." If the user writes their own instruction instead, follow it and show the result before moving on.

To fix one, take its `fix.patch` or `fix.description` from `gargoyle report <run_id> --format json`, where `findings` is in the same order as the numbers. Re-read the file and apply the change with your edit tools, lines may have moved since the review. Only touch files that were part of the review.
