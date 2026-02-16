# Parser Error Model and Phase Boundaries

## Purpose

Define the parser error-model direction (`Option` to `Result`) and the intended
ownership boundaries between parsing, structural validity checks, and later
semantic/type phases.

This document now also evaluates these choices against future LSP/tooling needs,
where partial results, resilient recovery, and explicit failure semantics matter
as much as command-line batch compilation.

## Context

The parser historically used `Option<T>` in many functions:

- `Some(T)` means parse succeeded.
- `None` means parse failed (often after emitting a diagnostic and/or attempting
  recovery).

This worked for early syntax, but generics and constraints increase grammar
depth and failure modes. `Option` does not encode _why_ parsing failed, and
nested optionality (`Option<Option<T>>`) can appear when modeling optional
grammar elements.

## Current State

### Historical baseline (why migration started)

- Parse functions frequently returned `Option<T>`.
- Diagnostics are emitted as side effects (`self.error(...)`).
- Recovery is done by synchronization helpers (`synchronize_*`) in callers.

### Current implementation snapshot

- `ParseResult<T>` is now used across major parser modules.
- Centralized boundary reporting (`report_parse_error(...)`) is in place for
  many recovery catch points.
- Some transitional direct diagnostic paths remain (documented in Implementation
  Status below).

### Compiler pipeline behavior

- Phase contracts are mixed:
  - parsing externally returns `Result<ParsedFile, Vec<Diagnostic>>`
  - file-role/resolution/type-analysis phases mostly use `&mut Vec<Diagnostic>`
  - driver aggregates diagnostics and gates later phases
- This is workable, but phase boundary semantics are inconsistent.

### Limitations

- Failure semantics are ambiguous (`None` = absent node? invalid syntax? sync
  bailout?).
- Readability degrades in complex branches.
- Harder to audit whether failure was expected/handled.
- Easy to accidentally encode awkward types for optional grammar shapes.
- Cross-phase boundary contracts are not yet a single obvious pattern.

## Proposed State

### A. Parser internals use `Result`

Use `Result` for parser mechanics:

```rust
type ParseResult<T> = Result<T, ParseError>;
```

with:

- `Ok(T)` for success
- `Err(ParseError)` for parse failure

Keep diagnostics emission in parser context during initial migration
(side-effect diagnostics remain), while `ParseError` drives control flow and
recovery.

Target final form:

- Leaf parser helpers (for example `expect_*`) return structured parse failures
  and are not primary user-diagnostic emitters.
- Recovery/boundary points own diagnostic emission because they have sufficient
  syntactic context to emit clearer, non-duplicative diagnostics.

Why this is valuable:

- Makes parser intent explicit: parse failure vs optional grammar.
- Scales better with deep generic syntax and constraints.
- Improves local readability and reviewability of parser control flow.
- Creates a better foundation for editor tooling that needs resilient parsing.

### B. Standardize phase boundaries with a single compiler-phase envelope

Adopt a uniform phase boundary contract:

```rust
type CompilerPhaseResult<T> = Result<CompilerPhaseOutput<T>, FatalError>;

struct CompilerPhaseOutput<T> {
    value: T,
    diagnostics: Vec<Diagnostic>,
}
```

Semantics:

- `diagnostics`: user program violations owned by that phase.
- `FatalError`: infrastructure/invariant failure that prevents meaningful
  continuation.

This standardizes inter-phase APIs while still allowing internal implementation
styles to differ.

Why this may be valuable:

- Gives tooling and orchestration a predictable shape for results.
- Separates user diagnostics from infrastructure/runtime failures.
- Can reduce ad-hoc phase composition logic in driver/tooling entrypoints.

Why this is not automatically a strict improvement:

- If applied mechanically, it can add boilerplate wrappers without improving
  semantics.
- Some in-process pass compositions are already simple and clear with
  `&mut Vec<Diagnostic>`.
- Value comes from stronger contracts (fatal vs diagnostic policy, partial
  outputs), not from wrapper types alone.

