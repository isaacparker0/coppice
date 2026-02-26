# Test Fixture Strategy

Authoritative policy for organizing and naming fixture tests as coverage grows.

## Context

Current pain points:

1. Passing behavior is often packed into `minimal_valid`, which hides intent.
2. It is hard to tell why a line exists, what contract it protects, and whether
   coverage is redundant.
3. Frontend diagnostics are shared across `check`/`build`/`run`, which makes
   ownership unclear and causes repeated snapshots.
4. We need one consistent runner API and fixture shape, not ad hoc command-
   specific formats.

## What We Considered

1. Keep current style and continue expanding `minimal_valid`:
   - simple short term, poor long-term clarity.
2. Encode outcomes in case names (`*_valid`, `*_fails`, `*_blocks`):
   - readable in isolation, but couples names to compiler behavior instead of
     input scenario facts.
3. Split everything by command suite (`tests/check`, `tests/build`,
   `tests/run`):
   - cleaner than old split, but still duplicates shared scenario setup and
     encourages drift between command trees.
4. Run `check`/`build`/`run` for every case:
   - over-constrained, noisy, and redundant for many invalid/pure-command cases.
5. Unified fixture pool + explicit per-case run entries + command-scoped
   assertion policy:
   - single API, explicit command intent, minimal duplication.

Selected direction: option 5.

## Conventions

### 1) Directory shape

Unified fixture layout:

`tests/<area>/<feature>/<case>/`

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

1. `check` runs own full diagnostics/reporting contract.
2. `build` runs own build-path contracts (target validation, dependency closure,
   artifacts, strict gating outcome).
3. `run` runs own runtime/output/exit contracts plus run-path gating outcome.

If a case intentionally overlaps behavior owned elsewhere, the case `README.md`
must explicitly reference the owning path.

## Shared Harness

Use one shared fixture harness library across the unified fixture pool.

Shared pieces:

1. case discovery
2. snapshot update/check mode
3. output normalization and placeholders
4. common assertion helpers

One runner executes all cases and all declared runs.

## Fixture Format Decision

Use file-based expectations with explicit command runs.

Per case:

1. `input/`
2. `case.runs`
3. `expect.build.stdout`
4. `expect.build.stderr`
5. `expect.build.exit`
6. `expect.build.artifacts`
7. `expect.run.stdout`
8. `expect.run.stderr`
9. `expect.run.exit`
10. `expect.run.artifacts`
11. `expect.check.text.stdout`
12. `expect.check.text.stderr`
13. `expect.check.text.exit`
14. `expect.check.json.stdout`
15. `expect.check.json.stderr`
16. `expect.check.json.exit`

`case.runs` format:

1. One run per line
2. Literal command token (`check`/`build`/`run`) followed by args
3. No synthetic run IDs

Example:

```text
check
build main.bin.copp
run main.bin.copp
```

Rationale:

1. Literal command runs avoid redundant command encoding and keep fixture files
   directly aligned with actual CLI invocations.
2. File-per-expectation keeps large snapshots diff-friendly.
3. One format works for single-run and multi-run cases without ad hoc per-
   command fixture schemas.

Why this is needed:

1. `check` diagnostics are shared frontend behavior and should be snapshotted
   once in their owning run type, not duplicated under `build`/`run`.
2. `build`/`run` still need command-level coverage, but usually not full
   diagnostic snapshot duplication.
3. A single runner API keeps command additions (for example `fix`, `lsp`) from
   creating fragmented fixture systems.

## Assertion Policy by Run Command

1. `check` run:
   - must verify both text and json diagnostic outputs.
   - each mode must assert actual command output channels (`stdout`/`stderr`)
     and exit code.
   - owns full diagnostics snapshot contract.
2. `build` run:
   - verifies build-path contract (exit/output/artifacts/failure summary).
   - does not re-own full diagnostics snapshot contract.
3. `run` run:
   - verifies runtime/output/exit contract (and run-path failure summary).
   - does not re-own full diagnostics snapshot contract.

Tradeoffs accepted:

1. More files per case than single-manifest approaches.
2. Some command overlap remains by design, but ownership is explicit by run
   type.
3. Build/run diagnostic rendering regressions rely primarily on `check`
   coverage.
4. `check` mode-specific expectation files add surface area, but keep channel
   semantics explicit and faithful to actual command output.

Alternatives considered:

1. Single nested manifest (all expected outputs inline):
   - fewer files, but worse large-output review ergonomics.
2. Indexed outputs (`expect.1.*`, `expect.2.*`) or synthetic run IDs:
   - too implicit and redundant with command tokens.
3. Force all commands on every case:
   - strict but noisy/redundant and poor signal for command-specific scenarios.
4. Command-specific fixture trees:
   - clearer than old split, but higher duplication and drift risk.

## Known Limitations

1. This model depends on disciplined case intent docs and ownership notes; the
   runner alone cannot prevent semantic overlap.
2. If non-`check` commands later expose stable structured report modes, this
   policy should be revisited so those contracts are owned explicitly.

## Migration Note

Case-by-case migration and rename/split recommendations are tracked in:

`specs/TEST_FIXTURE_STRATEGY_SEMANTIC_MAPPING.md`
