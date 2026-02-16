# Compiler Architecture Specification

## Status

Active draft. This document defines current compiler phase ownership, target
architecture, and migration invariants.

Last refreshed for repository state on February 16, 2026.

---

## Purpose

This spec exists to keep compiler boundaries explicit and stable as the language
expands.

Primary goals:

1. Keep dependency direction clean and enforceable.
2. Avoid phase leakage (for example, one pass consuming another pass's internal
   data shape).
3. Define a clear endpoint for package/import/typechecking architecture.
4. Provide a concrete migration plan from current state to target state.

---

## Architecture Principles

1. Organize modules by concept ownership, not convenience.
2. Depend on stable interfaces/data models, not implementation internals.
3. Use explicit named data structures; avoid opaque tuple keys.
4. Keep public API surfaces narrow.
5. Choose the earliest phase that has sufficient information for a rule.

---

## Canonical Pipeline

1. Lexing
2. Parsing (`compiler/parsing`, AST in `compiler/syntax`)
3. File-role policy checks (`compiler/file_role_rules`)
4. Package/import semantic resolution subpasses
5. Typechecking
6. Driver orchestration and diagnostic rendering (`compiler/driver`)

---

## Phase Ownership

## Parsing

Parsing owns syntax and structural canonical-form constraints that are decidable
from a single file's tokens/AST.

Examples:

1. Invalid syntax
2. Invalid token sequences
3. Structural ordering constraints (for example, import declarations must appear
   before non-import top-level declarations)

Parsing does not own cross-file/package/type semantics.

## File-Role Policy

File-role policy owns role-conditioned language rules that are not type
reasoning.

Examples:

1. `*.bin.coppice` `main` rules
2. `main` forbidden in library/test roles
3. `public` forbidden in binary/test files
4. `exports` only valid in `PACKAGE.coppice`

## Package/Import Semantic Resolution

Semantic resolution owns symbol/package/import/export relationships and
visibility legality.

Subpass ownership:

1. `compiler/symbols`: package symbol collection
2. `compiler/exports`: manifest export validation
3. `compiler/visibility`: import target/member accessibility resolution
4. `compiler/package_graph`: package dependency edges and cycle checks
5. `compiler/binding`: local name-binding conflicts from imports/declarations
6. `compiler/package_symbols`: typed cross-package symbol contracts for
   downstream type analysis

## Semantic Type Model

`compiler/semantic_types` owns stable semantic typing contracts shared across
analysis phases.

Examples:

1. Canonical semantic type enum used outside parser internals
2. Stable nominal type identity (`NominalTypeId`)
3. Imported/public symbol contract transport models

## Type Analysis

`compiler/type_analysis` owns semantic-program-based type analysis.

It consumes lowered semantic program units plus typed import contracts and emits
typing diagnostics and file summaries.

## Typechecking

`compiler/typecheck` is currently a thin public compatibility facade over
`compiler/type_analysis`.

It preserves external entrypoints while architectural migration is in progress.

Examples:

1. Unknown type names in semantic typing context
2. Assignment/call/return mismatches
3. Control-flow narrowing consistency
4. Match typing/exhaustiveness behavior

Typechecking must consume stable semantic contracts, not resolver or parser
implementation internals.

## Driver

Driver orchestrates pass order, scoping, and diagnostic aggregation.

Driver must not become semantic owner; it wires owned passes together.

---

## Current Code Mapping (As Of This Spec)

1. Parsing/AST
   - `compiler/parsing/*`
   - `compiler/syntax/*`
2. File-role policy
   - `compiler/file_role_rules/*`
3. Semantic resolution
   - `compiler/symbols/*`
   - `compiler/exports/*`
   - `compiler/visibility/*`
   - `compiler/package_graph/*`
   - `compiler/binding/*`
   - `compiler/package_symbols/*`
4. Semantic type contracts
   - `compiler/semantic_types/*`
5. Type analysis implementation
   - `compiler/type_analysis/*`
