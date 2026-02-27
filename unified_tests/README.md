# Unified Tests (Pilot)

Pilot suite for a unified fixture model rooted at:

`unified_tests/<area>/<feature>/<case>/`

Each case uses a script-style `case.test` file with literal CLI commands and
adjacent assertions.

## Case shape

```
unified_tests/<area>/<feature>/<case>/
  README.md
  input/...
  case.test
  expect/...
```

## `case.test` format

- `$ <command args...>` starts a run block.
- `> exit <code>` asserts exit code.
- `> stdout @<path>` asserts stdout from file.
- `> stderr @<path>` asserts stderr from file.
- `> text.stdout @<path>` / `> text.stderr @<path>` / `> text.exit <code>` are
  build text-channel assertions.
- `> json.stdout @<path>` / `> json.stderr @<path>` / `> json.exit <code>` are
  build json-channel assertions.
- `> artifacts @<path>` asserts artifact list from file.

Placeholders supported in commands and expected files:

- `${TMP_OUTPUT_DIR}`: per-run temporary output directory.
- `${INPUT_DIR}`: temporary copied `input/` working directory.

Artifact files are newline-delimited expected paths, with optional blank lines
and `#` comments.
