# LSP And Check Architecture

## Purpose

Define a clear, shared architecture for `coppice check` and `coppice lsp` that:

1. Preserves one language analysis implementation path.
2. Supports low-latency incremental editor feedback.
3. Keeps Bazel action semantics hermetic and deterministic.

---

## Decisions

1. Introduce `compiler/check_pipeline` as the shared stateless analysis engine
   for the check phases.
2. Introduce `compiler/check_session` as a stateful incremental wrapper over
   `check_pipeline` for long-lived interactive clients.
3. Keep default one-shot CLI check path stateless:
   `compiler/cli -> compiler/check_orchestration -> compiler/check_pipeline`.
4. LSP uses sessionized incremental path:
   `compiler/lsp -> compiler/check_session -> compiler/check_pipeline`.
5. Build/run orchestration remains separate from check orchestration:
   `compiler/build_orchestration` owns lowering/backend coordination.
6. De-emphasize `driver` as a catch-all name; prefer responsibility-specific
   orchestration names.

---

## Dependency Graph

1. `compiler/check_pipeline` -> phase crates (`parsing`, `syntax_rules`,
   `file_role_rules`, `resolution`, `semantic_lowering`, `type_analysis`) and
   shared models.
2. `compiler/check_session` -> `compiler/check_pipeline`.
3. `compiler/check_orchestration` -> `compiler/check_pipeline`.
4. `compiler/build_orchestration` -> check outputs + executable lowering/backend
   crates.
5. `compiler/cli` -> orchestration crates.
6. `compiler/lsp` -> `compiler/check_session`.

---

## Tradeoffs

1. Split pipeline/session/orchestration adds crate boundaries and naming
   surface, but makes ownership explicit and avoids tool-specific forks.
2. Default CLI check does not reuse long-lived in-memory cache, but remains
   simple, deterministic, and aligned with one-shot process model.
3. LSP gains incremental performance and unsaved-buffer support, but requires
   explicit invalidation and overlay management in `check_session`.

---

## Alternatives Considered

1. **Single `driver` for all check and LSP behavior** Rejected: over-broad
   responsibility and poor fit for long-lived incremental state.
2. **CLI and LSP both depend directly on `check_session`** Rejected for default
   one-shot CLI path: unnecessary statefulness and tighter coupling to session
   lifecycle concerns.
3. **No session layer, only stateless pipeline** Rejected for LSP target: cannot
   meet practical incremental latency goals.

---

## Bazel Mapping

1. `coppice_library` / `coppice_binary` / `coppice_test` remain action
   boundaries.
2. Bazel incrementality comes from target graph and cached artifacts, not
   cross-action in-memory sessions.
3. `check_session` is primarily for long-lived in-process clients (LSP, future
   watch/daemon modes), while Bazel actions can continue using one-shot
   pipeline/orchestration flows.

---

## Overall Recommendation

Adopt the three-layer check model:

1. `check_pipeline` (stateless shared engine),
2. `check_session` (incremental stateful wrapper for LSP),
3. `check_orchestration` (one-shot command policy).

Use explicit orchestration crate names (`check_orchestration`,
`build_orchestration`) instead of retaining a broad `driver` abstraction.
