# Packages, Imports, and Visibility Specification (Draft)

## Status

Draft. This document defines the intended language design for multi-file
programs, package boundaries, import syntax, visibility, and dependency
resolution in Coppice.

This spec is normative for syntax and semantics in this area.

---

## Design Intent

This design is optimized for:

1. Explicit, obvious dependencies at file level.
2. One canonical way to import and reference cross-file symbols.
3. Strict and stable package APIs.
4. Deterministic, hermetic dependency resolution.
5. Direct mapping to Bazel package targets.

The model intentionally prefers explicitness over minimal boilerplate.

---

## Terminology

1. **File**: a `.coppice` source file.
2. **Package**: a directory containing `PACKAGE.coppice`, plus source files in
   subdirectories that do not contain their own `PACKAGE.coppice`.
3. **Symbol**: top-level declaration (type, function, constant).
4. **External**: from a different package path.
5. **Workspace root**: the root directory of the current Coppice workspace.
6. **First-party package path**: a workspace-root-relative package path.

---

## File Roles (Suffix Semantics)

File role is a first-class language decision. Using filename suffixes to define
entrypoints and tests is a focused generalization of patterns already seen in Go
(`_test.go`), Rust (`main.rs` vs `lib.rs`), and TypeScript (`.d.ts` files).
Lang0 commits to this fully because it aligns with our constraints: no dual-use
files, a single canonical structure, and deterministic build graph mapping.

File roles:

1. **Library file**: `*.coppice` excluding `.bin.coppice` and `.test.coppice`.
2. **Binary entrypoint file**: `*.bin.coppice`.
3. **Test file**: `*.test.coppice`.

Role is determined by filename only; contents do not change role.

---

## Package Boundaries

### Rule

A directory is a package if and only if it contains `PACKAGE.coppice`.

Without a nested `PACKAGE.coppice`, subdirectory files belong to the parent
package.

All Coppice commands are evaluated relative to workspace root. Invoking
`coppice` commands outside workspace root is a compile-time error unless a
workspace root is explicitly provided by CLI flag.

Any `.coppice` source file not owned by any package (no ancestor
`PACKAGE.coppice` up to workspace root) is a compile error.

### Example

```text
platform/
  auth/
    PACKAGE.coppice
    token.coppice
    password.coppice
    crypto/
      hash.coppice
    oauth/
      PACKAGE.coppice
      google.coppice
```

In this layout:

1. `platform/auth` is a package containing `token`, `password`, `crypto/hash`.
2. `platform/auth/oauth` is a separate package.

---

## File Suffix and Package Manifest

1. All language files use `.coppice`.
2. Package manifests are named `PACKAGE.coppice`.
3. `PACKAGE.coppice` allows only:
   - comments/doc comments
   - `exports ...` declarations that define package external API members
4. Any executable code or declarations in `PACKAGE.coppice` is a compile error.

---

## Binary Entrypoints

Rules for `*.bin.coppice`:

1. Must declare exactly one `main` function.
2. `main` must have no parameters and no return value.
3. `main` must be file-private (not `public`).
4. No `public` declarations are allowed in a binary entrypoint file.
5. A binary entrypoint file may not be imported by any other file.

Violations are compile errors anchored to the offending declaration or import.

---

## Library Files

Rules for `*.coppice` (non-bin, non-test):

1. Must not declare `main`.
2. Any `main` in a library file is a compile error.

---

## Test Files

Rules for `*.test.coppice`:

1. Must not declare `main`.
2. No `public` declarations are allowed.
3. A test file may not be imported by any other file.
4. Tests may import library symbols per normal visibility rules.

Violations are compile errors anchored to the offending declaration or import.

---

## Import Syntax

### Canonical Form (Only Form)

```lang
import package/path { Member, OtherMember, TypeName as Alias }
```

### Constraints

1. `package/path` is always a fully qualified, workspace-root-relative package
   path.
2. `package/path` denotes the package directory path (the directory containing
   `PACKAGE.coppice`).
3. `PACKAGE.coppice` itself is never written in import syntax.
4. Import list must be explicit named members.
5. Alias is optional and only per member (`as`).
6. Relative imports are forbidden.
7. Glob imports are forbidden.
8. Namespace/default imports are forbidden.
9. Inline fully-qualified symbol usage is forbidden.
10. Import declarations must appear before all top-level declarations in a
    source file.

### Consequence

There is exactly one way to bring cross-file symbols into scope.

---

## Visibility Model

Visibility is split across two declaration kinds with one keyword:

1. **Top-level declarations** (`type`, `function`, constants):
   - default: file-private
   - `public`: package-visible (eligible to be imported from other files in the
     same package)
   - externally visible only if `public` and listed in `PACKAGE.coppice` via
     `exports`
2. **Struct members** (fields, methods):
   - default: type-private (accessible only inside methods on that type)
   - `public`: accessible anywhere the type is accessible

