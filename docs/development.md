# Development

Run all commands from a Coppice workspace. A workspace root is discovered via
nearest-ancestor `COPPICE_WORKSPACE` marker unless `--workspace-root` is passed.

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
  check path/inside/workspace/file.copp
```

```bash
# Uses COPPICE_WORKSPACE marker discovery
bazel run --run_in_cwd //compiler/cli:main -- \
  check
```