### C. Explicit structural validity phase between parsing and semantic analysis

Adopt a dedicated phase for syntax-adjacent language rules once a file is
parseable:

- parser owns syntax construction and syntax-error recovery
- structural pass owns parseable-but-invalid declaration/file structure rules
- semantic/type phases own rules requiring name/type/use information

Representative ownership:

- imports-must-be-top-of-file: structural validity phase
- doc-comment placement ("doc comment must document a declaration"): structural
  validity phase (from syntax-owned ordered items)
- unused imports: semantic/type analysis phase

### D. Explicit phase-status contract for downstream gating

Moving structural rules out of parser can change whether later phases run for
invalid files. That can introduce diagnostic cascades (for example one
structural error plus unrelated unknown-package follow-ons).

To keep ownership deterministic and avoid noise:

1. Structural validity phase should publish explicit per-file gating status.
2. Driver should gate downstream semantic phases based on that status.
3. This should be an explicit contract, not ad-hoc inference from incidental
   behavior.

## ParseError Shape (initial)

Minimal and control-flow oriented:

```rust
enum ParseError {
    UnexpectedToken,
    MissingToken,
    InvalidConstruct,
    Recovered,
}
```

This can evolve later to include richer metadata if needed.

Recommended near-term refinement (especially for LSP):

- Include whether recovery already happened or caller must synchronize.
- Include token/span context (at minimum failing span).
- Include expected-vs-found token class where practical.

Rationale:

- Better recovery decisions at call sites.
- Better debuggability for parser maintenance.
- Better tooling behavior when presenting inline syntax issues.

Target final-form guidance:

- `ParseError` should carry machine-usable metadata:
  - failing span/token position
  - expected vs found token class where available
  - recovery guidance (for example whether caller should synchronize)
  - consumption/recovery state where needed to prevent error cascades
- `ParseError` should not require finalized user-facing message strings at leaf
  sites.
- User-facing diagnostic text should be rendered at recovery/boundary points.

## Design Rules

1. Parser internals use `ParseResult<T>` for syntax control flow.
2. Optional grammar is represented as `Option<T>` inside success values, not in
   outer return types.
3. Recovery points are explicit: callers catch `Err`, synchronize, and continue
   where allowed.
4. At phase boundaries, phases return `CompilerPhaseResult<T>` (value +
   diagnostics or fatal failure).
5. Diagnostics ownership remains phase-local per `COMPILER_ARCHITECTURE.md`.

6. Parser entrypoint should continue to produce a parsed representation even
   when diagnostics exist where feasible; this is important for tooling
   workflows.
7. Boundary unification should only be adopted where it improves semantic
   clarity, composability, or tooling outputs (not as a blanket mechanical
   rule).
8. Parser internals should separate:
   - control-flow failure (`ParseError`)
   - user diagnostic rendering (recovery/boundary ownership)
9. Parseable-but-invalid structural language rules should not accumulate in
   parser internals; they should live in a dedicated structural validity phase.
10. Downstream phase gating should be driven by explicit phase status (per
    file), not by implicit heuristics such as "diagnostics vector is empty".
11. Structural checks should be implemented in `syntax_rules` when syntax output
    contains the required ordered source structure.

## Syntax Representation Decision (Compiler + Tooling)

Decision:

1. Long-term, prefer a lossless syntax representation over permanent ad-hoc side
   channels for comments/trivia.
2. Keep semantic meaning in `semantic_program`; keep source-fidelity concerns in
   `syntax`.

Why:

1. Clean ownership: parser builds structure, syntax rules validate structure,
   semantic passes operate on meaning.
2. Tooling/LSP needs stable source-fidelity data (comments/trivia/spans) for
   documentation, refactors, and resilient editor workflows.
3. Avoids long-term drift from scattered "side map" contracts that become
   difficult to reason about and maintain.

Alternatives considered:

1. Permanent parser-local policy checks:
   - lower implementation cost now
   - worse ownership clarity and weaker tooling scalability
2. Permanent AST + external side channel maps:
   - workable short term
   - acceptable only as a transitional step toward lossless syntax, not a target
     end state
3. Lossless syntax layer with derived AST/semantic views:
   - higher upfront modeling work
   - best long-term architecture for unified compiler + tooling behavior

## Ownership Rubric

Use this rubric for rule placement:

1. Parser: cannot build reliable syntax structure from tokens.
2. Structural validity pass: structure is parseable, but violates
   language-declared structural/order constraints.
3. Semantic/type analysis: requires name/type/use information.

## Example

Before:

```rust
fn parse_type_parameter_constraint(&mut self) -> Option<Option<TypeName>>;
```

After:

```rust
fn parse_type_parameter_constraint(&mut self) -> ParseResult<TypeName>;
```

And optionality is represented at call site:

```rust
let constraint = if self.peek_is_symbol(Symbol::Colon) {
    self.advance();
    Some(self.parse_type_parameter_constraint()?)
} else {
    None
};
```

## Migration Plan

### Phase 1: parser-internal cleanup

1. Introduce shared `ParseError` + `ParseResult<T>` aliases in parser module.
2. Convert leaf functions first:
   - `expect_*`
   - atom parsers (identifiers, type atoms, literals)
3. Convert list/sequence parsers:
   - type arguments
   - type parameters
   - parameter lists
4. Convert declaration parsers and top-level file parsing internals.
5. Remove legacy `Option` parse return signatures.
6. Keep diagnostic snapshot behavior stable throughout migration.

Why this sequence is valuable:

- Leaf-first conversion limits blast radius and keeps failure semantics local.
- Sequence/list conversion next removes the highest ambiguity (`Option` chains).
- Top-level conversion last preserves recovery behavior while internals settle.
- Snapshot stability ensures regressions are intentional and reviewable.
- Temporary mixed diagnostic ownership can exist during migration, but final
  ownership should converge to recovery/boundary sites.

### Phase 2: boundary unification

1. Introduce shared `CompilerPhaseOutput<T>`, `CompilerPhaseResult<T>`, and
   `FatalError` in a boundary crate.
2. Migrate phase entrypoints (parsing, file-role, resolution, lowering,
   type-analysis) to return `PhaseResult`.
3. Keep existing diagnostic text and ordering behavior unchanged.
4. Update driver orchestration to compose `PhaseResult` uniformly and preserve
   existing gating policy.

Refinement:

- Treat this as an architecture/tooling improvement phase, not a mandatory
  prerequisite for parser cleanup.
- Apply to boundaries where partial outputs and explicit fatal handling are
  beneficial (not necessarily every internal pass API immediately).

### Phase 3: parser final-form cleanup

1. Move remaining leaf-site `self.error(...)` emission to explicit
   recovery/boundary points where practical.
2. Enrich `ParseError` metadata to support robust recovery and tooling.
3. Define and enforce deduplication policy for error cascades.
4. Preserve parser resilience so errorful files still yield useful syntax output
   plus diagnostics for tooling.

### Phase 4: structural-validity extraction and stabilization

1. Introduce or expand a dedicated structural validity phase crate.
2. Move syntax-adjacent policy checks (for example declaration-order rules) out
   of parser internals into that phase.
3. Keep parser focused on structure construction + recovery only.
4. Preserve diagnostic ownership determinism and snapshot stability.
5. Introduce explicit structural-phase status contract for driver gating.
6. Continue moving remaining parser-owned structural exceptions to
   `syntax_rules` as syntax representation is extended.

### Phase 5: phase-status contract formalization (incremental)

1. Add a small explicit result type for `syntax_rules` (example:
   `SyntaxRulesResult { diagnostics, valid_for_semantic }`).
2. Replace driver-local inferred gating with this contract.
3. Optionally expand the same pattern to adjacent phases where it improves
   clarity.
