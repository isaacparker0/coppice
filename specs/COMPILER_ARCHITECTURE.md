# Compiler Architecture Specification

## Status

Active draft. This document defines current compiler phase ownership, target
architecture, and migration invariants.

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
   typecheck import environments

## Typechecking

Typechecking owns expression/statement typing semantics and related diagnostics.

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
4. Typechecking
   - `compiler/typecheck/*`
5. Driver
   - `compiler/driver/*`

---

## Dependency Direction (Current)

Current intended high-level direction:

1. `symbols -> exports`
2. `symbols + exports -> visibility`
3. `visibility -> package_graph`
4. `visibility + symbols -> binding`
5. `package_symbols + binding -> typecheck` (driver orchestrated)
6. `driver` depends on all phase crates; phase crates do not depend on `driver`

---

## Target End-State Architecture

The long-term clean architecture is:

1. `compiler/syntax`
   - parser AST only
2. `compiler/semantic_types` (name may be `compiler/types` if preferred)
   - canonical semantic type representation
   - includes stable IDs for nominal references
3. `compiler/semantic_ir`
   - lowered semantic program IR for typecheck and later semantic/codegen phases
4. `compiler/lowering`
   - AST (`syntax`) -> semantic IR conversion
5. `compiler/package_symbols`
   - package-level typed symbol contracts keyed by stable identities
6. `compiler/typecheck`
   - consumes semantic IR + semantic types + package symbol contracts
   - does not depend on parser AST

In this endpoint, cross-package imports are identity-based and typed; aliasing
is purely local name binding and does not require type-name string rewrites.

---

## Hard Dependency Invariants (Target)

These are normative target invariants:

1. `typecheck` must not depend on `compiler/syntax`.
2. `typecheck` must consume semantic IR + typed package symbol contracts.
3. Cross-package named-type identity must be stable-ID-based, not string-based.
4. `package_symbols` owns cross-package symbol contracts; `typecheck` does not
   own driver-facing transport models.
5. Driver is orchestration-only and does not duplicate pass semantics.

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

## Phase A: Boundary Cleanup (complete/in progress)

1. Move typed package symbol contracts out of `typecheck` into
   `compiler/package_symbols`.
2. Ensure driver consumes `package_symbols` API directly.
3. Keep behavior stable and diagnostics unchanged.

## Phase B: Semantic Type Identity

1. Introduce stable IDs for nominal type identity in semantic type model.
2. Replace stringly named-type joins at package boundaries.
3. Ensure import aliasing remains local-binding only.

## Phase C: Semantic IR Introduction

1. Define semantic IR structures for declarations/statements/expressions.
2. Add lowering from AST to semantic IR with span preservation.
3. Keep diagnostics parity while migrating pass consumers.

## Phase D: Typecheck Migration

1. Switch typecheck to semantic IR.
2. Remove `compiler/syntax` dependency from `compiler/typecheck`.
3. Enforce dependency invariant in BUILD/CI.

## Phase E: Enforcement

1. Bazel deps enforce intended direction.
2. Add CI guardrails preventing forbidden imports (for example
   `typecheck -> syntax`).

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
2. semantic IR for semantic passes
3. typed, ID-based package symbol contracts for cross-package analysis
4. strict dependency direction enforced by build graph

This is the maintainable and scalable path for the full language implementation.
