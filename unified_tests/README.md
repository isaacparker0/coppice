# Unified Tests (Pilot)

Pilot suite for a unified fixture model rooted at:

`unified_tests/<area>/<feature>/<case>/`

Each case uses a script-style `case.test` file with literal CLI commands.

## Case shape

```
unified_tests/<area>/<feature>/<case>/
  README.md
  input/...
  case.test
  expect/...
```

## `case.test` format

- Each non-empty non-comment line is one run command.
- Use either:
  - `<command args...>`
  - `[label] <command args...>`
- Labels use `[A-Za-z0-9_]`.
- If a command appears once in a case, a label is not allowed.
- If a command appears multiple times in a case, each occurrence must have a
  unique label.

Expectation files are convention-based under `expect/`, inferred from either the
command name (single occurrence) or label (multiple occurrences):

- `build`:
  - `<stem>.text.stdout`
  - `<stem>.json.stdout`
  - `<stem>.stderr`
  - `<stem>.artifacts`
  - required `<stem>.exit`
  - optional `<stem>.text.exit` / `<stem>.json.exit` to override `<stem>.exit`
- `run` / `fix`:
  - `<stem>.stdout`
  - `<stem>.stderr`
  - required `<stem>.exit`
- `run` also requires:
  - `<stem>.artifacts`

Placeholders supported in commands and expected files:

- `${TMP_OUTPUT_DIR}`: per-run temporary output directory.
- `${INPUT_DIR}`: temporary copied `input/` working directory.

Artifact files are newline-delimited expected paths, with optional blank lines
and `#` comments.