`public` is intentionally contextual by declaration kind:

1. On top-level declarations it means import-eligible from other files in the
   same package.
2. On struct members it means accessible wherever values of that type are
   accessible.
3. Diagnostics must state which contextual meaning applies.

No file has implicit cross-file name visibility. Accessing declarations from
another file always requires an explicit `import`.

### Intent

1. Keep file/package API boundaries explicit for build graph clarity.
2. Keep member encapsulation explicit at type boundary.
3. Keep one external API surface defined only by `PACKAGE.coppice`.

---

## Import Resolution Rules

For `import A/B { X }` in file `f`:

1. Resolver locates package `A/B`.
2. Imports from `*.bin.coppice` or `*.test.coppice` files are illegal.
3. If `f` is in package `A/B`:
   - `X` must be package-visible (`public`) in some file of `A/B`.
   - file-private symbols are not importable.
4. If `f` is in a different package:
   - `X` must be `public`.
   - `X` must be listed by `exports` in `A/B/PACKAGE.coppice`.
5. Missing or inaccessible symbols are compile errors with source span.

---

## Package API Syntax in `PACKAGE.coppice`

Canonical form:

```lang
exports { SymbolA, SymbolB }
```

Semantics:

1. Listed symbols are resolved in the current package symbol table.
2. Listed symbols become part of the package external API.
3. Listing a non-`public` declaration is a compile error.
4. Duplicate exported members are compile errors.
5. Unknown symbols are compile errors.

Note: `exports` is only valid in `PACKAGE.coppice`. `PACKAGE.coppice` is a
declarative manifest and does not use imports; exported members resolve against
package-level `public` declarations. The keyword `export` is invalid.

Keyword intent: `exports` is plural by design to emphasize that
`PACKAGE.coppice` is a declarative package API table, not a file-local export
statement or a barrel forwarding file.

---

## Name Collision Rules

1. Duplicate imported member names in one file are compile errors unless
   aliased.
2. Ambiguous local names between imports and local declarations are compile
   errors.
3. `public` top-level declarations share one package import namespace across
   kinds (`type`, `function`, constant). Duplicate `public` names in one package
   are compile errors, including cross-file duplicates.
4. File-private top-level declarations may reuse names across files because they
   are not importable.
5. Multiple packages with same trailing segment are irrelevant; identity is full
   package path, not final segment.

---

## Dependency Graph and Cycles

1. Package import graph is directed.
2. Any cycle is a compile error.
3. Cycle ban is permanent language policy, not an implementation phase detail.

Diagnostics should report at least one concrete cycle path.

---

## Bazel Mapping

1. One package path maps to one `lang0_library` target.
2. Import path maps to package target dependency edge.
3. Import member list affects symbol resolution, not target identity.
4. No hidden deps: if a file imports package `P`, its package target must depend
   on `P`.

This yields deterministic and hermetic build graph derivation.

---

## Third-Party Imports

Third-party imports use the same syntax and resolver model:

```lang
import std/json { decode }
import external/registry/uuid { V7 }
```

Policy:

1. No URL/network imports.
2. Resolver maps external package paths to build-system-pinned dependencies.
3. Top-level path prefixes `std/` and `external/` are reserved.
4. First-party package paths must not start with reserved prefixes.
5. Language semantics do not distinguish first-party vs third-party import
   syntax.

This preserves one import model and hermetic builds.

---

## Alternatives Considered

### A) Package-wide implicit visibility (Go-like)

Rejected for Coppice goals:

1. Too implicit for file readability.
2. Hidden cross-file dependencies.
3. Weak alignment with "explicit over implicit."

### B) `public` alone controls external API

Rejected:

1. Easy to leak external API unintentionally.
2. No centralized package API manifest.

### C) Rich multi-form imports (`*`, namespace, default, inline FQNs)

Rejected:

1. Violates one-canonical-construct principle.
2. Increases style variance and ambiguity.

---

## Tradeoffs

### Benefits

1. Strong dependency clarity per file.
2. Stable, reviewable package APIs.
3. Clean Bazel/Gazelle derivation.
4. Deterministic and strict compiler behavior.

### Costs

1. More import boilerplate than implicit-package models.
2. Refactors may require import updates across many files.

Mitigation: compiler autofix for import sorting, unused import removal, and
missing import insertion where unambiguous.

---

## Non-Goals

1. Relative import convenience syntax.
2. Glob-import ergonomics.
3. File-as-build-unit dependency semantics.

---

## Implementation Notes (Non-Normative)

1. Keep parser file-local; add top-level import AST nodes.
2. Add package graph builder (discover `PACKAGE.coppice`, assign files to
   package).
3. Add package export table from `PACKAGE.coppice`.
4. Add resolver pass before typechecking.
5. Typecheck against resolved package symbol environment, not single file only.
6. Extend diagnostics fixture harness from single-file assumption to
   multi-file/package fixtures.
