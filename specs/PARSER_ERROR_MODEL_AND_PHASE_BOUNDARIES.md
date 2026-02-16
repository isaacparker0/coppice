# Parser Error Model and Phase Boundaries

Defines the parser error model and the phase-boundary contract used by the
compiler pipeline.

## Scope

This document is normative for:

1. parser control-flow error handling (`ParseResult` / `ParseError`)
2. parse diagnostics ownership boundaries
3. shared phase output/gating contract (`PhaseOutput<T>`, `PhaseStatus`)
4. placement of syntax-structural vs semantic rules

## Parser Error Model

Parser internals use structured control-flow failures:

```rust
type ParseResult<T> = Result<T, ParseError>;
```

Rules:

1. Use `Err(ParseError)` for parse failure.
2. Use `Option<T>` only for optional grammar inside successful parse paths.
3. Recovery happens at explicit boundary/caller synchronization points.
4. User-facing parse diagnostics are rendered via parser boundary reporting
   (`report_parse_error`), not ad-hoc control-flow signaling through `Option`.

## Parse Ownership

1. Lexer owns lexical failures as structured lex errors.
2. Parser owns syntactic failures as structured parse errors.
3. Parse boundary aggregates lex+parse diagnostics deterministically.

## Parser Final-Form Constraints

1. Parser control flow remains `ParseResult<T>`-based and recovery-point driven.
2. User-facing diagnostics should be emitted at boundary/recovery sites when
   those sites have better context than leaf helpers.
3. Parser leaf helpers should avoid becoming long-term owners of policy
   diagnostics when equivalent boundary emission is clearer and non-duplicative.
4. `ParseError` should continue evolving toward richer machine-usable metadata
   for recovery/tooling (for example expected-vs-found class and recovery
   hints).

## Shared Phase Contract

All phase entrypoints use:

```rust
pub struct PhaseOutput<T> {
    pub value: T,
    pub diagnostics: Vec<Diagnostic>,
    pub status: PhaseStatus,
}

pub enum PhaseStatus {
    Ok,
    PreventsDownstreamExecution,
}
```

Semantics:

1. `diagnostics` are user-program violations emitted by that phase.
2. `status` controls downstream execution eligibility.
3. Driver gating uses explicit `status`, not inferred heuristics.

## Boundary Failure Semantics

1. `PhaseOutput<T>` models program-facing diagnostics plus downstream gating.
2. Infrastructure/runtime/invariant failures are not represented as normal phase
   diagnostics and should use explicit hard-failure channels at orchestration
   boundaries.
3. Do not encode infrastructure failures as synthetic language diagnostics.

## Current Boundary Shape by Phase

1. Parsing: `PhaseOutput<ParsedFile>`
2. Syntax rules: `PhaseOutput<()>`
3. File-role rules: `PhaseOutput<()>`
4. Resolution: `PhaseOutput<ResolutionArtifacts>`
5. Type analysis: `PhaseOutput<()>`

## Structural vs Semantic Rule Placement

Use this rubric:

1. Parsing phase:
   - cannot build reliable syntax structure from tokens.
2. Structural validity phase (`syntax_rules`):
   - syntax is parseable but violates declared structural/order constraints.
3. Semantic phases (`resolution`, `type_analysis`):
   - rule requires package/name/visibility/type/usage information.

Acceptance criteria for parser-vs-`syntax_rules` boundary:

1. If the parser can still produce a reliable syntax tree, the rule does not
   belong in parser.
2. If enforcement depends only on ordered syntax items/spans and not on package
   or type data, the rule belongs in `syntax_rules`.
3. Parser should report failures in token/grammar construction; parser should
   not be the long-term owner of parseable structural policy diagnostics.

Non-acceptance examples for parser ownership:

1. import ordering
2. doc-comment attachment/placement
3. other declaration-order policy rules

Examples:

1. imports-must-be-top-of-file -> `syntax_rules`
2. doc-comment placement -> `syntax_rules`
3. unknown package/import legality/export visibility -> `resolution`
4. unused imports/type mismatch/name usage/type rules -> `type_analysis`

## Syntax Representation Decision

For compiler + tooling/LSP alignment:

1. `syntax` owns source-structure fidelity (ordered doc comments, spans, etc.).
2. `semantic_program` owns semantic meaning.
3. Semantic doc attachment is derived in lowering from ordered syntax items.

## Driver Gating Policy

Per-file linear gating:

1. parse `status` gates syntax/file-role checks
2. syntax/file-role statuses gate resolution
3. resolution per-file status gates lowering/type-analysis

This is explicit per-file skipping within a linear pipeline, not non-linear
phase branching.

## LSP/Tooling Requirements

This model is required for unified CLI/tooling behavior:

1. parser returns usable syntax + diagnostics under error conditions where
   possible
2. every phase exposes machine-readable diagnostics + status
3. gating behavior is deterministic and shared across consumers

## Acceptance Criteria

1. Parser continues returning useful syntax output under recoverable syntax
   errors, with diagnostics.
2. Driver gating is explicitly status-driven per phase/per file.
3. Diagnostics remain phase-owned and deterministic (no ownership drift).
4. Cascades caused by running downstream phases on known-invalid prerequisites
   are prevented by explicit phase status, not ad-hoc heuristics.

## Remaining Work (Active)

1. Enrich `ParseError` metadata for stronger recovery decisions and tooling
   context.
2. Continue reducing direct leaf diagnostic side effects where boundary
   diagnostics are clearer.
3. Expand syntax losslessness incrementally as tooling use-cases demand
   additional trivia fidelity.
