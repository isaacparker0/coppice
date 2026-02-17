# Development

Run all commands from workspace root.

### Build and test

```bash
bazel build //...
```

```bash
bazel test //...
```

### Run the CLI

```bash
bazel run --run_in_cwd //compiler/cli:main -- \
  --workspace-root path/to/workspace \
  check
```

```bash
bazel run --run_in_cwd //compiler/cli:main -- \
  --workspace-root path/to/workspace \
  check path/inside/workspace/file.coppice
```
