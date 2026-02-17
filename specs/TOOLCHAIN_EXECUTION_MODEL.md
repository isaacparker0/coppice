# Toolchain Execution Model (Draft)

## Purpose

This document defines how Coppice compilation and execution use toolchains in
Bazel monorepo workflows and in future standalone CLI workflows.

This is the single source of truth for toolchain execution policy.

Companion docs:

- `specs/BACKEND_IMPLEMENTATION_PLAN.md`
- `specs/COMPILER_ARCHITECTURE.md`
- `specs/SAFETY_BACKEND_STRATEGY.md`

Status: Draft.

---

## Scope

This document governs:

1. Toolchain invocation and ownership boundaries.
2. Hermeticity requirements for build/run implementation.
3. Execution-mode parity expectations across all supported execution contexts.

This document does not define language semantics.

---

## Goals

1. Coppice tooling MUST work consistently across three execution contexts:
   monorepo CLI development, standalone prebuilt CLI use, and Bazel rule/action
   use in external repositories.
2. The command contract (`check`, `build`, `run` behavior and diagnostics) MUST
   be equivalent across those contexts, except where Bazel action mechanics
   require explicit output declaration.
3. Compiler orchestration MUST remain hermetic: no host
   compiler/linker/toolchain dependency.

---

## Normative Rules

1. Monorepo `build` and `run` MUST use Bazel-managed toolchains.
2. Compiler orchestration code MUST NOT invoke host system compilers/linkers
   directly (for example `rustc`, `clang`, `ld`) via shell/process execution.
3. In Bazel rule/action mode, generated artifacts MUST be produced as
   Bazel-declared outputs (for example under `bazel-out`), not as implicit
   workspace side effects.
4. Backend implementations MUST be reachable via compiler package boundaries
   (`executable_lowering`, `executable_program`, backend target) and MUST NOT
   bypass these boundaries with ad hoc compilation shortcuts.
5. Toolchain resolution MUST be implementation-defined per mode, but all modes
   MUST resolve to managed hermetic toolchains rather than host system tools.

---

## Current Baseline in This Repository

From `MODULE.bazel`:

1. `toolchains_llvm_bootstrapped` toolchains are registered.
2. `rules_rust` toolchains are registered.

Implication:

- In Bazel mode, Rust targets can compile/link using hermetic Bazel-managed
  toolchain resolution.

Note:

- `toolchains_llvm_bootstrapped` is primarily a hermetic LLVM/C/C++ toolchain
  ecosystem, with explicit guidance for Rust integration in Bazel workflows.

---

## Execution Modes

## A) Monorepo CLI development mode (`bazel run //compiler/cli:main -- ...`)

1. Entrypoint is the user-facing Coppice CLI.
2. Toolchain binaries are resolved from Bazel-provided runtime data/runfiles.
3. Workspace output directories are allowed (for example default `.coppice/`, or
   explicit `--output-dir`).
4. Hermeticity is mandatory.

## B) Standalone prebuilt CLI mode (planned)

1. Must preserve language and backend semantics parity with Bazel mode.
2. Must preserve command-level UX parity with monorepo CLI mode (`check`,
   `build`, `run` behavior and user-visible contracts).
3. Must preserve deterministic output guarantees for equivalent inputs.
4. Toolchain provisioning strategy is required (bundled or otherwise managed).
5. Any non-hermetic fallback behavior, if introduced temporarily, must be
   explicitly documented and gated.
6. Default output location may be workspace-local (for example `.coppice/`),
   with explicit output path overrides supported.

## C) Bazel rule/action mode (`rules_coppice`, planned)

1. Entrypoint is a Bazel rule, not an interactive CLI invocation.
2. All generated artifacts must map to Bazel-declared outputs.
3. Output paths are supplied by Bazel action wiring, not implicit workspace
   defaults.
4. Toolchains are resolved through Bazel toolchain registration and action
   inputs.
5. Hermeticity and reproducibility are mandatory.

---

## Minimal Slice Policy

For minimal end-to-end backend slices:

1. It is acceptable to support only a narrow language subset.
2. It is not acceptable to bypass toolchain policy (for example host `rustc`
   shelling) to achieve that subset.
3. Prototype shortcuts must remain architecture-aligned with planned backend
   boundaries.

---

## Open Decisions

1. Standalone CLI toolchain packaging strategy and release model.
2. How standalone mode enforces the same determinism guarantees as Bazel mode
   across supported platforms.
