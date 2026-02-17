# Executable end-to-end fixture tests

This suite validates end-to-end `build`/`run` behavior of the Coppice CLI,
including process output/exit behavior and artifact output contracts.

See `tests/README.md` for shared fixture hierarchy and naming conventions.

## Fixture contract

```
tests/executable_end_to_end/<area>/<feature>/<case>/
  input/PACKAGE.coppice
  input/<files>.coppice
  invoke.args
  expect.exit
  expect.stdout
  expect.stderr
  expect.artifacts
```

## Content rules

- Prefer a small number of high-signal cases per feature.
- Each fixture is self-contained and should include `input/PACKAGE.coppice`.
- `invoke.args` is required and contains one CLI argument per line.
- `expect.exit`, `expect.stdout`, `expect.stderr`, and `expect.artifacts` are
  all required for every case.
- `expect.stdout` and `expect.stderr` must be exact output matches (runner trims
  one trailing newline from process output before compare).
- `expect.artifacts` lists expected output files. Use one path per line. If no
  artifacts are expected, the file must exist and be empty.
- `${TMP_OUTPUT_DIR}` and `${INPUT_DIR}` placeholders may be used in
  `invoke.args`, `expect.stdout`, `expect.stderr`, and `expect.artifacts`.

## Update snapshots

```sh
UPDATE_SNAPSHOTS=1 bazel run //tests/executable_end_to_end:executable_end_to_end_test
```
