# Test Fixture Strategy

Authoritative policy for organizing and naming fixture tests as coverage grows.

## Context

Current pain points:

1. Passing behavior is often packed into `minimal_valid`, which hides intent.
2. It is hard to tell why a line exists, what contract it protects, and whether
   coverage is redundant.
3. Shared behavior across commands can create duplicated snapshots when command
   ownership is unclear.
4. We need one consistent runner API and fixture shape, not ad hoc command-
   specific formats.

## What We Considered

1. Keep current style and continue expanding `minimal_valid`:
   - simple short term, poor long-term clarity.
2. Encode outcomes in case names (`*_valid`, `*_fails`, `*_blocks`):
   - readable in isolation, but couples names to compiler behavior instead of
     input scenario facts.
3. Split everything by command suite:
   - cleaner than old split, but still duplicates shared scenario setup and
     encourages drift between command trees.
4. Run every command for every case:
   - over-constrained, noisy, and redundant for many invalid/pure-command cases.
5. Unified fixture pool + explicit per-case runs + command-scoped assertion
   policy:
   - single API, explicit command intent, minimal duplication.

Selected direction: option 5.

## Why Unified

The unified model provides three concrete benefits:

1. One fixture format and one harness API for all command contracts (`build`,
   `run`, `fix`), avoiding suite-specific drift.
2. One scenario can assert multiple command behaviors when that overlap is
   meaningful, avoiding duplicated fixture setup.
3. Coverage is organized by language/domain scenario (`<area>/<feature>/<case>`)
   rather than fragmented by command suites.

## Execution Plan

Use a parallel pilot root first, then migrate after validation.

1. Pilot root: `unified_tests/<area>/<feature>/<case>/`
2. Existing suites remain unchanged during pilot.
3. Evaluate fixture authoring/review ergonomics on real cases.
4. After format confirmation, migrate all cases and retire legacy suite split.

## Conventions

### 1) Directory shape

Unified fixture layout:

`<root>/<area>/<feature>/<case>/`

Keep the same shared hierarchy semantics:

1. `<area>` = subsystem boundary
2. `<feature>` = feature family
3. `<case>` = single scenario

### 2) Case naming

Case names describe input scenario facts, not compiler reactions.

1. Prefer `boolean_type_argument` over `boolean_type_argument_valid`.
2. Prefer `unattached_end_of_file` over vague names like `trailing`.
3. Do not encode outcomes in names (`valid`, `fails`, `blocks`, etc.).

### 3) Case intent documentation

Every case directory must include `README.md` with exactly one sentence stating
the contract the fixture is intended to lock.

Sentence quality rules:

1. Must add meaning beyond directory name.
2. Must describe what is being validated, not restate the case slug.
3. Must not claim behavior the fixture does not actually assert.

### 4) `minimal_valid` policy

`minimal_valid` is optional.

1. Use it only when a feature has a meaningful baseline smoke scenario.
2. Keep it intentionally small.
3. Do not accumulate unrelated behavior into it when targeted cases exist.

### 5) Ownership and overlap

Each behavior should have one primary ownership location.

Command ownership is by run type within the unified fixture pool:

1. `build` runs own diagnostics/reporting contract and build-path contracts
   (target validation, dependency closure, artifacts, strict gating outcome).
2. `run` runs own runtime/output/exit contracts plus run-path gating outcome.
3. `fix` runs own source-rewrite contract.

If a case intentionally overlaps behavior owned elsewhere, the case `README.md`
must explicitly reference the owning path.

### 6) Case shape defaults

Choose the smallest natural input shape for the scenario.

1. Default to library/package-shaped fixtures for declaration/type/package
   contracts that do not require executable behavior.
2. Use binary-entrypoint-shaped fixtures when runtime execution is part of the
   contract.
3. Do not add binary scaffolding to purely library/package scenarios only to
   force run coverage.

## Shared Harness

Use one shared fixture harness library across the unified fixture pool.

Shared pieces:

1. case discovery
2. snapshot update/check mode
3. output normalization and placeholders
4. common assertion helpers

One runner executes all cases and all declared runs.

## Fixture Format Decision

Use script-style case files with one literal CLI command per non-comment line.

Per case:

1. `input/`
2. `case.test`
3. `expect/` payload files discovered by naming convention

