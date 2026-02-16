# Diagnostic Code Model (Future)

Defines the planned diagnostic-code architecture for a future rollout.

## Status

This spec is not part of the current JSON phase-provenance implementation.
Current structured diagnostics intentionally omit `code`.

## Purpose

Introduce stable machine-readable diagnostic identities with type-safe dynamic
message data.

## Problem

Relying only on user-facing message text as identity is fragile:

1. wording changes can break machine consumers and tests
2. dynamic message interpolation can lose stable identity
3. message-only contracts make integration/filtering less robust

## Goals

1. Provide stable diagnostic identities independent of text wording.
2. Keep dynamic diagnostic details type-safe.
3. Centralize message rendering logic by diagnostic identity.
4. Preserve phase ownership provenance from phase crates through driver output.

## Non-Goals

1. Do not implement this model in the current phase-provenance rollout.
2. Do not replace phase ownership as a primary contract dimension.

## Proposed Model

Use a typed enum for diagnostic identity plus payload:

1. Define `DiagnosticCode` as an enum with per-variant typed fields.
2. Each variant represents one stable diagnostic identity.
3. Variants with dynamic text carry required fields explicitly in payload.

Example shape (illustrative):

1. `DiagnosticCode::ImportOrderViolation`
2. `DiagnosticCode::ImportedSymbolNotFound { symbol: String, package: String }`

## Type Safety

Typed payloads ensure:

1. required dynamic fields are always supplied at compile time
2. no unknown/extra fields are passed accidentally
3. rendering logic cannot reference missing placeholders

This avoids untyped key-value metadata maps for dynamic diagnostics.

## Message Rendering

Message generation is driven by `DiagnosticCode`:

1. implement message rendering on the enum (or a closely coupled formatter)
2. keep user-facing message text centralized by code variant
3. allow wording evolution without changing code identity

## Ownership and Emission

1. Phase crates emit diagnostics with code + span (+ phase provenance).
2. Driver preserves provenance and code, and handles output formatting.
3. Driver does not infer code from message text.

## Structured Output Contract (Future Extension)

When adopted, JSON diagnostics add:

1. `code`: stable code identifier derived from `DiagnosticCode`

Optional future fields may include code-oriented metadata (for example category
or help references), but are out of scope for this design baseline.

## Rollout Strategy (Future)

1. Introduce `DiagnosticCode` type and rendering support.
2. Migrate diagnostics phase-by-phase.
3. Add/upgrade JSON fixture expectations for `code`.
4. Enforce new diagnostics to include code.

## Acceptance Criteria (Future)

1. Every emitted diagnostic has a stable code identity.
2. Dynamic diagnostics use typed payloads, not untyped field maps.
3. JSON output includes stable `code` alongside existing phase provenance.
4. Message wording can change without code identity churn.
