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
7. Shared check orchestration and rendering (`compiler/check_pipeline`)

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
3. Orchestration consumes statuses explicitly; it does not infer gating from
   incidental behavior.

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

## Shared Non-Phase Packages

These packages are shared utilities/contracts and are not additional compiler
phases:

1. `compiler/fix_edits`:
   - source edit model + deterministic merge/apply behavior.
   - owns edit mechanics, not language-rule ownership.
2. `compiler/source_formatting`:
   - canonical source-formatting engine used by orchestration.
   - owns formatting implementation, not command policy.
3. `compiler/autofix_policy`:
   - shared strict/non-strict policy evaluation for pending safe autofixes.
   - owns policy decision logic, not rendering and not language-rule ownership.

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

1. `PACKAGE.copp` declaration constraints
2. `.bin.copp` `main` placement/signature constraints
3. role-specific visibility restrictions

Output: `PhaseOutput<()>`.

### `compiler/resolution`

Owns package-level resolution of symbols, exports, import visibility legality,
package cycles, and binding conflict checks.

Output: `FileScopedPhaseOutput<ResolutionArtifacts>`.

### `compiler/type_analysis`

Owns type, flow, and semantic usage checks (for example unused imports).

When introduced, this phase also owns:

1. function-type assignability and callability checks
2. explicit interface conformance and interface-based assignability checks

Output: `PhaseOutput<type_annotated_program::TypeAnnotatedFile>`.

### `compiler/check_pipeline`

Owns orchestration only:

1. phase ordering
2. workspace/package scoping
3. diagnostics aggregation/sorting/rendering
4. status-driven downstream gating
5. aggregation of phase-emitted safe-autofix artifacts

### `compiler/check_session`

Owns stateful interactive check orchestration for long-lived clients.

Responsibilities:

1. in-memory source overlay state
2. check invalidation/re-execution via `check_pipeline`
3. session lifecycle/state boundaries for tooling clients

### `compiler/lsp`

Owns LSP protocol transport/serving only.

Responsibilities:

1. stdio JSON-RPC framing and message handling
2. request/notification mapping to `check_session`
3. LSP-specific result publishing (for example diagnostics notifications)

### `compiler/driver`

Owns build/run orchestration only.

Responsibilities:

1. build/run command policy and target validation
2. consumption of analyzed check artifacts from `check_pipeline`
3. backend lowering/codegen execution flow
4. application of strict/non-strict autofix policy outcome for build/run

### `compiler/cli`

Owns CLI UX only.

Responsibilities:

1. command parsing/dispatch
2. rendering diagnostics/failures/policy messages
3. explicit source-write command behavior (`fix`)

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
6. phase crates may emit safe-autofix artifacts for their own diagnostics
7. orchestration layers (`check_pipeline`, `check_session`, `lsp`, `driver`,
   `cli`): consume/aggregate/policy-evaluate/render only

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
6. `type_analysis -> {semantic_program,semantic_types,type_annotated_program}`
7. `executable_lowering -> {type_annotated_program,executable_program}`
8. `cranelift_backend -> {executable_program,runtime_interface}`
9. `check_pipeline -> {parsing,syntax_rules,file_role_rules,resolution,semantic_lowering,type_analysis,source_formatting,fix_edits}`
10. `check_session -> check_pipeline`
11. `lsp -> check_session`
12. `driver -> {check_pipeline,executable_lowering,cranelift_backend,autofix_policy}`
13. `cli -> {check_pipeline,driver,lsp,autofix_policy}`

Key prohibitions:

1. `type_analysis` must not depend on `syntax`
2. `package_symbols` must not depend on `syntax`
3. semantic phase crates must not depend on orchestration crates
4. frontend phase crates must not depend on backend/runtime interface crates
5. shared non-phase packages (`fix_edits`, `source_formatting`,
   `autofix_policy`) must not own language rule evaluation

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
4. `check_pipeline` is the single shared check implementation for CLI and LSP
5. `check_session` encapsulates stateful overlays/incremental behavior for
   long-lived tooling clients

## Acceptance Criteria

1. Phase-owned diagnostics are deterministic and non-overlapping.
2. Downstream execution is status-driven, not heuristic-driven.
3. Recoverable parser failures still yield useful syntax outputs.
4. Hard failures stay outside phase diagnostics.

## Planned Extensions

These are intentional future extensions, not current architecture debt:

1. enrich `ParseError` metadata for stronger recovery/tooling decisions
2. expand syntax losslessness as tooling needs additional trivia fidelity
3. expand backend IR/runtime layering beyond the current minimal runnable slice