4. Keep full boundary-envelope unification optional and incremental.

## Compatibility and Risk

### What should remain stable

- User-facing diagnostic messages (unless intentionally improved).
- Recovery behavior and number of diagnostics per fixture.
- Existing AST shapes (except where improved for correctness/clarity).

### Risks

- Recovery regressions if `Err` propagation bypasses synchronization.
- Snapshot churn if error timing/locations change.
- Partial migration inconsistency (`Option`/`Result` mixed in the same layer).
- Confusion between phase diagnostics and fatal errors if contracts are
  underspecified.
- Over-abstracting phase contracts without real semantic benefit.
- Mixed long-term patterns where some parser paths emit diagnostics at leaf
  sites while others rely on boundary emission.
- Implicit gating logic in driver can drift from phase intent and cause
  diagnostic cascades.

### Mitigations

- Convert by parser layer (leaf -> sequence -> declarations -> entrypoint).
- Keep explicit recovery boundaries in converted callers.
- Run diagnostics snapshots after each phase.
- Document explicit fatal-vs-diagnostic rules per phase.
- For boundary unification, require a concrete before/after benefit at each
  adoption site (clearer composition, better tooling output, or stronger
  invariants).
- Track diagnostic emission ownership by parser layer during migration.
- Define explicit per-phase gating semantics and keep them in phase-owned
  contracts.

## Acceptance Criteria

1. Parser internals no longer use `Option` as a parse-failure channel.
2. Optional syntax is represented without nested option return types.
3. Phase entrypoints either use a consistent boundary envelope
   (`CompilerPhaseResult`) or explicitly document why a different boundary shape
   is clearer for that phase.
4. `tests/diagnostics` pass with expected single-error fixture behavior rules
   intact.
5. Generics/constraints parsing remains correct and readable.
6. Parser failure modes are explicit enough to support resilient future LSP
   parsing/editing workflows.
7. If boundary unification is adopted, it demonstrates concrete semantic/tooling
   value rather than only API-shape consistency.
8. Parser final form clearly separates structured parse failures from user
   diagnostic rendering responsibilities.
9. Errorful files still produce useful parse outputs and diagnostics for tooling
   flows.
10. Parseable-but-invalid structural constraints are owned by structural
    validity phase, not parser internals.
11. Driver gating for structural invalidity is based on explicit phase status,
    not incidental diagnostic side effects.
12. Doc-comment placement diagnostics are owned by `syntax_rules` and derived
    from syntax-owned ordered items.

## Target End State

The intended parser end state is:

1. Structured control-flow failures:
   - parser internals use `ParseResult<T>`
   - `ParseError` carries actionable parse/recovery metadata

2. Explicit recovery ownership:
   - callers decide synchronization/recovery from `Err(ParseError)`
   - recovery/boundary points emit user diagnostics once per incident

3. Clear diagnostic boundary:
   - leaf helpers avoid long-term user-diagnostic ownership
   - user-facing message rendering happens with broader syntactic context

4. Tooling-compatible outputs:
   - parser generally provides syntax output + diagnostics under syntax errors
   - fatal errors remain reserved for infrastructure/invariant failures

## LSP/Tooling Implications

This migration should preserve and strengthen the ability to support interactive
tools:

1. Parser resilience:
   - Continue producing useful syntax trees under error conditions where
     practical.
   - Keep recovery explicit and localized.

2. Structured failures:
   - Parse failures should carry enough structure for caller decisions and
     editor diagnostics.

3. Stable phase outputs:
   - Unified boundary envelopes are useful if they provide predictable partial
     outputs and clear fatal/error semantics.
   - They are not a goal by themselves; tooling value is the goal.

4. Shared frontend behavior:
   - Structural validity diagnostics should come from a dedicated phase that can
     be reused consistently by CLI and LSP.
   - Parser should remain resilient and avoid policy-specific branching that is
     unrelated to syntax construction.

