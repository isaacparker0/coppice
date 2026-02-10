# Diagnostics fixtures

This directory contains compiler diagnostics fixtures. Every fixture uses the same
three-level layout:

```
tests/diagnostics/<area>/<feature>/<case>/
  input/main.lang0
  expect.txt
```

## Layout rules

1. `<area>` is a broad domain (e.g. `functions`, `expressions`, `variables`).
2. `<feature>` is the specific syntactic or semantic feature under test
   (e.g. `bindings`, `operators`, `if`, `declarations`).
3. `<case>` is the scenario. Use `minimal_valid` for the canonical positive case.
   Error cases should be named after the behavior (e.g. `duplicate_name`,
   `arity_mismatch`, `condition_not_boolean`).

## Content rules

- Each fixture is self-contained and uses a single file: `input/main.lang0`.
- `expect.txt` starts with `# exit: <code>` and the exact expected output after
  running `compiler/cli` with `check`.
- Avoid adding new case names for variations that can be covered by the
  existing `minimal_valid` fixture.
