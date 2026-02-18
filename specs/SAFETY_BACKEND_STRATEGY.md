# Safety and Backend Strategy (Draft)

## Purpose

This document captures the current intended direction for Coppice safety
semantics and backend execution strategy, including alternatives considered and
why they were accepted or deferred.

It complements:

- `specs/LANGUAGE_DESIGN.md`
- `specs/COMPILER_ARCHITECTURE.md`
- `specs/LANGUAGE_NAME.md`

Status: Draft for design alignment before implementation of runnable backend
(`build`/`run`).

---

## Design Alignment

This strategy is constrained by Coppice goals:

1. Strong correctness and safety guarantees.
2. Ergonomics with minimal annotation overhead.
3. Explicit, canonical language behavior (one obvious way).
4. Deterministic, maintainable compiler evolution.

The language philosophy in `LANGUAGE_NAME.md` (managed growth, pruning,
durability) implies we should prefer a narrow, explicit core with clear upgrade
paths over convenience mechanisms that create semantic drift.

---

## Executive Summary

Planned direction:

1. Adopt a **mostly static safety model with narrowly scoped runtime checks** in
   v1.
2. Keep semantics compatible with future strengthening to near Rust-level static
   guarantees.
3. Define **executable program + runtime interface contracts before significant
   backend implementation**.
4. Implement first runnable backend as a **replaceable backend target** behind
   shared IR boundaries (not an ad hoc transpiler path).
5. Defer intersection types and advanced async/runtime details until core
   ownership/safety contracts are stable.

---

## Decision Matrix by Axis

### 1) Core Safety Contract

Options considered:

1. Rust-level static guarantees immediately.
2. Mostly static guarantees + targeted runtime checks.
3. Managed-runtime/GC-first safety model.

Direction:

- Choose **Option 2** for v1.

Rationale:

1. Best balance between safety goals and implementation tractability.
2. Supports rapid language iteration while preserving a strong safety posture.
3. Can evolve toward stronger static checking without changing user-facing
   meaning if semantics are disciplined now.

Guardrail:

- Runtime checks in v1 are implementation-scoped and explicitly documented, not
  semantic escape hatches.

### 2) Ownership and Mutation Semantics

Options considered:

1. Exposed borrow/lifetime annotation model (Rust-like syntax burden).
2. Hidden inference-heavy borrow model with broad implicit behavior.
3. Value-by-default semantics with explicit mutation and explicit
   shared-ownership escape hatch.

Direction:

- Choose **Option 3**.

Semantics:

1. Values are by default local-owned and non-aliased.
2. Mutation requires explicit `mut`.
3. Shared ownership uses explicit `shared` model (ARC semantics), with shared
   mutation only via explicit synchronization mechanisms.

Rationale:

1. Preserves explicitness and canonicality.
2. Aligns with ergonomic goals without exposing full borrow annotation surface.
3. Creates a practical bridge toward stronger static checking.

### 3) Inference Scope

Options considered:

1. Broad global inference across most constructs.
2. Local/bidirectional inference with explicit boundaries.
3. Mostly explicit type arguments and annotations.

Direction:

- Choose **Option 2**.

Semantics:

1. Keep explicitness at API boundaries where ambiguity harms diagnostics.
2. Improve ergonomics at use sites where inference is deterministic and
   unambiguous.
3. Prioritize generic call-site inference as a high-value improvement area.

Rationale:

1. Better deterministic diagnostics.
2. Avoids hidden, brittle global inference behavior.
3. Supports one-canonical-style rule set.

### 4) Async/Concurrency Semantics

Options considered:

1. Full async runtime semantics now.
2. Minimal async contract now, richer runtime semantics later.
3. Defer async entirely.

Direction:

- Choose **Option 2**.

Minimum semantic commitments before major backend work:

1. Suspension points are explicit (`await`).
2. Exclusive mutable access is not permitted to remain live across `await`
   boundaries.
3. `spawn` requires compiler-proven safe capture transfer/share policy.
4. Cancellation/error behavior is defined at a minimum contract level.

Rationale:

1. Async interacts directly with ownership and soundness.
2. Deferring all async semantics creates backend rework risk.

