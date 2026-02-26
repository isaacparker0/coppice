# Test Fixture Strategy

Authoritative policy for organizing and naming fixture tests as coverage grows.

## Context

Current pain points:

1. Passing behavior is often packed into `minimal_valid`, which hides intent.
2. It is hard to tell why a line exists, what contract it protects, and whether
   coverage is redundant.
3. The old split (`tests/diagnostics`, `tests/executable_end_to_end`) mixes
   command contracts and causes ambiguity (for example `build`-only tests inside
   a runtime-oriented suite).

## What We Considered

1. Keep current style and continue expanding `minimal_valid`:
   - simple short term, poor long-term clarity.
2. Encode outcomes in case names (`*_valid`, `*_fails`, `*_blocks`):
   - readable in isolation, but couples names to compiler behavior instead of
     input scenario facts.
3. Keep old suite split and add special-case policy suites:
   - patches symptoms but keeps unclear suite ownership.
4. Re-split tests by command contract (`check`, `build`, `run`) with shared
   fixture infrastructure:
   - clear ownership and predictable coverage reasoning.

Selected direction: option 4.

## Conventions

### 1) Directory shape

All command suites use:

`tests/<suite>/<area>/<feature>/<case>/`

Planned command suites:

1. `tests/check/`
2. `tests/build/`
3. `tests/run/`

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

1. `check` suite owns diagnostics/reporting contracts.
2. `build` suite owns build-path contracts (target validation, dependency
   closure, artifacts, strict policy gating at build time).
3. `run` suite owns runtime/output/exit contracts plus run-path gating.

If a case intentionally overlaps behavior owned elsewhere, the case `README.md`
must explicitly reference the owning path.

## Shared Harness

Use one shared fixture harness library across all command suites.

Shared pieces:

1. case discovery
2. snapshot update/check mode
3. output normalization and placeholders
4. common assertion helpers

Each command suite runner should be thin and only define:

1. command invocation (`check`/`build`/`run`)
2. required fixture files for that suite
3. command-specific contract assertions

## Fixture Format Decision

Use file-based expectations with explicit run IDs.

Per case:

1. `input/`
2. `case.runs`
3. `expect.<run_id>.exit`
4. `expect.<run_id>.stdout`
5. `expect.<run_id>.stderr`
6. `expect.<run_id>.artifacts`

`case.runs` format:

1. One run per line
2. Explicit run ID in brackets, then args
3. Suite determines command (`check`/`build`/`run`)

Example:

```text
[text] --format text .
[json] --format json .
```

Rationale:

1. Explicit run IDs avoid implicit index coupling (`expect.1.*`) and improve
   readability/review.
2. File-per-expectation keeps large snapshots diff-friendly.
3. One format works for single-run and multi-run cases without command-specific
   special casing.

Why this is needed:

1. `check` cases generally require both text and json verification, so multi-run
   support is mandatory there.
2. `build`/`run` cases are usually single-run, but sharing the same run model
   avoids introducing a separate runner API just for `check`.
3. A single runner contract across `check`/`build`/`run` keeps maintenance and
   migration costs predictable as command surfaces evolve.

Tradeoffs accepted:

1. More files per case than single-manifest approaches.
2. Some expectation redundancy remains, mitigated by harness defaults for
   omitted files where allowed.

Alternatives considered:

1. Single nested manifest (all expected outputs inline):
   - fewer files, but worse large-output review ergonomics.
2. Indexed outputs (`expect.1.*`, `expect.2.*`):
   - simple but too implicit and harder to maintain.

## Migration Note

Case-by-case migration and rename/split recommendations are tracked in:

`specs/TEST_FIXTURE_STRATEGY_SEMANTIC_MAPPING.md`
