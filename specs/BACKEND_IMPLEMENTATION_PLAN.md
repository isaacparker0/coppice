# Backend Implementation Plan (Draft)

## Purpose

This document defines the implementation architecture and rollout plan for
adding runnable compilation (`build`/`run`) to Coppice.

It is a planning/spec companion to:

- `specs/SAFETY_BACKEND_STRATEGY.md`
- `specs/COMPILER_ARCHITECTURE.md`
- `specs/LANGUAGE_DESIGN.md`
- `specs/TOOLCHAIN_EXECUTION_MODEL.md`

Status: Draft.

---

## Goals

1. Add runnable backend capability without compromising current frontend
   architecture.
2. Preserve a clean path to one long-term production backend, with any
   intermediate backend overlap treated as temporary migration scaffolding.
3. Keep `check` fast and frontend-only.
4. Enforce explicit ownership boundaries between compiler and runtime concerns.

Toolchain policy for build/run execution is defined in
`specs/TOOLCHAIN_EXECUTION_MODEL.md`.

---

## Backend Transition Policy

1. Long-term steady state is one production backend path.
2. The currently planned successor backend for transition is direct AOT
   Cranelift.
3. This planned successor does not permanently lock the ultimate long-term
   backend choice.
4. Any period with more than one backend implementation is temporary migration
   state only.
5. Temporary overlap is allowed only to derisk migration and must be removed
   once the planned AOT Cranelift path reaches required coverage for the
   supported language slice.
6. Multi-backend parity testing, when present, is migration validation, not a
   permanent product contract.

---

## Repository Organization Options

### Option A: Keep compiler crates in `compiler/`, split runtime into `runtime/` (Chosen)

Layout:

- `compiler/*` for parsing, rules, resolution, semantic, type, IR lowering,
  backend drivers/emitters.
- `runtime/*` for runtime libraries linked by compiled programs.

Pros:

1. Strong separation of concerns.
2. Supports short-lived temporary overlap during backend migration.
3. Avoids compiler-runtime dependency bleed.
4. Aligns with current crate-per-phase structure.

Cons:

1. Adds a new top-level directory.

### Option B: Keep runtime under `compiler/`

Pros:

1. Fewer top-level directories.
2. Slightly simpler initial navigation.

Cons:

1. Weak compiler/runtime conceptual boundary.
2. Higher risk of accidental dependency coupling.
3. Harder long-term maintainability.

### Option C: Reorganize into top-level `frontend/`, `backend/`, `runtime/`

Pros:

1. Very explicit architectural grouping.

Cons:

1. High churn relative to current repository layout.
2. Disruptive with limited immediate payoff.

Decision:

- Choose Option A.

---

## Proposed Package Architecture

## Compiler crates (`compiler/`)

1. `compiler/executable_program`

- Defines executable program structures and invariants.
- No emission logic.

2. `compiler/executable_lowering`

- Lowers semantic + typechecked program artifacts into `executable_program`.
- Owns lowering diagnostics for unsupported codegen constructs.

3. `compiler/runtime_interface`

- Canonical runtime interface contract (symbols, signatures, representation
  rules required by backends).
- Scope: language runtime operation contracts only (for example `print`,
  `abort`, and other runtime-provided operations).
- Non-scope (for now): general user-callable symbol mangling and host-entry shim
  naming policy; these remain backend implementation details unless later
  promoted to a shared linkage contract.

4. `compiler/cranelift_backend`

- Emits machine code/object output from the same `executable_program` and
  `runtime_interface` contracts.

5. `compiler/build_driver` (or extension of `compiler/driver`)

- Orchestrates backend phases for `build`/`run`.

## Runtime crates (`runtime/`)

1. `runtime/core`

- Required language runtime intrinsics (string/runtime helpers, abort/print,
  foundational operations).

2. `runtime/sync` (phased)

- Synchronization primitives for shared mutable state semantics.

3. `runtime/async` (phased)

- Executor/task support for async semantics once enabled.

---

## Dependency Direction Rules

Normative direction:

1. Frontend crates (`parsing` through `type_analysis`) must not depend on
   backend crates or runtime implementation crates.
2. `executable_lowering` may depend on semantic/type artifacts but not backend
   emitter implementations.
3. Backend emitters may depend on `executable_program` and `runtime_interface`.
4. Compiler crates must not depend on `runtime/*` implementation crates.
5. Runtime crates must not depend on compiler crates.

Intent:

- Keep language semantics and backend implementation concerns decoupled.

---

## Pipeline Extension

Current `check` pipeline remains unchanged.

`build`/`run` pipeline adds:

1. executable lowering: semantic+typed artifacts -> executable program
2. backend emission/compilation target
3. binary materialization/link
4. process execution (`run` only)

Per-file/package gating remains status-driven as in
`specs/COMPILER_ARCHITECTURE.md`.

---

## CLI Plan

