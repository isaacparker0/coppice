# Diagnostics fixture tests

Every fixture uses a three-level layout:

```
tests/diagnostics/<area>/<feature>/<case>/
  input/PACKAGE.coppice
  input/<files>.coppice
  expect.text
  expect.json
```

## Layout rules

1. `<area>` is a broad language subsystem (syntax or semantic policy), for
   example `declarations`, `statements`, `expressions`, `naming`, `file_roles`,
   `packages`, `pipeline`.
2. `<feature>` is the feature family under that area (e.g. `imports`,
   `control_flow`, `literals`, `bindings`).
3. `<case>` is the scenario. Use `minimal_valid` for the canonical positive case
   for that feature. Error cases should be named after the behavior (e.g.
   `duplicate_name`, `return_type_mismatch`, `unterminated_string`,
   `if_condition_not_boolean`).

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
- `minimal_valid` is the only success case for a feature and should cover all
  valid sub-kinds for that feature in one file.
- `expect.text` is the exact expected output from
  `compiler/cli check --format text`.
- `expect.json` is the exact expected output from
  `compiler/cli check --format json`.
- Avoid adding new case names for variations that can be covered by the existing
  `minimal_valid` fixture.
- Non-`minimal_valid` fixtures must produce exactly one `: error:` diagnostic in
  `expect.text`; in `expect.json` they must contain exactly one diagnostics
  entry or one top-level `error` object (but not both) to keep case intent
  obvious and avoid diagnostic cascades.
- Name error cases after the single behavior they validate (for example,
  `duplicate_name`, `return_type_mismatch`, `if_condition_not_boolean`).
