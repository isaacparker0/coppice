# Compiler Architecture

Authoritative architecture specification for the Coppice compiler frontend.

Related naming reference:

- `specs/COMPILER_NAMING.md`

## Purpose

This architecture exists to enforce deterministic semantics and maintainable
compiler evolution while keeping CLI and tooling/LSP behavior aligned.

Design goals:

1. One constrained, predictable implementation path for language rules.
2. Clear rule ownership by phase.
3. Build-time enforceable dependency direction.
4. Shared frontend behavior across command entrypoints and tooling.

## Design Rationale

1. Determinism over convenience: each rule has one owner phase.
2. Earliest-sufficient ownership: a rule belongs to the earliest phase with the
   information required to evaluate it correctly.
3. Explicit boundaries over implicit behavior: phase outputs and statuses are
   machine-readable contracts used by orchestration.
4. Representation separation: source-structure fidelity is distinct from
   semantic meaning.
5. Hard-failure separation: infrastructure/runtime failures are not modeled as
   language diagnostics.

## Canonical Pipeline

1. Parsing (`compiler/parsing`)
2. Syntax structural validity (`compiler/syntax_rules`)
3. File-role policy (`compiler/file_role_rules`)
4. Package/import/export/visibility resolution (`compiler/resolution`)
5. Semantic lowering (`compiler/semantic_lowering`)
6. Type analysis (`compiler/type_analysis`)
7. Driver orchestration and rendering (`compiler/driver`)

The pipeline is linear. Per-file downstream skipping is controlled by explicit
phase status, not by ad-hoc heuristics.

## Phase Contracts

All non-resolution phase boundaries use
`compiler/phase_results::PhaseOutput<T>`:

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

Resolution uses file-scoped output:

```rust
pub struct FileScopedPhaseOutput<T> {
    pub value: T,
    pub diagnostics: Vec<FileScopedDiagnostic>,
    pub status_by_file: BTreeMap<PathBuf, PhaseStatus>,
}
```

Contract semantics:

1. `diagnostics` are emitted by the owning phase.
2. `status` (or `status_by_file`) is the only downstream gating signal.
3. Driver consumes statuses explicitly; it does not infer gating from incidental
   behavior.

## Parser Error Model

Parser control flow uses structured parse failures:

```rust
type ParseResult<T> = Result<T, ParseError>;
```

Rules:

1. Use `Err(ParseError)` for parse failure.
2. Use `Option<T>` only for optional grammar on successful parse paths.
3. Recovery occurs at explicit parser boundary/caller synchronization points.
4. User-facing parse diagnostics are rendered through parser boundary reporting
   from structured parse errors.

Ownership and behavior:

1. Lexer owns lexical failures.
2. Parser owns syntactic failures.
3. Parse boundary aggregates lex+parse diagnostics deterministically.
4. Parser remains resilient and returns usable syntax under recoverable parse
   errors.

## Representation Boundaries

1. `compiler/syntax` owns source structure fidelity (ordered items, spans,
   doc-comment items, parse-facing shape).
2. `compiler/semantic_program` owns semantic pass input representation.
3. `compiler/semantic_lowering` maps `syntax` to `semantic_program` while
   preserving diagnostic spans and deriving semantic doc attachments from
   ordered syntax doc-comment items.

## Phase Ownership

### `compiler/parsing`

Owns lexical and syntactic parsing.

Output: `PhaseOutput<syntax::ParsedFile>`.

### `compiler/syntax_rules`

Owns parseable-but-invalid structural source-shape rules.

Examples:

1. import declarations must precede non-import top-level declarations
2. doc comments must document declarations

Output: `PhaseOutput<()>`.

### `compiler/file_role_rules`

Owns role-dependent policy rules.

Examples:

1. `PACKAGE.coppice` declaration constraints
2. `.bin.coppice` `main` placement/signature constraints
3. role-specific visibility restrictions

Output: `PhaseOutput<()>`.

### `compiler/resolution`

Owns package-level resolution of symbols, exports, import visibility legality,
package cycles, and binding conflict checks.

Output: `FileScopedPhaseOutput<ResolutionArtifacts>`.

### `compiler/type_analysis`

Owns type, flow, and semantic usage checks (for example unused imports).

Output: `PhaseOutput<()>`.

### `compiler/driver`

Owns orchestration only:

1. phase ordering
2. workspace/package scoping
3. diagnostics aggregation/sorting/rendering
4. status-driven downstream gating

## Gating Policy

Per file:

1. parse status gates syntax/file-role checks
2. syntax/file-role status gates resolution participation
3. resolution per-file status gates semantic lowering and type analysis

This is explicit per-file skipping inside a linear pipeline.

## Diagnostic and Failure Ownership

Phase-owned language diagnostics:

1. parsing: lexical/syntactic diagnostics
2. syntax_rules: structural source-shape diagnostics
3. file_role_rules: file-role policy diagnostics
4. resolution: package/import/export/visibility/binding/cycle diagnostics
5. type_analysis: type/flow/usage diagnostics
6. driver: rendering/sorting only

Hard failures:

1. infrastructure/runtime/invariant failures are represented via
   `compiler/reports::CompilerFailure`
2. these remain distinct from phase diagnostics
3. infrastructure failures must not be encoded as synthetic language diagnostics

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

1. `type_analysis` must not depend on `syntax`
2. `package_symbols` must not depend on `syntax`
3. semantic phase crates must not depend on `driver`

These are enforced by Bazel dependency-enforcement tests.

## Rule Placement Rubric

Choose ownership using these checks:

1. Parsing owns a rule only when violating it prevents reliable syntax structure
   construction from tokens.
2. Syntax rules own parseable structural/order policy constraints.
3. File-role rules own constraints requiring file-role knowledge.
4. Resolution owns constraints requiring package/import/export/visibility/graph
   information.
5. Type analysis owns constraints requiring type/flow/usage information.

Non-acceptance examples for parser ownership:

1. import ordering
2. doc-comment placement/attachment
3. declaration-order policy rules that do not block syntax construction

## Tooling/LSP Alignment

This architecture is required for shared CLI/tooling behavior:

1. parser returns syntax + diagnostics under recoverable parse errors
2. phase outputs are machine-readable and stable
3. gating behavior is deterministic and shared across consumers

## Acceptance Criteria

1. Phase-owned diagnostics are deterministic and non-overlapping.
2. Downstream execution is status-driven, not heuristic-driven.
3. Recoverable parser failures still yield useful syntax outputs.
4. Hard failures stay outside phase diagnostics.

## Planned Extensions

These are intentional future extensions, not current architecture debt:

1. enrich `ParseError` metadata for stronger recovery/tooling decisions
2. expand syntax losslessness as tooling needs additional trivia fidelity
3. introduce backend IR ownership layers when build/run/codegen are added
