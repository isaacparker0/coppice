# Parser `Option` to `Result` Migration

## Purpose

Define a migration from `Option`-based parser internals to `Result`-based
parsing, and align that change with a unified compiler-phase boundary contract.

This document now also evaluates these choices against future LSP/tooling needs,
where partial results, resilient recovery, and explicit failure semantics matter
as much as command-line batch compilation.

## Context

The parser currently uses `Option<T>` in many functions:

- `Some(T)` means parse succeeded.
- `None` means parse failed (often after emitting a diagnostic and/or attempting
  recovery).

This worked for early syntax, but generics and constraints increase grammar
depth and failure modes. `Option` does not encode _why_ parsing failed, and
nested optionality (`Option<Option<T>>`) can appear when modeling optional
grammar elements.

## Current State

### Parser behavior

- Parse functions frequently return `Option<T>`.
- Diagnostics are emitted as side effects (`self.error(...)`).
- Recovery is done by synchronization helpers (`synchronize_*`) in callers.

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

### Mitigations

- Convert by parser layer (leaf -> sequence -> declarations -> entrypoint).
- Keep explicit recovery boundaries in converted callers.
- Run diagnostics snapshots after each phase.
- Document explicit fatal-vs-diagnostic rules per phase.
- For boundary unification, require a concrete before/after benefit at each
  adoption site (clearer composition, better tooling output, or stronger
  invariants).

## Acceptance Criteria

1. Parser internals no longer use `Option` as a parse-failure channel.
2. Optional syntax is represented without nested option return types.
3. Phase entrypoints use a consistent boundary envelope (`CompilerPhaseResult`).
4. `tests/diagnostics` pass with expected single-error fixture behavior rules
   intact.
5. Generics/constraints parsing remains correct and readable.
6. Parser failure modes are explicit enough to support resilient future LSP
   parsing/editing workflows.
7. If boundary unification is adopted, it demonstrates concrete semantic/tooling
   value rather than only API-shape consistency.

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

## Non-Goals (this migration)

- Rewriting all diagnostics formatting.
- Adding advanced parser combinator framework.
- Introducing inference or other semantic/type-system changes.