6. Semantic program model + lowering
   - `compiler/semantic_program/*`
   - `compiler/semantic_lowering/*`
7. Typecheck compatibility facade
   - `compiler/typecheck/*`
8. Driver
   - `compiler/driver/*`

---

## Dependency Direction (Current)

Current intended high-level direction:

1. `symbols -> exports`
2. `symbols + exports -> visibility`
3. `visibility -> package_graph`
4. `visibility + symbols -> binding`
5. `semantic_types` is shared by `package_symbols` and `type_analysis`
6. `syntax -> semantic_lowering -> semantic_program -> type_analysis`
7. `type_analysis + semantic_types -> typecheck` (facade only)
8. `driver` orchestrates `package_symbols` outputs and `typecheck` execution
9. `driver` depends on phase crates; phase crates do not depend on `driver`

---

## Target End-State Architecture

The long-term clean architecture is:

1. `compiler/syntax`
   - parser AST only
2. `compiler/semantic_types`
   - canonical semantic type representation
   - includes stable IDs for nominal references
3. `compiler/semantic_program`
   - lowered semantic program representation for semantic passes
4. `compiler/semantic_lowering`
   - AST (`syntax`) -> semantic program conversion
5. `compiler/package_symbols`
   - package-level typed symbol contracts keyed by stable identities
6. `compiler/typecheck`
   - consumes semantic program + semantic types + package symbol contracts
   - does not depend on parser AST

In this endpoint, cross-package imports are identity-based and typed; aliasing
is purely local name binding and does not require type-name string rewrites.

Current progress already achieved toward this endpoint:

1. Shared semantic type contracts live in `compiler/semantic_types`.
2. Cross-package nominal type identity is stable-ID-based in semantic types.
3. Alias rewriting for cross-package type identity has been removed.

---

## Hard Dependency Invariants (Target)

These are normative target invariants:

1. `typecheck` must not depend on `compiler/syntax`.
2. `typecheck` must consume semantic program + typed package symbol contracts.
3. Cross-package named-type identity must be stable-ID-based, not string-based.
4. `package_symbols` owns cross-package symbol contracts; `typecheck` does not
   own driver-facing transport models.
5. Driver is orchestration-only and does not duplicate pass semantics.

Invariants already realized in current code:

1. Cross-package named-type identity is stable-ID-based.
2. `package_symbols` owns cross-package typed contracts.
3. `package_symbols` has an enforced forbidden dependency on
   `//compiler/typecheck`.

Temporary coupling still present (known debt):

1. `typecheck` still provides a compatibility facade over `type_analysis`
   instead of owning semantic analysis directly.
2. `package_symbols` still consumes parser-owned declarations directly and has
   not yet migrated to semantic-program-owned declarations.

---

## Dependency Invariant Enforcement Mechanism

Dependency graph invariants are enforced with Bazel analysis-time tests:
`dependency_enforcement_test` in
`tools/bazel/aspects/dependency_enforcement.bzl`.

Mechanism details:

1. The rule applies an aspect with `attr_aspects = ["*"]` to compute transitive
   target reachability.
2. Each invariant declares a `target` and `forbidden` label list.
3. Analysis fails if a forbidden label is reachable, with one concrete
   dependency path in the failure output.
4. There is no exception/allowlist mechanism; invariants are hard fail once
   added.

Placement policy:

1. Invariants are colocated in the relevant `compiler/*/BUILD.bazel` file.
2. Do not maintain a global registry of invariants.
3. One target may use a single `*_forbidden_dependencies` test and grow its
   `forbidden` list over time.

Example:

```starlark
dependency_enforcement_test(
    name = "syntax_forbidden_dependencies",
    target = ":syntax",
    forbidden = ["//compiler/typecheck"],
)
```

---

## Why `package_symbols` Is Separate From `packages`

`compiler/packages` owns package identity primitives (for example `PackageId`).

`compiler/package_symbols` owns semantic symbol contracts and typed public API
surfaces across packages.

