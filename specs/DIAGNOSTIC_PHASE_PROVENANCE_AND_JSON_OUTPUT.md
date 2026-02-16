# Diagnostic Phase Provenance and JSON Output

Defines a structured diagnostics output mode with explicit phase provenance.

## Purpose

The compiler architecture depends on phase ownership boundaries. Current fixture
tests assert only rendered text output and exit status. That validates user
experience and suppression behavior, but it does not prove which phase emitted a
diagnostic.

This spec adds a machine-readable diagnostics output contract so tests can
assert diagnostic ownership directly.

## Problem Statement

Current text fixtures can pass even when ownership drifts across phases, if:

1. message text remains unchanged
2. span/path remain unchanged
3. output ordering remains unchanged

This creates a gap between architecture intent and test enforceability.

## Goals

1. Preserve existing human-readable CLI diagnostics behavior by default.
2. Add an explicit JSON mode for machine assertions.
3. Include diagnostic `phase` provenance in JSON output.
4. Enable robust ownership and pipeline tests without coupling tests to internal
   phase entrypoints.
5. Support future tooling/LSP integration via stable structured output.
6. Keep diagnostics fixture policy uniform across all cases.
7. Deliver phase-ownership assertions without requiring diagnostic-code
   infrastructure in this change.

## Non-Goals

1. Do not change default text rendering format.
2. Do not require direct phase-entrypoint tests for ownership assertions.
3. Do not expose internal implementation details beyond stable output schema.
4. Do not add explicit cross-version compatibility fields in JSON output at this
   stage.
5. Do not introduce per-case or per-area format-selection policy.
6. Do not introduce gradual fixture migration compatibility paths.
7. Do not introduce diagnostic code fields in this first structured-output
   rollout.

## Proposed CLI Contract

`check` adds an output format option:

1. `--format text` (default)
2. `--format json`

`text` mode remains current behavior.

`json` mode emits a single JSON document to stdout.

## JSON Output Schema

```json
{
    "ok": false,
    "diagnostics": [
        {
            "phase": "syntax_rules",
            "path": "lib.coppice",
            "message": "import declarations must appear before top-level declarations",
            "span": {
                "line": 5,
                "column": 1,
                "start": 42,
                "end": 79
            }
        }
    ]
}
```

Field semantics:

1. `ok`: true when no diagnostics are emitted.
2. `diagnostics`: sorted diagnostics list.
3. `phase`: owning phase that emitted the diagnostic.
4. `path`: rendered path used by CLI.
5. `message`: user-facing diagnostic text.
6. `span`: 1-based line/column plus byte offsets.

Schema versioning note:

1. The output intentionally omits a `version` field.
2. Cross-version compatibility is not a current requirement.
3. Compatibility needs can be introduced later if external consumers require
   explicit version negotiation.

Diagnostic-code note:

1. This rollout intentionally omits diagnostic `code`.
2. Phase ownership assertions are fully supported by `phase` + `path` + `span`
   - `message`.
3. Typed diagnostic codes are specified separately for future adoption.

## Phase Enum Values

`phase` must be one of:

1. `parsing`
2. `syntax_rules`
3. `file_role_rules`
4. `resolution`
5. `semantic_lowering`
6. `type_analysis`

`driver` does not own language-rule diagnostics and should not be used as
diagnostic phase provenance.

## Canonical Phase Enum

Introduce an explicit canonical enum for diagnostic provenance:

1. Type name: `DiagnosticPhase`
2. Home crate: `compiler/diagnostics`
3. Scope: shared identity vocabulary for diagnostics contracts (JSON output,
   tests, and any provenance-preserving rendering paths)

Required enum variants:

1. `Parsing`
2. `SyntaxRules`
3. `FileRoleRules`
4. `Resolution`
5. `SemanticLowering`
6. `TypeAnalysis`

JSON wire mapping:

1. `Parsing` -> `"parsing"`
2. `SyntaxRules` -> `"syntax_rules"`
3. `FileRoleRules` -> `"file_role_rules"`
4. `Resolution` -> `"resolution"`
5. `SemanticLowering` -> `"semantic_lowering"`
6. `TypeAnalysis` -> `"type_analysis"`

Provenance attachment rules:

1. Each phase contributes diagnostics tagged with its own `DiagnosticPhase`.
2. Driver aggregation preserves this phase tag without inference from message
   text.
3. Driver orchestration logic remains explicit and is not driven by iterating
   `DiagnosticPhase`.

Architectural note:

1. `DiagnosticPhase` is a provenance/contract type.
2. It is not a generic phase-runner abstraction.
3. Execution ordering and gating stay in explicit driver code.

## Serialization Dependencies

JSON output mode requires structured serialization support.

1. Use `serde` for derive/serialization traits on JSON output structs and
   serializable enums used by that output (including `DiagnosticPhase` if it is
   serialized directly).
2. Use `serde_json` in CLI JSON rendering/output code paths.
3. Text mode remains independent of JSON serialization concerns.

## Ownership and Provenance Rules

1. `phase` identifies the phase that produced the diagnostic.
2. Driver aggregation/rendering must preserve phase provenance.
3. Provenance must not be inferred from message text in tests.
4. Structured output is authoritative for ownership assertions.

## Testing Strategy

Every diagnostics fixture case asserts both output formats.

Fixture layout:

1. `input/`
2. `expect.text`
3. `expect.json`

Harness rules:

1. Every case must include both `expect.text` and `expect.json`.
2. The harness runs `check --format text` and `check --format json` for every
   case.
3. Missing either expected output file is a test failure.

Assertion scope:

1. `expect.text` validates user-facing rendering and caret formatting.
2. `expect.json` validates machine contract fields, including phase ownership.

Rationale:

1. A split policy (some text-only, some JSON-only) is ambiguous and hard to
   govern at scale.
2. A uniform dual-format rule removes case-classification overhead.
3. This keeps ownership and UX guarantees aligned for every diagnostics case.

## Why This Approach

Compared to direct phase-entrypoint tests:

1. Lower coupling to internal crate APIs.
2. Better end-to-end fidelity with actual CLI behavior.
3. Scales with diagnostics growth and refactors.

Compared to text-only fixtures:

1. Asserts ownership directly instead of indirectly.
2. Prevents silent architecture drift when text stays constant.

## Rollout Plan

1. Add `--format` option and JSON renderer.
2. Add phase provenance to rendered diagnostics.
3. Update diagnostics fixture harness to require dual-format expectations for
   every case.
4. Convert the fixture corpus to `expect.text` + `expect.json` in one change.
5. Keep the dual-format requirement mandatory for all future cases.

## Acceptance Criteria

1. `check` default text output remains unchanged.
2. `check --format json` outputs the documented JSON schema.
3. JSON diagnostics include stable `phase`.
4. Pipeline tests can fail on ownership drift even when text output is
   unchanged.
5. Structured output remains deterministic and sorted.
6. Every diagnostics fixture case asserts both text and JSON output.
