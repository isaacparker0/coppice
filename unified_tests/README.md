# Unified Tests

Unified fixture suite for CLI contract coverage across `build`, `run`, and
`fix`.

Use this README as the authoritative fixture policy for adding new tests.

## Layout

Each case lives at:

`unified_tests/<feature>/<case>/`

Required case files:

```
unified_tests/<feature>/<case>/
  README.md
  input/...
  case.test
  expect/...
```

Path ownership is scenario-first:

- Place each case under the feature family that primarily owns the behavior.
- If a case exercises multiple concerns, place it under the family that owns the
  asserted contract, not the setup mechanism used to trigger it.
- Use package/import families when package-boundary mechanics are the contract
  (for example export visibility, import resolution, package wiring, or
  transitive import behavior). Keep cases under language-feature families when
  imports/cross-file setup are only context for asserting that feature's
  semantics.

## Naming, Placement, And Intent

- A top-level directory under `unified_tests/` is a coherent contract family.
- Top-level families may be language-facing (`functions`, `operators`,
  `control_flow`) or non-language (`build_pipeline`, `workspace_root`,
  `file_roles`).
- Do not split top-level directories by command (`build`/`run`/`fix`).
- Keep all cases for a family together; command selection belongs in
  `case.test`.

- Case names must describe input scenario facts, not outcomes.
- Do not encode outcome words in names (`valid`, `fails`, `blocks`).
- Do not encode command choice in case names (`build`, `run`, `fix`, `runtime`)
  unless command-phase behavior is itself the contract being tested.
- Prefer scenario-first names (`list_index_assignment_out_of_range`).
- Prefer a consistent slug shape: `subject_rule_variant` (for example
  `variable_name_not_camel_case`, `variable_name_double_underscore`).
- `minimal_valid` may be used if there is a truly minimal case for the feature.

Each case directory must include `README.md` with one plain contract sentence.

Required format:

- plain prose only (no heading/list/code formatting)
- must end with `.`

Required content:

- states the contract being locked
- adds meaning beyond the case slug
- describes what is being validated

Preferred verbiage:

- use direct contract language in present tense
- use behavior-first wording (`X can...`, `X must...`, `X cannot...`)

Examples:

- `Function declarations accept typed parameters.`
- `Generic functions cannot be used as values without instantiation.`

## `case.test` Commands

Each non-empty non-comment line is one literal CLI invocation.

Allowed forms:

- `<command args...>`
- `[label] <command args...>`

Rules:

- supported commands are `build`, `run`, and `fix`
- labels may only use `[A-Za-z0-9_]`
- if a command appears once in a case, label is not allowed
- if a command appears multiple times, each occurrence must have a unique label

Expectation stems:

- single occurrence uses command name (`build`, `run`, `fix`)
- repeated occurrence uses explicit label

## Expected Files

Expectation files are convention-based under `expect/`.

For `build` runs:

- required: `<stem>.text.stdout`
- required: `<stem>.json.stdout`
- required: `<stem>.text.stderr`
- required: `<stem>.json.stderr`
- required: `<stem>.artifacts`
- required: `<stem>.exit`
- optional: `<stem>.text.exit`
- optional: `<stem>.json.exit`

If format-specific exit files are absent, both formats use `<stem>.exit`.

For `run` runs:

- required: `<stem>.stdout`
- required: `<stem>.stderr`
- required: `<stem>.artifacts`
- required: `<stem>.exit`

For `fix` runs:

- required: `<stem>.stdout`
- required: `<stem>.stderr`
- required: `<stem>.exit`

General file policy:

- keep required files even when output is empty
- do not represent "no output" by omitting required files

Artifact files are newline-delimited expected paths. Blank lines and `#`
comments are allowed.

## Placeholders

The runner supports these placeholders in command args and expected files:

- `${TMP_OUTPUT_DIR}`: per-run temporary output directory
- `${INPUT_DIR}`: temporary copied case `input/` working directory

`build` and `run` output-directory plumbing is runner-owned. Do not pass per-run
output-directory flags in `case.test`.

## Command Ownership

- `build` runs own diagnostics/reporting and build-path contracts.
- `run` runs own runtime/stdout/stderr/exit behavior and run-path gating.
- `fix` runs own `fix` command exit/stdout/stderr contract. Source rewrite
  assertions should be added under the same unified case model as `fix` coverage
  expands.
- Build-owned contracts are diagnostics/reporting surfaces (text/json), artifact
  set expectations, and build-only gating behavior for non-runnable or
  pre-runtime failure paths.

If a case intentionally overlaps behavior owned elsewhere, state that ownership
in the case `README.md`.

## Run Selection Guidance

- For runnable success-path semantics, use `run` as the single default. Do not
  add a separate build-only success smoke case unless the case asserts a
  build-owned contract.
- Use `build` for non-runnable analysis/build scenarios.
- If a new `run` case subsumes an existing build-only success smoke case,
  replace the build case rather than keeping both.
- Include both `build` and `run` in one case only when the same scenario
  intentionally asserts both runtime behavior and a build-owned contract.
- Use strict/default mode differences only when mode behavior is the contract
  being tested.

## Update snapshots

```sh
UPDATE_SNAPSHOTS=1 bazel run //unified_tests:unified_tests_test
```
