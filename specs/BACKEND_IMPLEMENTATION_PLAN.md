# Backend Implementation Plan (Draft)

## Purpose

This document defines the implementation architecture and rollout plan for
adding runnable compilation (`build`/`run`) to Coppice.

It is a planning/spec companion to:

- `specs/SAFETY_BACKEND_STRATEGY.md`
- `specs/COMPILER_ARCHITECTURE.md`
- `specs/LANGUAGE_DESIGN.md`

Status: Draft.

---

## Goals

1. Add runnable backend capability without compromising current frontend
   architecture.
2. Preserve a clean path to long-term direct backend targets.
3. Keep `check` fast and frontend-only.
4. Enforce explicit ownership boundaries between compiler and runtime concerns.

---

## Repository Organization Options

### Option A: Keep compiler crates in `compiler/`, split runtime into `runtime/` (Chosen)

Layout:

- `compiler/*` for parsing, rules, resolution, semantic, type, IR lowering,
  backend drivers/emitters.
- `runtime/*` for runtime libraries linked by compiled programs.

Pros:

1. Strong separation of concerns.
2. Scales with multiple backend targets.
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

4. `compiler/rust_backend` (optional first backend target)

- Emits Rust from `executable_program`.
- Exists to accelerate runnable iteration while preserving backend replacement.

5. `compiler/cranelift_backend` (future target)

- Emits machine code/object output from the same `executable_program` and
  `runtime_interface` contracts.

6. `compiler/build_driver` (or extension of `compiler/driver`)

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

### Phase 3: Backend Diversification

1. Introduce/expand direct backend target(s) against same IR contract.
2. Keep behavior parity tests between backends.

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
2. Interfaces/intersections
3. Advanced generics corners
4. Full shared mutable synchronization surface

---

## Open Decisions

1. First backend target choice for phase 1 execution (`rust_backend` vs direct
   Cranelift in phase 1).
2. Artifact format and workspace output location policy.
3. Debug info policy for generated artifacts in early phases.
4. Exact strictness for unsupported-in-build diagnostics vs fallback behavior.
