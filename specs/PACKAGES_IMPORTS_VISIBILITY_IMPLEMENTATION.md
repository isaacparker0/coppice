# Packages, Imports, and Visibility: Implementation Specification (Draft)

## Status

Draft. Companion to `specs/PACKAGES_IMPORTS_VISIBILITY.md`.

This document defines an implementation plan for introducing multi-file
compilation, package manifests, explicit imports, visibility enforcement, and
package-level dependency checking in Coppice.

---

## Scope

In scope:

1. Multi-file package compilation.
2. `PACKAGE.coppice` parsing and validation.
3. New top-level `import` syntax in source files.
4. File/package/external visibility enforcement.
5. Package graph construction and cycle diagnostics.
6. Resolver and typechecker integration.
7. Diagnostics fixture infrastructure changes for multi-file cases.
8. File-role enforcement for binary entrypoints and tests.

Out of scope:

1. Code generation backend changes.
2. LSP/editor features.
3. Performance optimization beyond baseline correctness.

---

## Current-State Constraints (As Implemented Today)

Current compiler constraints that drive the migration design:

1. Parsing is file-local:
   `parse_file(source: &str) -> Result<File, Vec<Diagnostic>>`.
2. AST has file-local import/exports declaration nodes, but no package-level
   semantic model.
3. Typechecker entrypoint is single-file: `check_file(file: &File)`.
4. Symbol tables are flat maps keyed by name within one file context.
5. CLI `check` takes a single file path.
6. Diagnostics fixtures hardcode `input/main.coppice` per case.

Implication: this is a structural refactor, not an additive feature toggle.

---

## Implementation Strategy

Use staged architecture changes with strict phase gates:

1. Build package/model/resolver scaffolding first.
2. Integrate import and visibility semantics second.
3. Migrate typechecking from file-level to package-level third.
4. Expand diagnostics fixtures and lock behavior with snapshots.

Each phase must keep compiler behavior deterministic and testable.

---

## Target Compiler Architecture

Planned compile pipeline for `check <path>`:

1. **Workspace Discovery**
   - Resolve workspace root:
     - default current working directory
     - optional `--workspace-root <path>` override
   - Validate workspace root (`PACKAGE.coppice` must exist at root).
   - Determine package/file set under workspace root.
2. **Lex/Parse**
   - Parse all `.coppice` files in each package.
   - Parse `PACKAGE.coppice` manifest syntax.
3. **Package Graph Build**
   - Build package nodes and import dependency edges.
4. **Cycle Detection**
   - Reject graph cycles with concrete path diagnostic.
5. **Declaration Collection**
   - Build per-package symbol tables from all source files.
6. **Export Table Build**
   - Build external API table from `PACKAGE.coppice` `exports` declarations.
7. **Import Resolution + Visibility Validation**
   - Resolve each file import against package tables.
8. **Typecheck**
   - Typecheck per package using resolved environment.
9. **Emit Diagnostics**
   - Stable ordering by file path, span, message.

---

## Data Model Additions

## Frontend AST Additions

1. Add `imports: Vec<ImportDeclaration>` to `ast::File`.
2. Add import nodes:
   - `ImportDeclaration { package_path, members, span }`
   - `ImportMember { name, alias: Option<String>, span }`
3. Keep existing declaration nodes and spans unchanged.

## Package Manifest AST

Introduce a dedicated manifest AST:

1. `PackageManifest { exports: Vec<ExportsDeclaration> }`
2. `ExportsDeclaration { members, span }`

Manifest parser should be separate from normal source parser to enforce strict
grammar (comments + `exports` only).

## Middle-Layer IR Structures

Add explicit semantic structures to avoid ad-hoc maps:

1. `PackageId` (interned stable ID).
2. `PackageNode { id, path, root_dir, source_files, manifest_file }`.
3. `PackageGraph { nodes, deps }`.
4. `SymbolId` and `SymbolKind` (`Type`, `Function`, `Constant`).
5. `DeclaredSymbol { id, name, kind, visibility, file_id, span }`.
6. `PackageSymbolTable { by_name, declarations }`.
7. `ExportTable { exported_symbols }`.
8. `ResolvedImport { source_file, package_id, imported_symbol_ids, aliases }`.
9. `FileRole` enum: `Library | BinaryEntrypoint | Test`.
10. `SourceFile { id, path, role, contents, ast }` (role derived from suffix).

Avoid positional tuples for multi-part keys; use named structs.

---

## Work Breakdown (Chunked Plan)

## Phase 0: Foundation and Guardrails

Goals:

1. Introduce compile session and file identity primitives.
2. Preserve current behavior while adding new plumbing.

Tasks:

1. Add `FileId`, `SourceFile`, and deterministic file ordering helpers.
2. Add a central `DiagnosticSink` with stable sort utility.
3. Add feature-gated package-check entrypoint (internal only).

Exit criteria:

1. Existing diagnostics tests still pass unchanged.
2. New internal scaffolding has unit tests for stable ordering.

---

## Phase 1: Package Discovery and Graph Build

Goals:

1. Discover packages using `PACKAGE.coppice`.
2. Associate each source file with exactly one package.
3. Classify file role by suffix.

Tasks:

1. Directory walker:
   - locate all `PACKAGE.coppice`
   - assign descendant files to nearest ancestor package root unless shadowed by
     nested package.
   - assign `FileRole` based on filename suffix.
2. Validate package invariants:
   - no orphan source files outside any package (policy decision: error).
   - one manifest per package root directory.
   - command invocation must resolve a workspace root deterministically.
3. Build `PackageNode` set with deterministic path-based IDs.

Exit criteria:

1. Unit tests for nested package ownership.
2. Deterministic package set on repeated runs.
3. Unit tests for file-role classification and suffix parsing.

---

## Phase 2: Parser Extensions (`import` and Manifest)

Goals:

1. Parse canonical file imports.
2. Parse strict `PACKAGE.coppice` `exports` declarations.
3. Add file-role semantic validation on declarations.

Tasks:

1. Lexer:
   - add `import`, `exports`, and `as` keywords.
2. Source parser:
   - parse top-level `import package/path { ... }` where `package/path` is
     import-origin-prefixed (`workspace`, `std`, `external`).
   - enforce import-before-declarations policy (recommended for clarity).
   - enforce explicit type annotations on all constant declarations.
3. Manifest parser:
   - parse `exports { ... }`.
   - reject non-comment non-`exports` tokens.
4. File-role validation (post-parse, pre-resolver):
   - `*.bin.coppice` must declare exactly one `main`.
   - `main` must have no params and no return value.
   - `main` must be file-private (not `public`).
   - `*.bin.coppice` must not contain any `public` declarations.
   - `*.coppice` (library) must not declare `main`.
   - `*.test.coppice` must not declare `main`.
   - `*.test.coppice` must not contain any `public` declarations.

Exit criteria:

1. New parser fixture suite for valid/invalid import syntax.
2. New parser fixture suite for manifest grammar violations.
3. New diagnostics fixtures for file-role violations (missing/invalid `main`,
   `public` in bin/test, `main` in library/test).
4. New diagnostics fixtures for missing constant type annotations.

---

## Phase 3: Declaration Collection and Export Table

Goals:

1. Collect all declared symbols across files per package.
2. Build external export surface from manifests.

Tasks:

1. Per-package declaration collector:
   - detect duplicate symbol names across package files.
   - enforce one shared import namespace for package-visible (`public`)
     top-level symbols across kinds (`type`, `function`, constant).
   - enforce top-level file-private/package-visible modifiers.
   - enforce member type-private/public modifiers.
2. Manifest export resolver:
   - resolve each exported member to package symbol.
   - ensure exported symbol is `public`.
   - reject duplicates and unknown symbols.

Exit criteria:

1. Diagnostics fixtures for duplicate package-level names across files.
2. Fixtures for invalid `exports` declarations.

---

## Phase 4: Package Dependency Resolution and Cycles

Goals:

1. Resolve import paths to package IDs.
2. Build and validate dependency graph.

Tasks:

1. For each source file import, resolve package path.
2. Emit unknown package diagnostics with source span.
3. Emit unknown import origin diagnostics (`workspace`, `std`, `external` are
   the only valid top-level import origins).
4. Build directed package graph from resolved imports.
5. Detect and report cycles (permanent error policy).

Cycle diagnostic requirements:

1. Report at least one concrete cycle path.
2. Anchor diagnostic to one import in the cycle.

Exit criteria:

1. Fixtures for unknown package imports.
2. Fixtures for unknown import origin.
3. Fixtures for simple and multi-node import cycles.

---

## Phase 5: Import Binding and Visibility Enforcement

Goals:

1. Resolve imported members.
2. Enforce same-package and external visibility rules.

Tasks:

1. Same-package imports:
   - only `public` symbols importable from other files.
2. External imports:
   - symbol must be `public`.
   - symbol must be listed in `PACKAGE.coppice` `exports`.
3. Validate alias collisions and imported-name collisions.
4. Build per-file import environment (name -> symbol binding).

Exit criteria:

1. Fixtures for file-private access denial.
2. Fixtures for missing package API member denial.
3. Fixtures for import alias/name conflicts.

---

## Phase 6: Typechecker Refactor to Package Context

Goals:

1. Replace single-file name resolution with package-aware resolution.
2. Preserve existing type rules and diagnostics behavior where unchanged.

Tasks:

1. Introduce `check_package(package_ctx)` entrypoint.
2. Split symbol resolution from type checking:
   - expression/type checking must query resolved environment, not local maps.
3. Preserve current checks:
   - type compatibility
   - control flow
   - unreachable code
   - naming/casing
   - method calls, fields, match exhaustiveness
