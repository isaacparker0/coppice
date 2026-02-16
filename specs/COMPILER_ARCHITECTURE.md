# Compiler Architecture

Authoritative architecture specification for the current compiler design.

## Purpose

This architecture supports Coppice language goals:

1. One constrained, deterministic way to implement language semantics.
2. Clear ownership boundaries by phase.
3. Build-time enforceable dependency direction.
4. A shared frontend foundation for CLI and tooling/LSP.

## Core Principles

1. Own each rule in the earliest phase that has enough information.
2. Separate source structure (`syntax`) from semantic meaning
   (`semantic_program`).
3. Keep orchestration in `driver`; keep language semantics in phase crates.
4. Use explicit phase outputs for diagnostics and downstream gating.
5. Prefer stable contracts over cross-crate implementation coupling.

## Canonical Pipeline

1. Parsing (`compiler/parsing`)
2. Syntax structural validity (`compiler/syntax_rules`)
3. File-role policy (`compiler/file_role_rules`)
4. Package/import/export/visibility resolution (`compiler/resolution`)
5. Semantic lowering (`compiler/semantic_lowering`)
6. Type analysis (`compiler/type_analysis`)
7. Driver orchestration and rendering (`compiler/driver`)

## Shared Reporting Contracts

`compiler/reports` owns shared frontend output/failure reporting contracts used
across command entrypoints (`check` today, extensible to future `build`/`run`):

1. `DiagnosticPhase`
2. `RenderedDiagnostic`
3. `CompilerFailure` (+ `CompilerFailureKind`)
4. `ReportFormat`

`compiler/driver` produces these contracts; CLI/tooling consumers render or
serialize them.

## Package Ownership

### `compiler/parsing`

Owns lexical and syntactic parsing.

Produces parser-facing syntax (`compiler/syntax::ParsedFile`) with parse
diagnostics and phase status via `PhaseOutput<ParsedFile>`.

### `compiler/syntax`

Owns source-structure representation.

Doc-comment source of truth:

- top-level docs are ordered `FileItem::DocComment` items
- struct-member docs are ordered `StructMemberItem::DocComment` items
- declaration nodes do not duplicate doc attachments

### `compiler/syntax_rules`

Owns parseable-but-invalid structural language rules.

Examples:

- imports must appear before other top-level declarations
- doc comments must document declarations

Returns `PhaseOutput<()>`.

### `compiler/file_role_rules`

Owns file-role-dependent policy rules.

Examples:

- `PACKAGE.coppice` declaration constraints
- `.bin.coppice` `main` placement/signature rules
- role-specific visibility restrictions

Returns `PhaseOutput<()>`.

### `compiler/resolution`

Owns package-level semantic resolution orchestration:

- symbols
- exports
- import visibility legality
- package cycle diagnostics
- binding conflict checks

Returns `FileScopedPhaseOutput<ResolutionArtifacts>` where artifacts include:

- resolved imports
- per-file resolution status
- path-aware resolution diagnostics

### `compiler/semantic_lowering`

Owns conversion from `syntax` to `semantic_program`.

- preserves spans for diagnostics
- attaches semantic docs from ordered syntax doc-comment items

### `compiler/semantic_program`

Owns semantic pass input model.

### `compiler/package_symbols` and `compiler/semantic_types`

Own typed cross-package symbol/type contracts.

### `compiler/type_analysis`

Owns type checking and related semantic usage checks (for example unused
imports).

Returns `PhaseOutput<()>`.

### `compiler/driver`

Owns orchestration only:

- phase ordering
- workspace/package scoping
- diagnostics aggregation/sorting/rendering
- downstream gating from explicit phase statuses

## Phase Contract

All phase boundaries use the shared envelope from `compiler/phase_results`:

```rust
pub struct PhaseOutput<T> {
    pub value: T,
    pub diagnostics: Vec<PhaseDiagnostic>,
    pub status: PhaseStatus,
}

pub enum PhaseStatus {
    Ok,
    PreventsDownstreamExecution,
}
```

