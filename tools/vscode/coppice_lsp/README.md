# Coppice LSP VS Code Dev Extension

This extension is for local development/testing of the Coppice language server.

## One-time install (normal VS Code window, always on)

From the monorepo root:

```bash
bin/install-coppice-vscode-dev-extension
```

This symlinks the extension into your global VS Code extensions directory and
installs its dependencies.

After that, open the monorepo in your normal VS Code window and the extension
auto-activates on `*.copp` files.

When you change extension code, run `Developer: Reload Window` in VS Code.

## Run from source

1. Install extension dependencies:

```bash
cd tools/vscode/coppice_lsp
pnpm install
```

2. Open this folder (`tools/vscode/coppice_lsp`) in VS Code.
3. Press `F5` to launch an Extension Development Host.
4. In the Extension Development Host, open a Coppice workspace and edit `*.copp`
   files.

The extension starts `bin/coppice-lsp`, which runs:

```bash
bazel run --run_in_cwd //compiler/cli:main -- --workspace-root <workspace> lsp
```

## Notes

- This is a development client, not a published extension package.
- It assumes the opened workspace contains `bin/coppice-lsp`.