5. Predictable gating semantics:
   - Tooling and CLI should observe the same phase-gating behavior from explicit
     status contracts.
   - This avoids mode-specific cascade differences.

## Lexer/Parser Final Ownership Model

To avoid long-term diagnostic drift and duplication, the intended end state is:

1. Lexing owns lexical failures as structured `LexError` values.
2. Parsing owns syntactic failures as structured `ParseError` values.
3. Parse-phase boundaries own user-facing diagnostic materialization.

Implications:

- Lexer and parser internals should prefer returning structured errors over
  directly emitting user diagnostics from deep helper functions.
- `parse_file` (or equivalent parse-phase boundary) should aggregate lex+parse
  structured failures into user-facing diagnostics once, with deterministic
  ordering and de-duplication policy.
- Temporary migration bridges are acceptable, but should converge toward this
  model.

## Unified Frontend Goal (Compiler + Tooling)

If compiler checking and LSP/tooling are expected to remain as unified as
possible, this migration should optimize for a shared frontend core:

1. Shared lexer/parser/type front-end libraries with stable contracts.
2. One internal diagnostic data model (machine-readable first).
3. Multiple consumers/adapters:
   - CLI formatting/reporting
   - LSP diagnostics and related tooling features

This favors:

- structured errors internally
- boundary-level user diagnostic rendering
- minimal mode-specific branching inside parser internals

and disfavors:

- duplicated parser logic across compiler/tooling
- ad-hoc side-effect diagnostics scattered through deep parse helpers

## Alternatives Considered

### A. Keep widespread direct side-effect diagnostics in lexer/parser internals

Pros:

- low short-term migration effort

Cons:

- harder to prevent duplicate diagnostics
- weaker failure contracts as grammar complexity grows
- higher risk of divergence between CLI and tooling behavior

### B. Parser-only diagnostics, lexer never reports lexical failures directly

Pros:

- one emitter surface

Cons:

- lexical error fidelity can be degraded or awkwardly represented
- parser layer takes on failure modes it does not own conceptually

### C. Emit everywhere, deduplicate later with heuristics

Pros:

- easiest incremental wiring

Cons:

- brittle dedup logic
- less deterministic behavior
- harder to audit/maintain as language features expand

### Preferred Long-Term Direction

Use owner-typed structured errors (`LexError` / `ParseError`) plus one
parse-phase aggregation boundary for user diagnostics. This best supports:

1. full language expansion without diagnostic chaos
2. clean maintainable architecture boundaries
3. unified compiler + tooling front-end behavior

## Non-Goals (this migration)

- Rewriting all diagnostics formatting.
- Adding advanced parser combinator framework.
- Introducing inference or other semantic/type-system changes.

## Implementation Status (Current)

This section records current implementation state so the spec remains aligned
with reality.

### Completed

1. `ParseResult` migration is implemented across major parser modules:
   - recovery helpers (`expect_*`)
   - types
   - imports/exports
   - statements
   - expressions
   - declarations
   - top-level declaration dispatch
2. Parser control-flow now uses structured `ParseError` variants in many
   previously `Option`-based failure paths.
3. Boundary/recovery catch points now report parse errors through a centralized
   `report_parse_error(...)` path.
4. Structural phase (`syntax_rules`) is implemented and integrated in driver
   orchestration before semantic resolution.
5. Imports-at-top and doc-comment placement diagnostics are owned by
   `syntax_rules`.
6. Syntax now preserves ordered doc-comment/declaration/member items needed for
   structural validity ownership.

### Transitional (intentional, not final form)

1. Parser currently renders parse-error messages from typed `ParseError` kinds
   at the parser boundary (`report_parse_error`); this is acceptable while the
   parser owns those failures.

### Next Steps

1. Keep phase-status contracts explicit and phase-owned for downstream gating.
2. Continue de-stringifying parser failure metadata where practical and keep
   user-facing rendering centralized at boundaries.
3. Expand syntax losslessness incrementally (beyond doc comments) as tooling
   features require it.