4. Add file-aware lookup:
   - local scope variables
   - imported package symbols
   - same-file declarations where applicable

Compatibility rule:

1. Existing single-file diagnostics should remain identical unless new import/
   visibility rules intentionally change behavior.

Exit criteria:

1. Existing diagnostics fixtures still pass after migration.
2. New multi-file fixtures pass for imports/visibility cases.

---

## Phase 7: CLI and Diagnostics Fixture Harness Migration

Goals:

1. Make `check` package-aware.
2. Support multi-file fixture inputs.

Tasks:

1. CLI:
   - `check` is canonical and equivalent to `check .`.
   - `check <path>` accepts file or package directory, relative to workspace
     root.
   - if file path is provided (including `.bin.coppice` and `.test.coppice`),
     resolve owning package and check package.
   - if no owning package exists (no ancestor `PACKAGE.coppice`), emit a
     package-ownership compile error.
   - default workspace root is current working directory.
   - optional `--workspace-root <path>` overrides workspace root.
   - workspace root must contain `PACKAGE.coppice` at root; otherwise emit a
     clear workspace-root error before package discovery.
   - invocation outside workspace root is an error unless path is rebased via
     explicit workspace root.
2. Fixture runner:
   - remove hardcoded `input/main.coppice`.
   - run compiler from `input/` as cwd and invoke `check`.
   - allow multi-file fixture trees.
   - require `input/PACKAGE.coppice` as explicit fixture workspace root marker.
3. Update `tests/diagnostics/README.md` rules to include multi-file fixture
   structure.

Exit criteria:

1. Mixed fixture suites (single-file + multi-file) pass.
2. Snapshot update flow remains unchanged.

---

## Diagnostics Design and Ordering

Requirements:

1. Deterministic order across machines/runs.
2. Primary span points at source of error, not derived site where possible.
3. Visibility errors should mention:
   - symbol name
   - defining package/file context
   - required visibility level
4. Cycle errors should include cycle chain.
5. File-role errors should mention:
   - file path and role
   - required rule (e.g., `main` signature, no `public` in bin/test)

Stable order strategy:

1. Sort by file path.
2. Then span `(line, column)`.
3. Then message text.

---

## Testing Strategy

## Unit Tests

1. Package discovery and file ownership.
2. Manifest parsing strictness.
3. Import resolution and alias conflict handling.
4. Cycle detection algorithm.

## Diagnostics Fixtures

Add new fixture areas:

1. `tests/diagnostics/imports/...`
2. `tests/diagnostics/packages/...`
3. `tests/diagnostics/visibility/...` (cross-file, cross-package)

Representative fixture cases:

1. `minimal_valid` multi-file package.
2. unknown package.
3. unknown imported member.
4. file-private symbol imported from sibling file.
5. external import of non-exported symbol.
6. external import of non-public symbol.
7. cycle of length 2 and 3.
8. duplicate imports without alias.
9. invalid `PACKAGE.coppice` code content.
10. `.bin.coppice` missing `main`.
11. `.bin.coppice` `main` signature mismatch.
12. `main` declared in library file.
13. `public` declaration in `.bin.coppice`.
14. `public` declaration in `.test.coppice`.

## Regression Tests

1. Ensure all previous fixture categories continue to pass.
2. Add targeted tests for deterministic diagnostic ordering.

---

## Incremental Rollout and Risk Controls

### Risk 1: Breaking existing typecheck behavior

Mitigation:

1. Preserve old check path behind adapter until phase 6 complete.
2. Keep old fixtures green at each phase.

### Risk 2: Resolver complexity causes fragile diagnostics

Mitigation:

1. Separate resolver diagnostics from typechecker diagnostics.
2. Add unit tests for each resolver failure mode.

### Risk 3: Non-deterministic graph/build behavior

Mitigation:

1. Canonicalize paths early.
2. Use sorted collections for iteration where output order matters.

### Risk 4: Fixture migration churn

Mitigation:

1. Keep backward compatibility for single-file fixture layout initially.
2. Migrate to multi-file incrementally by area.

---

## Proposed Milestones (Execution Order)

1. M1: Phase 0-1 complete (discovery and package graph scaffolding).
2. M2: Phase 2 complete (parser + manifest parser).
3. M3: Phase 3-5 complete (resolver, visibility, cycle errors).
4. M4: Phase 6 complete (package-aware typechecker).
5. M5: Phase 7 complete (CLI + fixtures migration).
6. M6: hardening pass (ordering, docs, cleanup).

Recommended merge policy:

1. Merge per milestone.
2. No milestone merges with red diagnostics suite.

---

## Immediate Next Step

Start with M1 and M2 in one branch:

1. Land package discovery + IDs + deterministic ordering.
2. Land parser support for source imports and strict `PACKAGE.coppice`.
3. Add parser/discovery fixtures before touching typechecker logic.

This minimizes blast radius while proving the structural model early.