Keeping these separate:

1. Preserves concept ownership boundaries.
2. Prevents workspace/discovery layers from depending on semantic typing models.
3. Keeps dependencies cleaner and future refactors safer.

---

## Rule Placement Rubric

1. If a rule depends only on token/AST structure and declaration ordering:
   Parsing.
2. If a rule depends on file role/policy but not type reasoning: File-role
   policy.
3. If a rule depends on package symbols/imports/exports/visibility or cross-file
   identity: Semantic resolution.
4. If a rule depends on expression/statement types/control-flow typing:
   Typechecking.

If a rule appears to fit multiple phases, place it in the earliest phase with
sufficient information and no duplication.

---

## Migration Plan To Target Invariants

## Phase A: Boundary Cleanup (complete)

1. Move typed package symbol contracts out of `typecheck` into
   `compiler/package_symbols`.
2. Ensure driver consumes `package_symbols` API directly.
3. Keep behavior stable and diagnostics unchanged.

## Phase B: Semantic Type Identity (complete)

1. Introduce stable IDs for nominal type identity in semantic type model.
2. Replace stringly named-type joins at package boundaries.
3. Ensure import aliasing remains local-binding only.

## Phase C: Type Analysis Extraction (complete)

1. Move AST-based type analysis implementation into `compiler/type_analysis`.
   (Historical step; now migrated to semantic-program-based input.)
2. Keep `compiler/typecheck` as a thin compatibility facade.
3. Ensure diagnostics parity while changing dependency direction.

## Phase D: Semantic Program Introduction (complete)

1. Define semantic program structures for declarations/statements/expressions.
2. Add lowering from AST to semantic program with span preservation.
3. Keep diagnostics parity while migrating pass consumers.

## Phase E: Typecheck Migration To Semantic Program (in progress)

1. Switch typecheck to semantic program.
2. Remove `compiler/syntax` dependency from `compiler/typecheck`.
3. Add colocated `dependency_enforcement_test` invariant and enforce in CI.

## Phase F: Enforcement Tightening (ongoing)

1. Encode phase-boundary invariants as colocated `dependency_enforcement_test`
   targets.
2. Keep CI running these invariants under `bazel test`.
3. Add each invariant in the same change that completes the corresponding
   migration step.

## Phase G: Constant Annotation Enforcement (complete)

1. Enforce explicit type annotations on all constant declarations.
2. Remove public constant fixed-point inference from `compiler/driver`.
3. Keep `compiler/driver` orchestration-only by wiring declaration-owned pass
   outputs without semantic inference loops.
4. Add/enforce dependency invariants guaranteeing package contracts are built
   from explicit declarations rather than driver-owned inference.

## Phase H: Enum Surface Syntax (complete)

1. Add parser + AST support for explicit closed-set enum declarations:
   `type Direction :: enum { North, South, ... }`.
2. Keep `A | B` as union composition over existing types only.
3. Forbid implicit enum-variant synthesis from unresolved union members.
4. Ensure enum lowering keeps existing deterministic type-analysis and
   exhaustiveness behavior, with diagnostics owned by parsing/type-analysis (not
   driver).

---

## Diagnostic Ownership

Diagnostics must be emitted by the owning phase for the violated rule.

Guideline:

1. Prefer single-source ownership per rule.
2. Avoid duplicate diagnostics for the same root cause across phases.
3. Driver only renders/orders diagnostics; it should not invent semantic
   diagnostics.

---

## Non-Goals

1. Forcing all compiler internals into one giant shared crate.
2. Premature optimization that obscures boundaries.
3. Preserving accidental legacy entrypoints purely for backward compatibility.

---

## Summary

The compiler should evolve toward stable semantic boundaries:

1. AST for parsing only
2. semantic program for semantic passes
3. typed, ID-based package symbol contracts for cross-package analysis
4. strict dependency direction enforced by build graph

This is the maintainable and scalable path for the full language implementation.
