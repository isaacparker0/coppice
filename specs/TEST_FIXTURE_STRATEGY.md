# Test Fixture Strategy

Authoritative policy for organizing and naming fixture tests as coverage grows.

## Context

Current pain points:

1. Passing behavior is often packed into `minimal_valid`, which hides intent.
2. It is hard to tell why a line exists, what contract it protects, and whether
   coverage is redundant.
3. Cross-cutting behavior (for example autofix strict/non-strict policy) spans
   multiple commands and does not map cleanly to one existing suite.

## What We Considered

1. Keep current style and continue expanding `minimal_valid`:
   - simple short term, poor long-term clarity.
2. Encode outcomes in case names (`*_valid`, `*_fails`, `*_blocks`):
   - readable in isolation, but couples names to compiler behavior instead of
     input scenario facts.
3. Merge suites:
   - weakens clean contract boundaries (`check` diagnostics vs build/run e2e).
4. Keep suite boundaries and move to scenario-first, intent-explicit cases:
   - best balance of clarity, scalability, and low churn.

Selected direction: option 4.

## Conventions

### 1) Directory shape

Keep existing shape from `tests/README.md`:

`tests/<suite>/<area>/<feature>/<case>/`

### 2) Case naming

Case names describe input scenario facts, not compiler reactions.

1. Prefer `boolean_type_argument` over `boolean_type_argument_valid`.
2. Prefer `strict_mode_pending_safe_autofix` over
   `strict_blocks_pending_safe_autofix`.

### 3) Case intent documentation

Every case directory must include `README.md` with exactly one sentence stating
what scenario contract the case locks.

### 4) `minimal_valid` policy

`minimal_valid` is optional.

1. Use it only when a feature has a meaningful baseline smoke scenario.
2. Keep it intentionally small.
3. Do not accumulate unrelated behavior into it when targeted cases exist.

### 5) Ownership and overlap

Each behavior should have one primary ownership area.

1. Local semantics belong in local feature areas.
2. Cross-package behavior belongs in package/import areas.
3. Build/run/runtime behavior belongs in executable end-to-end areas.

If a case intentionally overlaps behavior owned elsewhere, the case `README.md`
must explicitly reference the owning path.

## Cross-Cutting Policy Coverage

For behavior that spans commands (for example autofix policy across
`check/build/run`), keep current suite boundaries and add a dedicated
cross-command policy suite rather than duplicating deep assertions across
multiple existing suites.
