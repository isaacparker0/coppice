# Autofix Execution Model (Draft)

## Purpose

Define how Coppice supports compiler-owned formatting and safe autofix behavior
without violating Bazel sandbox constraints.

This document is the source of truth for:

1. Autofix data model and ownership boundaries.
2. `check` / `build` / `run` / `fix` behavior.
3. Bazel action vs workspace-write semantics.

Companion specs:

- `specs/LANGUAGE_DESIGN.md`
- `specs/COMPILER_ARCHITECTURE.md`
- `specs/TOOLCHAIN_EXECUTION_MODEL.md`

Status: Draft.

---

## Problem

Coppice language direction states "the compiler is the formatter and linter",
but Bazel build actions cannot write source files in the workspace.

If canonical formatting is required for successful build, a naive `build --fix`
approach conflicts with sandboxing and makes normal build workflows painful.

We need one model that preserves:

1. Compiler-owned canonical source rules.
2. Deterministic build behavior in Bazel.
3. Explicit workspace writes only in write-capable execution modes.

---

## Design Goals

1. Keep rule ownership in frontend phases (no orchestration-owned language
   rules).
2. Keep formatting and autofix as compiler contracts, not external sidecars.
3. Separate "compute fixes" from "apply fixes to workspace files".
4. Allow build/run to consume canonicalized source in-memory.
5. Preserve hermetic/sandbox-safe behavior in Bazel action mode.

---

## Terminology

1. **Diagnostic**: phase-owned language issue with span and message.
2. **Autofix**: machine-applicable edit proposal attached to a diagnostic.
3. **Safe autofix**: one obvious semantics-preserving edit.
4. **Canonical source**: source after compiler-defined formatting/canonical
   transformations.
5. **Workspace write mode**: command context allowed to write source files (for
   example explicit user-invoked CLI run mode).

---

## Normative Model

### 1) Phase Ownership

1. Language diagnostics remain owned by phase crates per
   `specs/COMPILER_ARCHITECTURE.md`.
2. A phase that owns a diagnostic may optionally attach autofix edits.
3. Orchestration crates (`check_pipeline`, `driver`, `cli`) must not invent new
   language diagnostics and must not infer autofixes from diagnostic text.

### 2) Edit Model

Compiler edit model is source-snapshot-based:

1. Edit fields:
   - target file path
   - start byte offset (inclusive)
   - end byte offset (exclusive)
   - replacement text
   - fix identity metadata (phase/rule/safety)
2. Offsets for edits in one file must refer to the same input snapshot.
3. Overlapping edits are conflicts unless they are byte-identical replacements.
4. Conflict resolution is deterministic and implementation-defined by explicit
   priority policy.
5. Applying edits must be deterministic.

### 3) Conflict Resolution and Apply Order

Per file:

1. Partition edits into accepted vs rejected conflicts.
2. Sort accepted edits by source position.
3. Apply edits in descending start-offset order to avoid offset shifting.
4. Rejected edits remain reportable for user visibility.

### 4) Formatter Positioning

Formatting is modeled as compiler-owned autofix behavior.

Initial guidance:

1. Formatter-only autofix should be the first shipped autofix class.
2. Formatter output is canonical and deterministic.
3. Additional lint autofixes are admitted only when they are one obvious safe
   rewrite.

---

## Command Behavior Contract

## `check`

1. Computes diagnostics and autofix proposals.
2. Does not write workspace files by default.
3. May emit machine-readable fix artifacts (for example JSON/diff) for tooling.

## `fix` (new command)

1. Applies safe autofixes to workspace files.
2. Requires workspace-write-capable execution context.
3. Reports applied edits, rejected conflicts, and remaining diagnostics.

## `build` / `run`

1. Must not write workspace source files in Bazel action mode.
2. May compile from in-memory canonicalized source text derived from safe
   autofixes.
3. Strictness behavior is policy-driven:
   - permissive mode: build/run allowed, with diagnostics indicating
     non-canonical source
   - strict mode: fail when canonicalization differences exist or when unapplied
     style diagnostics are present

---

## Execution Environments

Autofix behavior must be consistent across standalone CLI and Bazel execution.

1. `fix` is a CLI command in both environments.
2. `fix` may write source files only in user-invoked, workspace-write contexts.
3. `build`/`run`/`test` must not rely on source-tree writes for correctness.
4. In sandboxed action contexts, compilation may use in-memory canonicalized
   source and may emit fix artifacts as action outputs.

---

## Current Policy

1. Developer iteration uses non-strict `build`/`run`.
2. CI enforces strict mode so pending safe autofixes fail.
3. Source rewrites happen via explicit `fix`, not during build/test actions.
4. Pre-commit/editor workflows should run `fix` before CI.

---

## Package and Responsibility Plan

### New compiler package: `compiler/fix_edits`

Owns:

1. Shared edit model types.
2. Merge/conflict detection.
3. Deterministic apply logic.

Non-scope:

1. Language-rule decisions.
2. Formatting policy.

### New compiler package: `compiler/source_formatting`

Owns:

1. Canonical Coppice formatting engine.
2. Formatting-specific edit generation.

Non-scope:

1. CLI/build orchestration.
2. Workspace write policy.

### Existing packages

1. `syntax_rules` and later `type_analysis` may emit diagnostics with autofixes.
2. `check_pipeline` aggregates diagnostics and autofixes across phases.
3. `driver` consumes canonicalized in-memory source for `build`/`run` when
   enabled.
4. `cli` owns command UX only.

---

## Data Contract Direction

Current diagnostics model:

- `compiler/diagnostics::PhaseDiagnostic` has message + span only.

Planned extension:

1. Add optional fix payload support to phase diagnostics or a parallel
   phase-owned fix output channel.
2. Preserve phase provenance for every emitted fix.
3. Keep hard failures in `compiler/reports::CompilerFailure` (not as synthetic
   diagnostics/fixes).

---

## Rollout Plan

### Phase 0: Infrastructure

1. Introduce shared edit model and deterministic merge/apply library.
2. Add `check` support for fix artifact emission (no workspace writes).

### Phase 1: Formatting Autofix

1. Implement canonical source formatter.
2. Emit formatter-backed safe autofixes.
3. Add `fix` command to apply formatter edits.

### Phase 2: Build/Run Canonicalization

1. Add in-memory canonicalization path for `build`/`run`.
2. Introduce strict/permissive policy controls.

### Phase 3: Additional Safe Autofixes

1. Add selected lint autofixes with explicit safety criteria.
2. Expand fixture coverage for conflict handling and determinism.

---

## Testing Requirements

1. Unit tests for edit merge/conflict/apply logic.
2. Fixture tests for formatter determinism.
3. CLI contract tests for:
   - `check` fix emission
   - `fix` workspace application
   - strict/permissive build behavior
4. Bazel action-mode tests ensuring no source writes occur.

---

## Open Decisions

1. Whether diagnostic+fix payload lives in one type vs parallel output channel.
2. Default strictness for local `build`/`run` vs CI.
3. Fix artifact format(s): JSON only vs JSON + unified diff.
4. Priority ordering between formatter edits and non-formatting safe autofixes.