Extend CLI commands:

1. `coppice check` (existing; unchanged behavior)
2. `coppice build` (new)
3. `coppice run` (new)

Optional developer flags (non-user-contract initially):

1. `--emit-ir`
2. `--emit-backend-source` (if Rust backend target is present)

---

## Testing Plan

1. Keep diagnostics fixture suite for frontend correctness.
2. Add executable program fixture tests:

- input Coppice -> expected executable program snapshot

3. Add backend emitter fixture tests:

- executable program -> expected emitted artifact (text/object metadata as
  applicable)

4. Add end-to-end runnable fixtures:

- source -> build -> run -> assert stdout/stderr/exit code

---

## Phased Rollout

### Phase 0: Contracts

1. Finalize `runtime_interface` v0.
2. Finalize `executable_program` v0 invariants.
3. Finalize executable lowering acceptance criteria.

### Phase 1: Runnable MVP

1. Implement executable lowering for core subset.
2. Implement first backend target behind `executable_program`.
3. Add `build`/`run` for MVP subset.

### Phase 2: Coverage Expansion

1. Expand construct support (control flow, types, package edges).
2. Improve diagnostics for unsupported codegen paths.

### Phase 3: Backend Replacement (Temporary Transition Only)

1. Introduce direct AOT Cranelift backend against the same IR contract.
2. If temporary overlap is required, keep migration parity tests between old and
   new backend paths only for the overlap window.
3. Remove transitional backend path once AOT Cranelift coverage and behavior
   requirements are met.

---

## MVP Feature Cut (Initial)

Supported for first runnable milestone:

1. Scalars: int64/boolean/string/nil
2. Functions, locals, returns
3. Calls
4. If/for
5. Struct literals and field access

Deferred with explicit build diagnostics:

1. Advanced async semantics
2. Interface types (explicit nominal conformance via inline `implements`)
3. General intersection types
4. Polymorphic function values
5. Advanced generics corners
6. Full shared mutable synchronization surface

Detailed semantics and feature-admission criteria for these deferred language
features are defined in `specs/LANGUAGE_DESIGN.md`.

---

## Current Minimal Slice Status (Implemented)

This section records the current repository state for the minimal end-to-end
slice. It is intentionally narrow and is expected to expand incrementally.

### What is implemented now

1. New backend boundary crates exist:

- `compiler/executable_program`
- `compiler/executable_lowering`
- `compiler/type_annotated_program`
- `compiler/runtime_interface`
- `compiler/cranelift_backend`

2. CLI supports runnable flow:

- `coppice build <path-to-bin.copp> [--output-dir ...]`
- `coppice run <path-to-bin.copp> [--output-dir ...]`
- `build`/`run` require an explicit `.bin.copp` file path.

3. Toolchain execution intent is fully hermetic across all supported execution
   modes:

- compiler resolves tool binaries from Bazel-provided runfiles/runtime data.
- backend code generation is in-process (no host `rustc` shell-out).
- final executable linking currently uses a temporary host-linker bridge
  (`xcrun clang++` on macOS, `clang++` on Linux) isolated in
  `compiler/cranelift_backend/linker_bridge.rs`.
- this temporary bridge is implementation debt, not accepted steady-state
  behavior.

4. Runnable subset coverage extends beyond the original minimal slice:

- `main() -> nil`
- local bindings/assignment
- function calls and returns
- `if`/`for`/`break`/`continue`
- operators
- struct literals and field access
- `print(...)` and `abort(...)`

### Where this is aligned

1. Matches planned backend package boundaries and keeps `check` frontend-first.
2. Aligns with `specs/TOOLCHAIN_EXECUTION_MODEL.md` package boundaries and
   execution model direction, with one explicitly scoped temporary linker
   exception.
3. Driver orchestration is single-pass per command and reused by `check` and
   `build`/`run`, avoiding split correctness contracts.
4. Executable lowering consumes `type_annotated_program` (typed artifact
   boundary), not semantic-only artifacts.
5. Backend artifact identity is explicit (no fixed output names), providing a
   scalable path for multiple binaries and Bazel rule/action integration.
6. Provides a real executable path for language iteration without bypassing
   long-term architecture.

### Critical gaps to address next

1. Runtime interface v0 contract details are still being finalized.
2. Migration validation scope and temporary overlap parity tests are still being
   finalized.
3. Executable lowering and type-annotated artifact coverage is intentionally
   narrow and should expand with language feature support.
4. Replace the temporary host-linker bridge with a fully hermetic linker
   integration for monorepo CLI `build`/`run`.

---

## Open Decisions

1. Artifact format and workspace output location policy.
2. Debug info policy for generated artifacts in early phases.
3. Exact strictness for unsupported-in-build diagnostics vs fallback behavior.
4. Interface rollout sequencing after current runnable subset expansion
   (`implements` in frontend first vs backend coverage in same phase).
