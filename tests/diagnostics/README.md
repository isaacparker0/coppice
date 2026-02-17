# Diagnostics fixture tests

This suite validates the frontend diagnostics contract exposed by
`coppice check`.

See `tests/README.md` for shared fixture hierarchy and naming conventions.

## Fixture contract

```
tests/diagnostics/<area>/<feature>/<case>/
  input/PACKAGE.coppice
  input/<files>.coppice
  expect.text
  expect.json
```

## Content rules

- Each fixture is self-contained and should use a clear role-oriented default
  filename under `input/`: prefer `lib.coppice` for library/general cases,
  `lib.test.coppice` for test-role cases, `main.bin.coppice` for binary-role
  cases, and `PACKAGE.coppice` for manifest-role cases; these are defaults for
  clarity, not hard constraints, and multi-file fixtures may use other
  filenames/layouts as needed.
- The fixture runner invokes `check` from `input/` (equivalent to `check .`), so
  each `input/` directory should contain `PACKAGE.coppice` to define the package
  root. The intentional exception is workspace-root error fixtures that validate
  missing-root-manifest behavior.
- `expect.text` is the exact expected output from
  `compiler/cli check --format text`.
- `expect.json` is the exact expected output from
  `compiler/cli check --format json`.
- Non-`minimal_valid` fixtures must produce exactly one `: error:` diagnostic in
  `expect.text`; in `expect.json` they must contain exactly one diagnostics
  entry or one top-level `error` object (but not both) to keep case intent
  obvious and avoid diagnostic cascades.

## Update snapshots

```sh
UPDATE_SNAPSHOTS=1 bazel run //tests/diagnostics:diagnostics_test
```
