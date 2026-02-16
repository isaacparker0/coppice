# Compiler Architecture

Authoritative architecture specification for the current compiler design.

---

## Purpose

This architecture exists to support Coppice language goals:

1. One obvious constrained way to implement language semantics.
2. Build-time explicitness and deterministic behavior.
3. Clear ownership boundaries that are easy to enforce in the build graph.
4. Refactorability without accidental cross-phase coupling.

---

## Core Principles

1. Own concepts in the earliest phase that has enough information.
2. Separate syntax representation from semantic representation.
3. Keep orchestration separate from semantic ownership.
4. Depend on stable contracts, not neighboring implementation internals.
5. Enforce dependency direction with analysis-time build tests.

---

## Canonical Pipeline

1. Lexing and parsing
2. Syntax structural validity checks
3. File-role policy checks
4. Package/import/export/visibility resolution
5. Semantic lowering (`syntax` -> `semantic_program`)
6. Type analysis
7. Driver orchestration and diagnostic rendering

---

## Package Ownership

## `compiler/parsing`

Owns lexical and syntactic parsing.

Produces `compiler/syntax::ParsedFile`.

## `compiler/syntax`

Owns parser-facing syntax AST.

This is the representation of source structure, not semantic meaning.

## `compiler/syntax_rules`

Owns syntax-adjacent language validity rules that require parsed structure but
not semantic/type information.

Examples:

- declaration-order constraints (for example imports-at-top-of-file)
- doc-comment placement constraints (doc comments must attach to declarations)
- file-level structural constraints independent of file role

## `compiler/file_role_rules`

Owns role-based language policy (library/binary/test/package-manifest rules).

## `compiler/symbols`

Owns package-local symbol discovery.

## `compiler/exports`

Owns package export manifest validation.

## `compiler/visibility`

Owns cross-package visibility/import legality.

## `compiler/package_graph`

Owns package dependency graph and cycle diagnostics.

## `compiler/binding`

Owns local binding conflict checks from declarations/imports.

## `compiler/semantic_program`

Owns semantic pass input data model.

- Contains semantic declarations/statements/expressions.
- Single source of truth for top-level declaration ordering.
- Deliberately independent from parser AST crate boundaries.

## `compiler/semantic_lowering`

Owns conversion from syntax AST to semantic program.

- Preserves spans for diagnostics.
- Central place for semantic normalization/desugaring decisions.

## `compiler/semantic_types`

Owns shared semantic type contracts:

- canonical type enum
- stable nominal identity (`NominalTypeId`)
- imported/public typing transport contracts

## `compiler/package_symbols`

Owns typed public package symbol contracts used for cross-package type analysis.

Consumes semantic program declarations for public API extraction.

## `compiler/type_analysis`

Owns type analysis implementation.

Consumes:

- semantic program units
- imported typed symbol contracts
- semantic type contracts

Emits:

- type diagnostics
- per-file typed symbol summaries

## `compiler/driver`

Owns orchestration only.

Responsibilities:

- pass ordering
- workspace/package scoping
- lowering invocation
- diagnostic aggregation and ordering

Non-responsibility:

- no semantic inference loops or pass-local semantic ownership

---

## Data Boundary Contracts

## Syntax boundary

`compiler/syntax::ParsedFile` is the parser output contract only.

Long-term direction for this boundary:

1. `syntax` should be lossless enough for tooling and structural policy checks
   (including comments/trivia/span fidelity where needed).
2. Structural validity phases should not depend on parser-internal side effects
   or transient parser-local state.
3. Semantic lowering remains the boundary where syntax-trivia concerns are
   dropped for semantic representation.

## Semantic boundary

`compiler/semantic_program::PackageUnit` is the semantic pass input contract.

## Cross-package type boundary

`compiler/semantic_types` + `compiler/package_symbols` define the typed import
contract for downstream analysis.

---

## Dependency Direction (Normative)

High-level direction:

1. `parsing -> syntax`
2. `syntax_rules -> syntax`
3. semantic resolution passes depend on parser/syntax outputs as needed
4. `syntax -> semantic_lowering -> semantic_program`
5. `package_symbols -> semantic_program + semantic_types`
6. `type_analysis -> semantic_program + semantic_types`
7. `driver` depends on all phase crates for orchestration

Key prohibitions:

1. `type_analysis` must not depend on `syntax`.
2. `package_symbols` must not depend on `syntax`.
3. Semantic phase crates must not depend on `driver`.

These are enforced by Bazel `dependency_enforcement_test` targets.

---

## Diagnostic Ownership

Diagnostics must be emitted by the phase that owns the violated rule.

Rules:

1. Parsing errors come from parsing.
2. Syntax structural validity errors come from syntax rules.
3. File-role policy errors come from file-role rules.
4. Import/export/visibility errors come from semantic resolution passes.
5. Type errors come from type analysis.
6. Driver renders and sorts diagnostics; it does not invent semantic rules.

## Phase Gating Policy

Phase ordering is not only about execution order. It also defines prerequisites
for later analysis.

Rules:

1. A phase may emit diagnostics and still allow later phases to run when results
   remain semantically meaningful.
2. A phase must block downstream phases for units that fail required
   prerequisites for downstream correctness/noise control.
3. The driver should gate by explicit per-unit phase status contracts, not
   inferred heuristics.

Current policy direction:

1. Parse failures block downstream phases for that file.
2. Syntax structural validity failures block semantic resolution/lowering/type
   analysis for that file.
3. File-role failures do not necessarily block all downstream phases unless a
   specific rule requires it.

## Ownership Rubric

Use this rubric whenever phase ownership is ambiguous:

1. Parsing owns failures where reliable syntax structure cannot be built from
   tokens.
2. Syntax rules own failures where structure is parseable but violates
   language-declared structural constraints.
3. Semantic/type phases own failures that require name, visibility, type, or
   usage information.

## Phase Output Contract Direction

Near-term recommendation:

1. Each phase should expose explicit per-unit status sufficient for driver
   gating (for example `valid_for_semantic`).
2. Avoid coupling gating behavior to incidental details such as "any diagnostics
   emitted".

Long-term recommendation:

1. Evolve toward a shared phase envelope with explicit status and diagnostics.
2. Keep this incremental; do not require all phases to switch at once.

---

## Why This Architecture Is Good

1. Deterministic behavior: each rule has a single owner and input boundary.
2. Explicit data flow: syntax -> semantic program -> analysis contracts.
3. Low coupling: semantic passes are insulated from parser-shape churn.
4. Build-time enforceability: dependency tests prevent architecture drift.
5. Refactor safety: internal pass changes do not require broad cross-crate
   edits.

---

## Language-Design Alignment

This architecture directly supports current language direction:

1. Explicit declarations over inference-heavy cross-package glue.
2. Enum and union semantics implemented in semantic passes, not orchestration.
3. Constant type annotation policy enforced in semantic/type phases.
4. No literal singleton types; closed sets modeled explicitly.

---

## Terminology Clarification

`semantic_program` is frontend semantic representation.

It is intentionally **not** named `ir` to avoid confusion with future backend IR
layers (for example mid-level or codegen IR for Cranelift/LLVM paths).

---

## Future Goals

1. Increase semantic normalization in `semantic_lowering` so analysis consumes
   simpler canonical forms.
2. Tighten and expand dependency invariants as new semantic passes are added.
3. Introduce backend-oriented IR layers with distinct names and ownership.
4. Maintain strict phase-owned diagnostics as language features grow (notably
   generics and trait/interface semantics).
5. Evolve `syntax` toward lossless source representation suitable for unified
   compiler + tooling front-end use (comments/trivia preserved where needed).

---

## Non-Goals

1. Reintroducing compatibility facade crates without strong ownership value.
2. Allowing driver-owned semantic behavior.
3. Blurring parser AST and semantic pass representations.
4. Weakening enforced dependency direction for short-term convenience.