Semantics:

1. `diagnostics` are owned by that phase.
2. `status` controls downstream execution eligibility.
3. Driver consumes these statuses explicitly; it does not infer gating from
   incidental behavior.

Boundary failure semantics:

1. `PhaseOutput<T>` is for program-facing diagnostics and phase gating.
2. Infrastructure/runtime/invariant failures should use explicit hard-failure
   channels at orchestration boundaries.
3. Infrastructure failures must not be emitted as synthetic language-rule
   diagnostics.

Current shared failure contract:

1. Hard failures are represented via `compiler/reports::CompilerFailure`.
2. This remains distinct from phase diagnostics and does not carry synthetic
   phase provenance.

## Current Gating Policy

Per file:

1. Parsing status gates syntax/file-role checks.
2. Syntax/file-role status gates resolution participation.
3. Resolution per-file status gates lowering/type-analysis.

This is linear orchestration with explicit per-file skipping; no non-linear
phase branching.

## Dependency Direction (Normative)

High-level direction:

1. `parsing -> syntax`
2. `syntax_rules -> syntax`
3. `file_role_rules -> syntax`
4. `resolution -> {symbols,exports,visibility,package_graph,binding}`
5. `semantic_lowering -> {syntax,semantic_program}`
6. `type_analysis -> {semantic_program,semantic_types}`
7. `driver` depends on phase crates for orchestration

Key prohibitions:

1. `type_analysis` must not depend on `syntax`.
2. `package_symbols` must not depend on `syntax`.
3. Semantic phase crates must not depend on `driver`.

These are enforced by Bazel dependency-enforcement tests.

## Diagnostic Ownership

Diagnostics are emitted by the phase that owns the violated rule:

1. parsing: lexical/syntactic failures
2. syntax_rules: structural source-shape failures
3. file_role_rules: file-role policy failures
4. resolution: package/import/export/visibility/binding/cycle failures
5. type_analysis: type and semantic usage failures
6. driver: rendering/sorting only

## Phase Placement Acceptance Criteria

Use these acceptance tests when deciding phase ownership:

1. Parser (`parsing`) owns a rule only if violating it prevents building
   reliable syntax structure from tokens.
2. Syntax rules (`syntax_rules`) own a rule when syntax structure is buildable
   but source structure/order is invalid by language definition.
3. File-role rules (`file_role_rules`) own a rule when validity depends on file
   role (`.bin.coppice`, `.test.coppice`, `PACKAGE.coppice`, library).
4. Resolution (`resolution`) owns a rule when package/import/export/visibility/
   dependency graph information is required.
5. Type analysis (`type_analysis`) owns a rule when type, flow, or usage
   information is required.

Negative constraints:

1. Parser must not enforce parseable structural policy rules (for example import
   ordering, doc-comment placement).
2. Syntax rules must not require package graph or type information.
3. Type analysis must not depend on parser/syntax internals.

Parser final-form constraints:

1. Parser leaf helpers should trend toward structured failures and boundary-site
   diagnostic ownership where context is clearer.
2. Recovery diagnostics should avoid duplicate emission and ownership drift.
3. Parser error metadata should continue becoming more machine-usable for
   tooling/recovery decisions.

## Tooling/LSP Alignment

This architecture is intended for a shared frontend:

1. parser remains resilient and returns syntax with diagnostics under errors
2. phase boundaries are explicit and machine-readable
3. gating semantics are deterministic and identical across CLI/tooling consumers

## Acceptance Criteria

1. Phase-owned diagnostics remain deterministic and non-overlapping by rule
   ownership.
2. Explicit phase statuses, not incidental heuristics, determine downstream
   execution.
3. Recoverable parse failures still produce useful syntax outputs for downstream
   tooling workflows.

## Future Goals

1. Continue enriching parser structured failure metadata (`ParseError`) for
   better recovery/tooling decisions.
2. Expand syntax losslessness incrementally as tooling features require
   additional source fidelity.
3. Introduce backend IR layers with distinct ownership when codegen paths are
   added.
