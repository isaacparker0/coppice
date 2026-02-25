# Workspace Marker Implementation Plan

1. Add a shared workspace-root resolver in compiler frontend code with this
   precedence:
   1. `--workspace-root <path>` when provided.
   2. Otherwise, nearest ancestor `COPPICE_WORKSPACE` from the explicit check
      target path.
   3. Otherwise, nearest ancestor `COPPICE_WORKSPACE` from current working
      directory.
   4. If unresolved, return a workspace-root resolution failure.

2. Update `check_pipeline` entrypoints to call the shared resolver before
   workspace discovery, and remove assumptions that workspace root is the
   current directory unless explicitly overridden.

3. Keep package ownership logic unchanged: `.copp`, `.bin.copp`, and
   `.test.copp` files must still be owned by a package via ancestor
   `PACKAGE.copp` within resolved workspace root.

4. Update CLI `check`/`build`/other command paths that currently assume
   cwd-based workspace root so they use the shared resolver behavior
   consistently.

5. Update LSP server startup behavior so root selection uses the same resolver
   contract:
   1. Honor explicit `--workspace-root` when passed.
   2. Otherwise resolve via `COPPICE_WORKSPACE` from checked document/target
      context.
   3. Return deterministic root-resolution errors when unresolved.

6. Update VS Code extension wiring so it does not hardcode a single global
   workspace root that bypasses marker discovery by default.

7. Add `COPPICE_WORKSPACE` to each standalone diagnostics fixture workspace root
   (`tests/diagnostics/**/input`) so fixture-local `workspace/...` imports
   resolve against fixture root.

8. Add `COPPICE_WORKSPACE` to each standalone example root in `examples/` so
   examples and playground use authored, explicit roots without runtime file
   injection.

9. Decide repo-root marker policy and apply it once:
   1. If this repo should act as a Coppice workspace by default, add
      `COPPICE_WORKSPACE` at repo root.
   2. If not, leave repo root unmarked and rely on nested markers only.

10. Update developer documentation to state the root-resolution contract and
    marker placement rules:
    1. `--workspace-root` override semantics.
    2. Nearest-ancestor `COPPICE_WORKSPACE` discovery semantics.
    3. Separation of responsibilities: `COPPICE_WORKSPACE` for workspace root,
       `PACKAGE.copp` for package boundaries.
