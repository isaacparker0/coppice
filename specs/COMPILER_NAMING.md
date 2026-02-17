# Compiler Naming (Draft)

## Purpose

This document defines canonical naming for compiler representations, lowering
phases, and planned backend packages.

It exists to prevent ambiguity (especially around multiple internal
representations that are all technically IRs) and to keep names aligned with
`docs/styleguide.md` (full words, clear responsibilities, minimal jargon).

Companion docs:

- `specs/COMPILER_ARCHITECTURE.md`
- `specs/SAFETY_BACKEND_STRATEGY.md`
- `specs/BACKEND_IMPLEMENTATION_PLAN.md`

Status: Draft.

---

## Naming Rules

1. Use full words over abbreviations by default.
2. Prefer representation names based on semantic role, not internal compiler
   jargon.
3. For lowering phase package names, use `<target>_lowering`.
4. For conversion function names, include both source and target when it
   improves clarity.
5. For model/data types that exist in multiple compiler representations, prefer
   globally unambiguous type names over local import aliasing.
6. For cross-representation model types, use canonical phase prefixes:
   - `Syntax*` for syntax representation types.
   - `Semantic*` for semantic program representation types.
   - `TypeAnnotated*` for type-annotated representation types.
   - `Executable*` for executable representation types.
7. Use consistent suffixes by semantic role:
   - `*Declaration` for declaration nodes.
   - `*Statement` / `*Expression` for executable/body nodes.
   - `*Signature` for callable/type contracts.

Examples:

- Good package names: `semantic_lowering`, `executable_lowering`
- Good function names:
  - `lower_parsed_file_to_semantic_file`
  - `lower_semantic_file_to_executable_file`
- Good model type patterns:
  - `SyntaxFunctionDeclaration`
  - `SemanticFunctionDeclaration`
  - `TypeAnnotatedFunctionDeclaration`
  - `ExecutableStatement`

---

## Canonical Representation Terms

1. `syntax`

- Parse-oriented source structure fidelity.

2. `semantic_program`

- Frontend semantic representation for semantic/type analysis.

3. `executable_program` (planned)

- Backend-facing representation produced after type analysis and executable
  lowering.

Notes:

- `semantic_program` and `executable_program` are both internal compiler
  representations.
- Do not use bare `IR` in spec text when ambiguity is possible.
- If needed for brevity inside implementation comments, qualify explicitly as
  `semantic representation` or `executable representation`.

---

## Canonical Lowering Terms

1. `semantic_lowering`

- Lowers `syntax` to `semantic_program`.

2. `executable_lowering` (planned)

- Lowers semantic/typechecked artifacts to `executable_program`.

Convention:

- Lowering package/module names indicate target representation.

---

## Planned Package Names

Compiler packages (planned additions):

1. `compiler/executable_program`
2. `compiler/executable_lowering`
3. `compiler/runtime_interface`
4. `compiler/rust_backend` (optional first backend target)
5. `compiler/cranelift_backend` (future backend target)

Runtime packages (planned additions):

1. `runtime/core`
2. `runtime/sync`
3. `runtime/async`

---

## Terminology Guidance for Specs

When describing pipeline stages:

1. Prefer `semantic lowering` and `executable lowering` over generic
   `IR
   lowering`.
2. Prefer `runtime interface` over `ABI` unless discussing platform-level ABI
   details specifically.
3. Prefer `executable program` over `backend IR` in normative design text.

---

## Open Naming Questions

1. Whether to keep `runtime_interface` as the canonical term or adopt
   `runtime_boundary`.
2. Whether backend package names should remain target-specific (`rust_backend`,
   `cranelift_backend`) or follow `backend_<target>`.
