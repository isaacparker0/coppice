# Compiler Architecture Specification (Draft)

## Status

Draft. This document defines phase boundaries in the Coppice compiler and the
ownership rules for diagnostics.

---

## Purpose

This spec exists to prevent "rule drift" where structural, role-based, and
semantic checks get mixed into arbitrary phases.

The immediate design goal is a precise answer to:

1. What belongs in parsing?
2. What belongs in policy/resolution/typechecking?
3. Why file-role checks belong in policy, not parsing.

---

## Pipeline Ownership

Canonical compile pipeline (conceptual):

1. **Lexing**
   - Convert source text to tokens.
2. **Parsing**
   - Build AST from tokens.
   - Emit syntax and structural canonical-form diagnostics.
3. **Role/Policy Analysis (pre-typecheck)**
   - Validate file-role-dependent language policy.
4. **Semantic Analysis / Resolution**
   - Build and validate symbol/import/export/package relationships.
5. **Typechecking**
   - Validate expression and statement typing semantics.
6. **Driver**
   - Orchestrate phase execution order and aggregate diagnostics.

---

## Phase Boundaries

## Parsing

Parsing owns rules that can be decided from one file's token stream + AST shape,
without symbol tables or cross-file/package context.

Examples:

1. malformed syntax
2. invalid token sequences
3. canonical structural ordering constraints (for example: `import` declarations
   must appear before non-import top-level declarations)

Non-goals for parser:

1. cross-file symbol validity
2. package graph semantics
3. type validity
4. file-role semantic policy beyond what is required to parse

## Role/Policy (Pre-Typecheck Passes)

Role/policy owns language policy that is not type reasoning but does require
semantic context such as file role.

Examples:

1. `.bin.coppice` must declare exactly one `main`
2. `.bin.coppice` `main` signature constraints
3. `main` forbidden in library/test/manifest roles
4. `public` forbidden in binary/test files
5. `exports` declarations only valid in `PACKAGE.coppice`

This is exactly the responsibility of `compiler/file_role_rules/lib.rs`.

## Semantic Analysis / Resolution

Owns symbol/package/import/export semantics requiring symbol tables and/or
multi-file context.

Examples:

1. unknown imported member
2. importing file-private symbol from sibling file
3. external import requires symbol to be both `public` and exported
4. duplicate package-visible names across files
5. export table validation (`unknown`, `duplicate`, `non-public`)

## Typechecking

Owns type rules once names/symbols resolve.

Examples:

1. unknown type names
2. assignment/call/return mismatches
3. control-flow/type narrowing consistency
4. match arm typing and exhaustiveness

---

## Decision Rubric For New Rules

Use this deterministic placement rubric:

1. If rule depends only on token/AST structure and declaration ordering:
   **Parsing**.
2. If rule depends on file role or language policy but not type inference:
   **Role/policy pre-typecheck**.
3. If rule depends on symbol tables, imports/exports, package boundaries, or
   cross-file identity: **Semantic analysis/resolution**.
4. If rule depends on expression/statement types or control-flow typing:
   **Typechecking**.

If a rule seems to fit multiple phases, choose the earliest phase that has all
required information and does not require duplicating logic elsewhere.

---

## Why File-Role Rules Stay In Policy

`file_role_rules` should not move into parser.

Reasons:

1. These checks are semantic policy driven by role metadata (`Library`,
   `BinaryEntrypoint`, `Test`, `PackageManifest`), not grammar.
2. Keeping parser focused on syntax/shape improves parser clarity and recovery.
3. Role/policy is the natural owner for all non-syntax language policy
   diagnostics.
4. This keeps a clean extension point for future role-based policy without
   inflating parser responsibilities.

Practical exception:

1. Parser may still accept role as input for parse behavior selection.
2. But role-conditioned semantic diagnostics should remain in policy passes.

---

## Current Code Mapping

1. Parsing:
   - `compiler/parsing/*`
2. File-role policy analysis:
   - `compiler/file_role_rules/lib.rs`
3. Semantic analysis / resolution:
   - `compiler/resolution/*`
4. Typechecking:
   - `compiler/typecheck/*`
5. Driver:
   - `compiler/driver/*`

This mapping is intentional and should be preserved as package/import/export
semantics are added.