### 5) Runtime Boundary (ABI Contract)

Options considered:

1. Ad hoc runtime coupling during backend implementation.
2. Fixed runtime boundary contract early.

Direction:

- Choose **Option 2**.

Scope:

1. Required runtime operations and signatures.
2. Core value representation contracts needed by executable program and
   emission.
3. Error/abort and basic host interaction boundaries.
4. Async runtime entry points (minimal subset) when enabled.

Rationale:

1. Prevents backend/language semantic drift.
2. Enables multiple backend targets against the same contract.

### 6) Backend Implementation Strategy

Options considered:

1. Direct Cranelift implementation immediately.
2. Ad hoc transpiler path to validate quickly.
3. Shared executable program with replaceable backend targets.

Direction:

- Choose **Option 3**.

Clarification:

- A first backend target may emit Rust for speed of iteration, but only through
  shared executable program and runtime contracts. This is a backend
  implementation choice, not a semantic fork and not a throwaway transpiler
  architecture.

Rationale:

1. Maintains long-term architecture integrity.
2. Enables faster runnable-language feedback loops.
3. Keeps path open to direct Cranelift backend without semantic rewrite.

### 7) Interfaces and Intersection Types

Options considered for interfaces:

1. Implicit structural conformance.
2. Explicit nominal conformance.

Direction:

- Prefer **explicit conformance model** when interfaces are introduced.
- Use inline `implements` on type declarations as the canonical conformance
  form.
- Do not introduce standalone conformance statements.

Rationale:

1. Aligns with explicit-over-implicit philosophy.
2. Reduces accidental conformance and improves diagnostics.
3. Preserves one canonical construct per intent.
4. Avoids additional coherence/orphan-style policy complexity.

Options considered for intersections:

1. General intersection types now.
2. Defer general intersections; use named abstractions and constraints.

Direction:

- Choose **Option 2** for now.

Rationale:

1. High type-system complexity cost.
2. Lower immediate unlock value for runnable backend milestone.
3. Re-evaluate based on real code pressure after runnable iteration.

### 8) Function-Type Scope For Backend Readiness

Direction:

- Support first-class function types with monomorphic signatures in v1 and defer
  polymorphic function values.

Scope note:

- Detailed language semantics, examples, and future-admission criteria are
  tracked in `specs/LANGUAGE_DESIGN.md`.

---

## V1 Semantic Contract (Proposed)

In safe Coppice code, v1 aims to guarantee:

1. No use-after-free.
2. No data races in supported concurrency model.
3. No uninitialized reads.
4. No implicit shared mutable aliasing.

Enforcement model in v1:

1. Prefer compile-time proof.
2. Allow targeted runtime checks for explicitly documented categories.

Runtime checks allowed in v1 (examples):

1. Bounds checks.
2. Selected numeric checks per operator policy.
3. Other explicitly documented deferred checks.

Non-goals for v1:

1. Full parity with Rust borrow-checker strength on all programs.
2. Fully specified advanced async runtime semantics.
3. General intersection types.
4. Polymorphic function values.

---

## Compatibility Rule for Future Strengthening

To preserve upgrade path from v1 safety to stronger static safety:

1. Do not introduce semantics that require permanent runtime fallback for core
   ownership/aliasing soundness.
2. Treat runtime checks as deferred static checks where feasible.
3. Keep enforcement tightening backward-compatible where possible (warnings to
   errors, optional strict modes to defaults).
4. Keep executable program semantics stable while strengthening proof
   obligations.

---

## Implementation Readiness Checklist

Required before broad backend implementation:

1. Safety contract section accepted.
2. Ownership/mutation/shared semantic rules accepted.
3. Minimal async ownership rules accepted.
4. Runtime interface contract finalized.
5. Executable program invariants documented.
6. Runnable MVP feature cut documented.

---

## Open Questions

1. Exact ARC/shared surface syntax and synchronization primitives.
2. Numeric overflow policy (trap/wrap/checked by operator family).
3. Whether first backend target should be Rust emitter or direct Cranelift,
   given team velocity and debugging preferences.
