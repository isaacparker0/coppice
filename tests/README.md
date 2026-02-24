# Tests

Fixture-driven test suites for the Coppice compiler and runtime contracts.

## Suites

1. `tests/diagnostics/`: frontend language diagnostics contract (`check`).
2. `tests/executable_end_to_end/`: runnable `build`/`run` end-to-end contract.

Suite-specific fixture rules, update commands, and scope live in each suite
README under `tests/<suite>/README.md`.

## Shared organization

All fixture suites use this hierarchy:

```
tests/<suite>/<area>/<feature>/<case>/
```

Naming guidance:

1. `<area>` is a broad subsystem boundary.
2. `<feature>` is a feature family under that area.
3. `<case>` is one scenario directory directly under `<feature>` (no extra
   nesting); use `minimal_valid` for canonical success.

## Case design principles

1. Keep each case self-contained under `input/`.
2. Prefer one behavior per non-`minimal_valid` case.
3. Name non-`minimal_valid` cases after the single behavior they validate.
4. Expectations must be stable and deterministic.
5. Avoid redundant near-duplicate fixtures.