`case.test` format:

1. `<command args...>` for unlabeled runs.
2. `[label] <command args...>` for labeled runs.
3. Labels may use `[A-Za-z0-9_]`.
4. If a command appears once in a case, label is not allowed.
5. If a command appears multiple times in a case, each occurrence must have a
   unique label.
6. No synthetic run IDs and no numeric indexing.

Expectation stem policy:

1. Single occurrence of a command uses command name as stem (`build`, `run`,
   `fix`).
2. Repeated occurrences of the same command use the explicit label as stem.

Expected file policy:

1. Keep a fixed expectation shape per channel/field.
2. If expected output is empty, keep the expectation file and leave it empty.
3. Do not encode "no output" by omitting required expected files.

Rationale:

1. Literal command runs avoid redundant command encoding and keep fixture files
   directly aligned with actual CLI invocations.
2. Explicit labels for repeated commands keep intent obvious without brittle
   positional mapping.
3. Convention-based payload discovery removes boilerplate mapping while keeping
   snapshots diff-friendly.

Why this is needed:

1. `build` now owns diagnostics/reporting.
2. `run` and `fix` need different assertion shapes while keeping one case model.
3. A single runner API keeps command additions (for example `lsp`) from creating
   fragmented fixture systems.

## Assertion Policy by Run Command

1. `build` run:
   - runner executes both `--format text` and `--format json` for the same run.
   - verifies build-path contract (artifacts/failure summary) plus dual-format
     reporting output.
   - required files:
     - `expect/<stem>.text.stdout`
     - `expect/<stem>.json.stdout`
     - `expect/<stem>.stderr`
     - `expect/<stem>.artifacts`
     - `expect/<stem>.exit`
   - optional format-specific exit overrides:
     - `expect/<stem>.text.exit`
     - `expect/<stem>.json.exit`
   - if format-specific exits are absent, both formats use `expect/<stem>.exit`.
2. `run` run:
   - verifies runtime/output/exit contract.
   - for build-phase failure paths, verifies human-readable diagnostic output.
   - required files:
     - `expect/<stem>.stdout`
     - `expect/<stem>.stderr`
     - `expect/<stem>.artifacts`
     - `expect/<stem>.exit`
3. `fix` run:
   - verifies fix command exit/output contract.
   - verifies expected rewritten source tree contract (where asserted).
   - required files:
     - `expect/<stem>.stdout`
     - `expect/<stem>.stderr`
     - `expect/<stem>.exit`

Runner-owned execution details:

1. `build`/`run` output directory plumbing is runner-managed to keep case files
   clean and deterministic.
2. Cases should not pass per-run output-directory flags in `case.test`.
3. Strictness coverage is explicit only where mode differences are the behavior
   under test.

Run selection policy:

1. Runnable happy-path scenarios generally use `run` only.
2. Include both `build` and `run` in one case only when a build-specific
   contract is also being asserted (for example artifacts, format/report
   surfaces, strict/default gating).
3. Use build-only for non-runnable scenarios.
4. Use strict mode by default for most cases; add default/non-strict mode only
   for scenarios where mode differences are the contract under test.

Tradeoffs accepted:

1. More expectation payload files than inline-manifest approaches.
2. Build coverage includes explicit dual-format assertions by design.
3. Parser complexity is still non-trivial due to label/stem and command-count
   rules.

Alternatives considered:

1. Single nested manifest with inline output payloads:
   - fewer files, but worse large-output review ergonomics.
2. Indexed output trees (`expect.1.*`, `expect.2.*`) or synthetic run IDs:
   - introduces mapping indirection and weak readability.
3. Force all commands/modes on every case:
   - strict but noisy/redundant and poor signal for command-specific scenarios.
4. Command-specific fixture trees:
   - clearer than old split, but higher duplication and drift risk.

## Known Limitations

1. This model depends on disciplined case intent docs and ownership notes; the
   runner alone cannot prevent semantic overlap.
2. If command/report surfaces change significantly, assertion profiles should be
   revised before broad migration.

## Migration Note

Legacy case-by-case migration and rename/split recommendations are tracked in:

`specs/TEST_FIXTURE_STRATEGY_SEMANTIC_MAPPING.md`
